//! One bounded worker lane, one replaceable pending request. Edits cancel all
//! phases; request/revision/fingerprint guards are checked before publication.
use crate::controller::{narrowest_cut_width, sim_profile, MAX_SIM_CELLS_ACROSS};
use crate::RunOutcome;
use cam_cldata::{Program, SpindleDir};
use cam_geo::{Polygon, Polyline};
use cam_model::{Document, Machine};
use cam_render::{mesh_vertices, MeshVertex, Scene, PART};
use cam_sim::{Collision, SimOptions, SimTool, SimulationCache};
use cam_toolpath::{
    build_job_cached, CancelToken, Diagnostic, DiagnosticCode, JobCompletion, OperationCache,
    PlanRequest,
};
use std::sync::{mpsc, Arc, Mutex};

#[path = "job_simulation.rs"]
pub(crate) mod simulation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobStamp {
    pub request_id: u64,
    pub revision: u64,
    pub input_fingerprint: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JobPhase {
    #[default]
    Queued,
    GeometryResolution,
    Generation,
    Verification,
    RenderPreparation,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobProgress {
    pub phase: JobPhase,
    pub completed: usize,
    pub total: usize,
    pub operation: Option<u32>,
}
#[derive(Clone, Default)]
pub(crate) struct Progress(Arc<Mutex<JobProgress>>);
impl Progress {
    pub fn set(&self, phase: JobPhase, completed: usize, total: usize, operation: Option<u32>) {
        *self.0.lock().unwrap() = JobProgress {
            phase,
            completed,
            total,
            operation,
        };
    }
    fn get(&self) -> JobProgress {
        self.0.lock().unwrap().clone()
    }
}

/// Only owned immutable domain data crosses the worker boundary. Imported
/// source bytes/topology remain Arc snapshots; CAD/GPU handles never cross it.
#[derive(Clone)]
pub(crate) struct JobInputs {
    pub document: Arc<Document>,
    pub machine: Machine,
    pub regions: Arc<Vec<Polygon>>,
    pub open_paths: Arc<Vec<Polyline>>,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub spindle_rpm: f64,
    pub tool_diameter: f64,
    pub frame_error: Option<String>,
    /// A playback prefix bypasses planning; it never replaces the verified job.
    pub preview_program: Option<Program>,
    pub simulation_accuracy: f32,
    /// Fixed from the full playback program so partial frames share one grid.
    pub preview_resolution: Option<f64>,
    /// Generate editable source paths without simulating stock removal.
    pub toolpath_preview: bool,
}
impl JobInputs {
    pub fn execute(
        &self,
        cancel: &CancelToken,
        cache: &mut OperationCache,
        progress: &Progress,
        simulation: &mut simulation::StockSimulationCache,
    ) -> RunOutcome {
        cache.reset_stats();
        if self.document.setups.is_empty() {
            return RunOutcome {
                completion: JobCompletion::Failed,
                diagnostics: vec![
                    Diagnostic::error("Create a setup before generating toolpaths.")
                        .with_code(DiagnosticCode::InvalidInput),
                ],
                ..Default::default()
            };
        }
        if cancel.is_cancelled() {
            return RunOutcome {
                completion: JobCompletion::Cancelled,
                ..Default::default()
            };
        }
        let mut job = if let Some(program) = &self.preview_program {
            cam_toolpath::JobResult {
                program: program.clone(),
                diagnostics: Vec::new(),
                completion: JobCompletion::Complete,
            }
        } else {
            progress.set(JobPhase::GeometryResolution, 0, 1, None);
            let setup_id = self.document.setup().id;
            let resolved = self.frame_error.clone().map_or_else(
                || {
                    self.machine
                        .validate()
                        .map_err(|error| {
                            Diagnostic::error(error.to_string())
                                .with_code(DiagnosticCode::InvalidInput)
                                .for_setup(setup_id)
                        })
                        .and_then(|()| {
                            crate::cad_operations::resolve_document_checked(&self.document, &|| {
                                cancel.is_cancelled()
                            })
                        })
                },
                |error| {
                    Err(Diagnostic::error(error)
                        .with_code(DiagnosticCode::GeometryUnresolved)
                        .for_setup(setup_id))
                },
            );
            if cancel.is_cancelled() {
                return RunOutcome {
                    completion: JobCompletion::Cancelled,
                    ..Default::default()
                };
            }
            let job = match resolved {
                Ok(document) => {
                    let total = document
                        .setup()
                        .operations
                        .iter()
                        .filter(|op| document.operation_is_included(setup_id, op.id()))
                        .count();
                    let mut completed = 0;
                    progress.set(JobPhase::Generation, 0, total, None);
                    let job = build_job_cached(
                        PlanRequest {
                            document: &document,
                            spindle_rpm: self.spindle_rpm,
                            dir: SpindleDir::Cw,
                            stock: Some(([self.min[0], self.min[1]], [self.max[0], self.max[1]])),
                            tool_change_height: document
                                .setup()
                                .resolved_tool_change_height(self.machine.envelope.max.z),
                        },
                        cancel,
                        cache,
                        |id| {
                            progress.set(JobPhase::Generation, completed, total, Some(id));
                            completed += 1;
                        },
                    );
                    progress.set(JobPhase::Generation, completed, total, None);
                    job
                }
                Err(error) => cam_toolpath::JobResult {
                    program: Program::new(),
                    diagnostics: vec![error],
                    completion: JobCompletion::Failed,
                },
            };
            job
        };
        let mut completion = job.completion;
        let stock = if !self.toolpath_preview
            && completion == JobCompletion::Complete
            && !cancel.is_cancelled()
        {
            match self.simulate_stock(&mut job.program, cancel, cache, progress, simulation) {
                Ok(stock) => stock,
                Err(error) => {
                    completion = JobCompletion::Failed;
                    job.diagnostics.push(error);
                    simulation::StockSimulation::default()
                }
            }
        } else {
            simulation::StockSimulation::default()
        };
        progress.set(JobPhase::RenderPreparation, 0, 1, None);
        let mut scene = if job.program.validate().is_ok() {
            Scene::from_program_controlled(&job.program, &|| cancel.is_cancelled())
                .unwrap_or_default()
        } else {
            Scene::new()
        };
        for strip in &mut scene.strips {
            if cancel.is_cancelled() {
                break;
            }
            for point in &mut strip.points {
                *point = self
                    .document
                    .setup()
                    .frame
                    .to_world_vector(point.map(f64::from))
                    .map(|n| n as f32);
            }
        }
        for cone in &mut scene.path_cones {
            if cancel.is_cancelled() {
                break;
            }
            let frame = &self.document.setup().frame;
            cone.at = frame
                .to_world_vector(cone.at.map(f64::from))
                .map(|n| n as f32);
            cone.direction = frame
                .to_world_vector(cone.direction.map(f64::from))
                .map(|n| n as f32);
        }
        for region in self.regions.iter() {
            if cancel.is_cancelled() {
                break;
            }
            scene.add_region(region, PART);
        }
        for path in self.open_paths.iter() {
            if cancel.is_cancelled() {
                break;
            }
            scene.add_open_path(path.points(), PART);
        }
        if cancel.is_cancelled() {
            completion = JobCompletion::Cancelled;
        }
        if !cancel.is_cancelled() {
            progress.set(JobPhase::RenderPreparation, 1, 1, None);
        }
        RunOutcome {
            completion,
            stamp: None,
            program: job.program,
            diagnostics: job.diagnostics,
            scene,
            initial_stock: stock.initial,
            stock_vertices: stock.vertices,
            stock_machined: stock.machined,
            stock_operations: stock.operations,
            stock_indices: stock.indices,
            collisions: stock.collisions,
        }
    }
}

struct JobRequest {
    stamp: JobStamp,
    inputs: JobInputs,
    cache: OperationCache,
}
struct Running {
    stamp: JobStamp,
    cancel: CancelToken,
    receiver: mpsc::Receiver<JobOutput>,
}
pub(crate) struct JobOutput {
    pub stamp: JobStamp,
    pub outcome: RunOutcome,
    pub cache: OperationCache,
}
#[cfg(test)]
type TestHook = Arc<dyn Fn(&CancelToken) + Send + Sync>;
#[derive(Default)]
pub(crate) struct JobService {
    simulation: Arc<Mutex<simulation::StockSimulationCache>>,
    serial: u64,
    expected: Option<JobStamp>,
    running: Option<Running>,
    queued: Option<JobRequest>,
    progress: Progress,
    #[cfg(test)]
    hook: Option<TestHook>,
}
impl JobService {
    pub fn start(
        &mut self,
        revision: u64,
        fingerprint: String,
        inputs: JobInputs,
        cache: OperationCache,
    ) -> JobStamp {
        self.cancel();
        self.serial = self.serial.checked_add(1).expect("request IDs exhausted");
        let stamp = JobStamp {
            request_id: self.serial,
            revision,
            input_fingerprint: fingerprint,
        };
        self.expected = Some(stamp.clone());
        self.queued = Some(JobRequest {
            stamp: stamp.clone(),
            inputs,
            cache,
        });
        self.progress = Progress::default();
        self.launch_queued();
        stamp
    }
    fn launch_queued(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Some(request) = self.queued.take() else {
            return;
        };
        let stamp = request.stamp.clone();
        let cancel = CancelToken::new();
        let token = cancel.clone();
        let progress = self.progress.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        let hook = self.hook.clone();
        let failure_sender = sender.clone();
        let setup_id = request.inputs.document.setup().id;
        let simulation = self.simulation.clone();
        let spawned = std::thread::Builder::new()
            .name("machining-job".into())
            .spawn(move || {
                let mut cache = request.cache;
                let mut outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    #[cfg(test)]
                    if let Some(hook) = hook {
                        hook(&token);
                    }
                    request.inputs.execute(
                        &token,
                        &mut cache,
                        &progress,
                        &mut simulation.lock().unwrap_or_else(|e| e.into_inner()),
                    )
                }))
                .unwrap_or_else(|_| RunOutcome {
                    completion: JobCompletion::Failed,
                    diagnostics: vec![Diagnostic::error("Machining worker failed; run again")
                        .with_code(DiagnosticCode::WorkerFailed)
                        .for_setup(setup_id)],
                    ..Default::default()
                });
                outcome.stamp = Some(request.stamp.clone());
                let _ = sender.send(JobOutput {
                    stamp: request.stamp,
                    outcome,
                    cache,
                });
            });
        if let Err(error) = spawned {
            let _ = failure_sender.send(JobOutput {
                stamp: stamp.clone(),
                cache: OperationCache::default(),
                outcome: RunOutcome {
                    completion: JobCompletion::Failed,
                    stamp: Some(stamp.clone()),
                    diagnostics: vec![Diagnostic::error(format!(
                        "Could not start machining worker: {error}"
                    ))
                    .with_code(DiagnosticCode::WorkerFailed)
                    .for_setup(setup_id)],
                    ..Default::default()
                },
            });
        }
        self.running = Some(Running {
            stamp,
            cancel,
            receiver,
        });
    }
    #[cfg(test)]
    pub fn set_test_hook(&mut self, hook: TestHook) {
        self.hook = Some(hook);
    }
    pub fn pending(&self) -> bool {
        self.expected.is_some()
    }
    pub fn progress(&self) -> Option<JobProgress> {
        self.pending().then(|| self.progress.get())
    }
    pub fn cancel(&mut self) {
        if let Some(running) = &self.running {
            running.cancel.cancel();
        }
        self.expected = None;
        self.queued = None;
    }
    pub fn poll(&mut self, revision: u64) -> Option<JobOutput> {
        let running = self.running.as_ref()?;
        let result = match running.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Some(JobOutput {
                stamp: running.stamp.clone(),
                cache: Default::default(),
                outcome: RunOutcome {
                    completion: JobCompletion::Failed,
                    diagnostics: vec![Diagnostic::error(
                        "Machining worker stopped without a result",
                    )
                    .with_code(DiagnosticCode::WorkerFailed)],
                    ..Default::default()
                },
            }),
        };
        self.running = None;
        let result = result.filter(|output| {
            self.expected.as_ref() == Some(&output.stamp) && output.stamp.revision == revision
        });
        if result.is_some() {
            self.expected = None;
        } else if self
            .expected
            .as_ref()
            .is_some_and(|stamp| stamp.revision != revision)
        {
            self.cancel();
        }
        self.launch_queued();
        if self.running.is_none() {
            self.expected = None;
        }
        result
    }
    /// Keep polling until a cancelled lane drains, even if no result can publish.
    pub fn needs_poll(&self) -> bool {
        self.running.is_some() || self.queued.is_some()
    }
}
impl Drop for JobService {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_result_requires_request_revision_and_fingerprint_match() {
        for mismatch in 0..3 {
            let stamp = JobStamp {
                request_id: 3,
                revision: 7,
                input_fingerprint: "abc".into(),
            };
            let mut wrong = stamp.clone();
            match mismatch {
                0 => wrong.request_id += 1,
                1 => wrong.revision += 1,
                _ => wrong.input_fingerprint = "def".into(),
            }
            let (sender, receiver) = mpsc::channel();
            sender
                .send(JobOutput {
                    stamp: wrong,
                    outcome: RunOutcome {
                        completion: JobCompletion::Complete,
                        ..Default::default()
                    },
                    cache: Default::default(),
                })
                .unwrap();
            let mut jobs = JobService::default();
            jobs.serial = 3;
            jobs.expected = Some(stamp.clone());
            jobs.running = Some(Running {
                stamp,
                receiver,
                cancel: CancelToken::new(),
            });
            assert!(jobs.poll(7).is_none());
            assert!(!jobs.pending());
        }
    }
}
