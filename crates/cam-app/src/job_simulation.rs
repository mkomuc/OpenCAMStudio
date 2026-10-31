//! Cancellable stock verification and mesh preparation.
use super::*;
use cam_sim::{Heightfield, SimResult};

#[derive(Default)]
pub(crate) struct StockSimulationCache {
    current: SimulationCache,
    previous: Option<(String, Heightfield)>,
}

#[derive(Default)]
pub(super) struct StockSimulation {
    pub initial: Option<(Vec<MeshVertex>, Vec<u32>)>,
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub machined: Vec<bool>,
    pub operations: Vec<Option<u32>>,
    pub cut_sources: Vec<Option<cam_sim::SurfaceLocation>>,
    pub collisions: Vec<Collision>,
}
impl JobInputs {
    pub(super) fn analyze_stock_removal(
        &self,
        cancel: &CancelToken,
        cache: &mut OperationCache,
        progress: &Progress,
    ) -> Result<Vec<cam_sim::RemovalSample>, Diagnostic> {
        if let Some(error) = &self.frame_error {
            return Err(Diagnostic::error(error.clone()));
        }
        let program = self
            .preview_program
            .as_ref()
            .ok_or_else(|| Diagnostic::error("Force analysis requires a simulation program."))?;
        let resolution = self
            .preview_resolution
            .unwrap_or_else(|| self.stock_resolution(program));
        let initial = if self.document.setup().stock == cam_model::Stock::PreviousSetup {
            if self
                .document
                .setups
                .first()
                .is_some_and(|s| s.id == self.document.setup().id)
            {
                return Err(Diagnostic::error(
                    "The first setup cannot inherit previous stock.",
                ));
            }
            Some(self.previous_stock(resolution, cancel, cache, progress)?)
        } else {
            None
        };
        let tools: Vec<_> = self
            .document
            .setup()
            .tools
            .iter()
            .map(|tool| {
                let outline = tool.profile();
                SimTool {
                    number: tool.number,
                    profile: sim_profile(tool),
                    noncutting_tip_radius: outline
                        .segs
                        .first()
                        .filter(|s| !s.cutting && s.end.y.abs() < 1e-9)
                        .map(|s| s.end.x),
                }
            })
            .collect();
        cam_sim::analyze_removal_controlled(
            program,
            self.min,
            self.max,
            &SimOptions {
                resolution,
                tool_radius: self.tool_diameter / 2.,
            },
            &tools,
            initial.as_ref(),
            &|| cancel.is_cancelled(),
            |done, total| progress.set(JobPhase::Verification, done, total, None),
        )
        .map_err(Diagnostic::error)
    }

    pub(crate) fn stock_resolution(&self, program: &Program) -> f64 {
        let span = (self.max[0] - self.min[0]).max(self.max[1] - self.min[1]);
        let setup = self.document.setup();
        let mut feature = narrowest_cut_width(program, &setup.tools, self.max[2]);
        for operation in &setup.operations {
            if let cam_model::Operation::Thread(thread) = operation {
                let included = program.steps().iter().any(|step| {
                    matches!(step,
                    cam_cldata::Step::Linear { tag, .. } | cam_cldata::Step::Arc { tag, .. }
                        if tag.op_id == thread.id)
                });
                if included {
                    feature = feature.min(thread.pitch);
                }
            }
        }
        simulation_resolution(span, feature, self.simulation_accuracy)
    }

    /// Simulate material removal for `program`: triangulate the remaining stock
    /// into render-ready vertices, indices and a machined-surface mask, and
    /// return any collisions found.
    /// Empty when there is no stock (no geometry loaded).
    pub(super) fn simulate_stock(
        &self,
        program: &mut Program,
        cancel: &CancelToken,
        cache: &mut OperationCache,
        progress: &Progress,
        simulation: &mut StockSimulationCache,
    ) -> Result<StockSimulation, Diagnostic> {
        if self.document.setup().stock == cam_model::Stock::PreviousSetup
            && self
                .document
                .setups
                .first()
                .is_some_and(|s| s.id == self.document.setup().id)
        {
            return Err(Diagnostic::error(
                "The first setup has no previous stock; choose box stock",
            )
            .with_code(DiagnosticCode::InvalidInput)
            .for_setup(self.document.setup().id));
        }
        let (min, max) = (self.min, self.max);
        if min[0] >= max[0] || min[1] >= max[1] {
            return Ok(StockSimulation::default());
        }
        let resolution = self
            .preview_resolution
            .unwrap_or_else(|| self.stock_resolution(program));
        let key = self.previous_stock_key(resolution);
        if let Some(key) = &key {
            if simulation
                .previous
                .as_ref()
                .is_none_or(|(saved, _)| saved != key)
            {
                let field = self.previous_stock(resolution, cancel, cache, progress)?;
                if cancel.is_cancelled() {
                    return Ok(StockSimulation::default());
                }
                simulation.previous = Some((key.clone(), field));
            }
        } else {
            simulation.previous = None;
        }
        let initial = key.as_ref().and(simulation.previous.as_ref());
        // Setup selection shows the input blank, while operation selection and
        // playback show the material remaining after the destination's cuts.
        let input_mesh = if let Some((_, field)) =
            initial.filter(|_| self.preview_program.is_none())
        {
            let Some(surface) = field.to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())
            else {
                return Ok(StockSimulation::default());
            };
            let Some(vertices) = self.world_stock_vertices(&surface, cancel) else {
                return Ok(StockSimulation::default());
            };
            Some((vertices, surface.indices))
        } else {
            None
        };
        simulation
            .current
            .set_surface_sources(self.stock_force_surfaces);
        let Some(sim) = self.simulate_field(
            program,
            resolution,
            initial.map(|(k, f)| (k.as_str(), f)),
            cancel,
            progress,
            &mut simulation.current,
        ) else {
            return Ok(StockSimulation::default());
        };
        progress.set(JobPhase::RenderPreparation, 0, 1, None);
        // Render the stock as a watertight solid down to its floor, so the full
        // thickness shows and cut walls shade crisply (not a smooth top sheet).
        let Some(surface) = sim
            .field
            .to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())
        else {
            return Ok(StockSimulation::default());
        };
        let Some(vertices) = self.world_stock_vertices(&surface, cancel) else {
            return Ok(StockSimulation::default());
        };
        Ok(StockSimulation {
            initial: input_mesh,
            vertices,
            indices: surface.indices,
            machined: surface.machined,
            operations: surface.operations,
            cut_sources: surface.cut_sources.unwrap_or_default(),
            collisions: sim.collisions,
        })
    }
    fn world_stock_vertices(
        &self,
        surface: &cam_sim::SurfaceMesh,
        cancel: &CancelToken,
    ) -> Option<Vec<MeshVertex>> {
        let mut vertices = mesh_vertices(&surface.positions, &surface.normals);
        for vertex in &mut vertices {
            if cancel.is_cancelled() {
                return None;
            }
            vertex.position = self
                .document
                .setup()
                .frame
                .to_world_vector(vertex.position.map(f64::from))
                .map(|n| n as f32);
            vertex.normal = self
                .document
                .setup()
                .frame
                .to_world_vector(vertex.normal.map(f64::from))
                .map(|n| n as f32);
        }
        Some(vertices)
    }

    fn simulate_field(
        &self,
        program: &mut Program,
        resolution: f64,
        initial: Option<(&str, &Heightfield)>,
        cancel: &CancelToken,
        progress: &Progress,
        simulation: &mut SimulationCache,
    ) -> Option<SimResult> {
        let tools = &self.document.setup().tools;
        let sim_tools: Vec<SimTool> = tools
            .iter()
            .map(|t| {
                let outline = t.profile();
                SimTool {
                    number: t.number,
                    profile: sim_profile(t),
                    noncutting_tip_radius: outline
                        .segs
                        .first()
                        .filter(|s| !s.cutting && s.end.y.abs() < 1e-9)
                        .map(|s| s.end.x),
                }
            })
            .collect();
        let options = SimOptions {
            resolution,
            tool_radius: tools.first().map_or(self.tool_diameter, |t| t.diameter) / 2.,
        };
        let cancelled = || cancel.is_cancelled();
        let report = |done, total| progress.set(JobPhase::Verification, done, total, None);
        let verify = |program: &Program, simulation: &mut SimulationCache| {
            if let Some((key, field)) = initial {
                simulation.simulate_from_stock_controlled(
                    program, self.min, self.max, &options, &sim_tools, field, key, &cancelled,
                    report,
                )
            } else {
                simulation.simulate_controlled(
                    program, self.min, self.max, &options, &sim_tools, &cancelled, report,
                )
            }
        };
        let mut result = verify(program, simulation)?;
        if self.preview_program.is_none() {
            if let Some(repaired) =
                self.reroute_parallel_stock_links(program, &result.collisions, cancel)
            {
                *program = repaired;
                // Re-run the unchanged collision checks on the actual output.
                // Clearance entries can only remove additional stock, so a
                // previously clear link cannot become blocked by this rewrite.
                result = verify(program, simulation)?;
            }
        }
        (!cancelled()).then_some(result)
    }

    /// CAD clearance is not proof of cleared stock: a ball nose leaves scallops
    /// and the next row may still be uncut. Preserve non-cutting link semantics
    /// and replace only blocked Parallel links with the normal clearance entry.
    fn reroute_parallel_stock_links(
        &self,
        program: &Program,
        collisions: &[Collision],
        cancel: &CancelToken,
    ) -> Option<Program> {
        use cam_cldata::{MoveKind, Point3, Step};
        let blocked: std::collections::BTreeSet<_> = collisions
            .iter()
            .filter(|c| c.kind == cam_sim::CollisionKind::NonCuttingThroughStock)
            .filter_map(|c| c.step_index)
            .collect();
        if blocked.is_empty() {
            return None;
        }
        let setup = self.document.setup();
        let policies: std::collections::BTreeMap<_, _> = setup
            .operations
            .iter()
            .filter_map(|op| {
                let cam_model::Operation::Parallel(parallel) = op else {
                    return None;
                };
                if !parallel.params.stay_down || parallel.params.lift_height <= 0. {
                    return None;
                }
                let clearance = match setup.height_settings.get(&op.id()) {
                    Some(settings) => settings.resolve(&self.document, op).ok()?.clearance,
                    None => setup.heights_for(op.id()).clearance,
                };
                Some((op.id(), (clearance, parallel.params.plunge_feed)))
            })
            .collect();
        let mut output = Program::new();
        let mut current: Option<Point3> = None;
        let mut changed = false;
        for (index, step) in program.steps().iter().enumerate() {
            if cancel.is_cancelled() {
                return None;
            }
            if let Step::Linear { to, tag, .. } = step {
                if tag.kind == MoveKind::Link && blocked.contains(&index) {
                    if let (Some(from), Some(&(clearance, feed))) =
                        (current, policies.get(&tag.op_id))
                    {
                        if clearance.is_finite()
                            && clearance > from.z.max(to.z).max(self.max[2])
                            && feed.is_finite()
                            && feed > 0.
                        {
                            output.push(Step::Rapid {
                                to: Point3::new(from.x, from.y, clearance),
                                tag: cam_cldata::Tag {
                                    kind: MoveKind::Retract,
                                    ..*tag
                                },
                            });
                            output.push(Step::Rapid {
                                to: Point3::new(to.x, to.y, clearance),
                                tag: *tag,
                            });
                            output.push(Step::Linear {
                                to: *to,
                                feed,
                                tag: cam_cldata::Tag {
                                    kind: MoveKind::Plunge,
                                    ..*tag
                                },
                            });
                            current = Some(*to);
                            changed = true;
                            continue;
                        }
                    }
                }
            }
            match step {
                Step::Rapid { to, .. } | Step::Linear { to, .. } => current = Some(*to),
                Step::Arc { end, .. } => current = Some(*end),
                Step::Drill(_) => current = None,
                _ => {}
            }
            output.push(step.clone());
        }
        changed.then_some(output)
    }

    fn previous_stock_key(&self, resolution: f64) -> Option<String> {
        if self.document.setup().stock != cam_model::Stock::PreviousSetup {
            return None;
        }
        let at = self
            .document
            .setups
            .iter()
            .position(|s| s.id == self.document.setup().id)?;
        let geometry = &self.document.geometry;
        let assets: Vec<_> = geometry
            .assets
            .iter()
            .map(|a| (a.id, a.revision, &a.source_fingerprint, &a.backend))
            .collect();
        let bytes = serde_json::to_vec(&(
            &self.document.setups[..at],
            &self.document.setup().frame,
            &self.document.excluded_operations,
            &self.document.disabled_origins,
            &geometry.bindings,
            &geometry.instances,
            assets,
            &self.document.cad_linked_operations,
            &geometry.drawing_regions,
            &geometry.drawing_paths,
            (
                &self.machine,
                self.spindle_rpm,
                self.tool_diameter,
                self.simulation_accuracy,
            ),
            (self.min, self.max, resolution),
        ))
        .expect("stock dependencies serialize");
        Some(cam_model::GeometryAsset::fingerprint_source(&bytes))
    }

    /// Generate only the linked predecessor chain. A separately defined blank
    /// breaks the chain. Never substitute raw stock for an unavailable result.
    fn previous_stock(
        &self,
        resolution: f64,
        cancel: &CancelToken,
        cache: &mut OperationCache,
        progress: &Progress,
    ) -> Result<Heightfield, Diagnostic> {
        let at = self
            .document
            .setups
            .iter()
            .position(|s| s.id == self.document.setup().id)
            .unwrap();
        let failure = |message: String| {
            Diagnostic::error(message)
                .with_code(DiagnosticCode::InvalidInput)
                .for_setup(self.document.setup().id)
        };
        let start = self.document.setups[..at]
            .iter()
            .rposition(|s| s.stock != cam_model::Stock::PreviousSetup)
            .ok_or_else(|| {
                failure("The first setup has no previous stock; choose box stock".into())
            })?;
        let mut remaining: Option<(Heightfield, cam_model::SetupFrame, [f64; 3])> = None;
        let transfer = |field: &Heightfield,
                        frame: &cam_model::SetupFrame,
                        min: [f64; 3],
                        target: &Self,
                        res: f64| {
            if frame.world_from_setup.rotation
                == target.document.setup().frame.world_from_setup.rotation
                && min == target.min
                && field.resolution() == res
            {
                return Some(field.clone());
            }
            let mut mesh = field.to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())?;
            for (i, p) in mesh.positions.iter_mut().enumerate() {
                if i % 1024 == 0 && cancel.is_cancelled() {
                    return None;
                }
                *p = target
                    .document
                    .setup()
                    .frame
                    .to_setup_vector(frame.to_world_vector(p.map(f64::from)))
                    .map(|n| n as f32);
            }
            Heightfield::from_stock_mesh_controlled(&mesh, target.min, target.max, res, &|| {
                cancel.is_cancelled()
            })
        };
        for setup in &self.document.setups[start..at] {
            if cancel.is_cancelled() {
                return Err(failure("Stock calculation cancelled".into()));
            }
            setup
                .frame
                .validate()
                .map_err(|e| failure(format!("{}: {e}", setup.name)))?;
            let mut document = (*self.document).clone();
            document.active_setup_id = setup.id;
            progress.set(JobPhase::GeometryResolution, 0, 1, None);
            // Resolution rotates and filters geometry for the active strategy.
            // Stock bounds must come from the complete source document first.
            let (min, max) = document.stock_bounds_for(document.setup());
            let document = crate::cad_operations::resolve_document_checked(&document, &|| {
                cancel.is_cancelled()
            })?;
            if (0..3).any(|i| !min[i].is_finite() || !max[i].is_finite() || min[i] >= max[i]) {
                return Err(failure(format!(
                    "{} has no valid stock to transfer",
                    setup.name
                )));
            }
            let mut job = build_job_cached(
                PlanRequest {
                    document: &document,
                    spindle_rpm: self.spindle_rpm,
                    dir: SpindleDir::Cw,
                    stock: Some(([min[0], min[1]], [max[0], max[1]])),
                    tool_change_height: setup
                        .resolved_tool_change_height(self.machine.envelope.max.z),
                },
                cancel,
                cache,
                |id| progress.set(JobPhase::Generation, 0, setup.operations.len(), Some(id)),
            );
            if job.completion != JobCompletion::Complete {
                let detail = job
                    .diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(failure(format!(
                    "Cannot calculate stock from {}: {detail}",
                    setup.name
                )));
            }
            let mut source = self.clone();
            source.document = Arc::new(document);
            // Predecessors are regenerated whole jobs, even when the destination
            // is a playback preview. Their links still require stock validation.
            source.preview_program = None;
            source.min = min;
            source.max = max;
            let res = source.stock_resolution(&job.program);
            let initial = match &remaining {
                Some((field, frame, floor)) => Some(
                    transfer(field, frame, *floor, &source, res)
                        .ok_or_else(|| failure("Stock transfer cancelled".into()))?,
                ),
                None => None,
            };
            let result = source
                .simulate_field(
                    &mut job.program,
                    res,
                    initial.as_ref().map(|f| ("predecessor", f)),
                    cancel,
                    progress,
                    &mut SimulationCache::default(),
                )
                .ok_or_else(|| failure("Stock simulation cancelled".into()))?;
            if !result.collisions.is_empty() {
                return Err(failure(format!(
                    "Cannot use stock from {}: resolve its simulation collisions first",
                    setup.name
                )));
            }
            remaining = Some((result.field, setup.frame.clone(), min));
        }
        let (field, frame, min) = remaining.unwrap();
        transfer(&field, &frame, min, self, resolution)
            .ok_or_else(|| failure("Stock transfer cancelled".into()))
    }
}

/// Keep playback and verified previews on the same fine grid. Compact flat
/// surfaces and analytic contours avoid tying GPU memory to the grid cell count.
fn simulation_resolution(span: f64, feature: f64, accuracy: f32) -> f64 {
    let accuracy = f64::from(crate::settings::clamp_simulation_accuracy(accuracy));
    let by_span = span / (600.0 * accuracy);
    let by_feature = feature / (6.0 * accuracy);
    by_span
        .min(by_feature)
        .clamp(0.02, 2.0)
        .max(span / MAX_SIM_CELLS_ACROSS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_machine, AppController};
    use cam_model::{DrillOp, Operation, Stock};

    fn drilled_blank() -> AppController {
        let mut app = AppController::new(default_machine());
        app.edit_stock(|s| {
            *s = Stock::FixedBox {
                size: [10., 10., 5.],
                model_position: [0., 0., 1.],
            }
        });
        app.edit_tool(0, |t| {
            t.diameter = 2.;
            t.flute_length = 12.;
        });
        app.edit_setup(|s| {
            s.heights = cam_model::Heights::new(5., 2., 0.);
            s.operations.push(Operation::Drill(DrillOp {
                id: 1,
                tool: 1,
                points: vec![[5., 5.]],
                depth: 3.,
                start_offset: 0.,
                peck: None,
                dwell: None,
                spindle_rpm: 1000.,
                tool_preset: None,
                cooling: Default::default(),
                entry_feeds: Default::default(),
                work_offset: 1,
                feed: 100.,
            }));
        });
        app
    }

    fn inherited_setup(app: &mut AppController) -> u64 {
        let id = app.add_setup();
        let mut settings = app.document().setup().clone();
        settings.stock = Stock::PreviousSetup;
        app.apply_setup_settings(settings).unwrap();
        id
    }

    #[test]
    fn force_analysis_uses_inherited_stock_and_does_not_replace_verification_meshes() {
        use cam_cldata::{MoveKind, Point3, ProgramBuilder};
        let mut app = drilled_blank();
        inherited_setup(&mut app);
        let before = app.document().clone();
        let mut inputs = app.job_inputs();
        inputs.removal_analysis = true;
        inputs.preview_resolution = Some(0.1);
        inputs.preview_program = Some(
            ProgramBuilder::new()
                .tool_change(1)
                .feed(100.)
                .rapid(Point3::new(5., 5., 3.), MoveKind::Link)
                .linear(Point3::new(5., 5., -2.), MoveKind::Plunge)
                .build(),
        );
        let run = inputs.execute(
            &CancelToken::new(),
            &mut OperationCache::default(),
            &Progress::default(),
            &mut StockSimulationCache::default(),
        );
        assert_eq!(
            run.completion,
            JobCompletion::Complete,
            "{:?}",
            run.diagnostics
        );
        assert!(!run.removal_samples.is_empty());
        assert!(run
            .removal_samples
            .iter()
            .all(|s| s.removed_volume_mm3 == 0.));
        assert!(run.stock_vertices.is_empty());
        assert_eq!(app.document(), &before);
        Arc::make_mut(&mut inputs.document).setup_mut().stock = Stock::FixedBox {
            size: [10., 10., 5.],
            model_position: [0., 0., 1.],
        };
        let fresh = inputs.execute(
            &CancelToken::new(),
            &mut OperationCache::default(),
            &Progress::default(),
            &mut StockSimulationCache::default(),
        );
        assert_eq!(fresh.completion, JobCompletion::Complete);
        assert!(fresh
            .removal_samples
            .iter()
            .any(|s| s.removed_volume_mm3 > 0.));
    }

    #[test]
    fn feed_height_below_stock_preserves_safe_pocket_entry_with_a_shifted_wcs() {
        use cam_cldata::{MoveKind, Step};
        use cam_geo::{Contour, Point};
        use cam_model::{
            HeightDefinition, HeightPlaneKind as P, HeightReference as R, OperationHeightSettings,
            PocketOp,
        };
        let mut app = drilled_blank();
        app.edit_tool(0, |tool| {
            tool.kind = cam_model::ToolKind::EndMill;
            tool.flute_length = 30.;
        });
        app.edit_setup(|setup| {
            setup.origin[2] = 20.;
            setup.stock = Stock::BoundingBox {
                x_offset: 10.,
                y_offset: 10.,
                top: 33.,
                thickness: 17.,
            };
            setup.operations = vec![Operation::Pocket(PocketOp {
                finishing: Default::default(),
                id: 1,
                tool: 1,
                work_offset: 1,
                spindle_rpm: 1000.,
                tool_preset: None,
                cooling: Default::default(),
                entry_feeds: Default::default(),
                boundary: Contour::new(vec![
                    Point::new(-6., -6.),
                    Point::new(6., -6.),
                    Point::new(6., 6.),
                    Point::new(-6., 6.),
                ]),
                islands: vec![],
                depth: 4.,
                start: None,
                end: None,
                clear: cam_model::ClearParams {
                    stepdown: 2.,
                    overlap: 0.4,
                    feed: 300.,
                    plunge_feed: 100.,
                    ..Default::default()
                },
            })];
            let mut settings = OperationHeightSettings::default();
            *settings.definition_mut(P::Top) = HeightDefinition::new(R::Origin, 0.);
            *settings.definition_mut(P::Bottom) = HeightDefinition::new(R::Plane(P::Top), -4.);
            setup.height_settings.insert(1, settings);
        });
        let heights = app.document().setup().height_settings[&1]
            .resolve(app.document(), &app.document().setup().operations[0])
            .unwrap();
        assert_eq!(heights.feed, Some(25.));
        assert_eq!(heights.stock_top, 33.);
        let result = app.run(&CancelToken::new());
        assert_eq!(
            result.completion,
            JobCompletion::Complete,
            "{:?}",
            result.diagnostics
        );
        assert!(result.collisions.is_empty(), "{:?}", result.collisions);
        assert!(result.program.steps().iter().any(|step| {
            matches!(step, Step::Linear { to, tag, .. }
                if tag.kind == MoveKind::Plunge && to.z == 16.)
        }));
        let program = result.program.clone();
        assert_eq!(app.run(&CancelToken::new()).program, program);
        let mut playback = app.job_inputs();
        playback.preview_program = Some(program);
        let result = playback.execute(
            &CancelToken::new(),
            &mut OperationCache::default(),
            &Progress::default(),
            &mut StockSimulationCache::default(),
        );
        assert!(result.collisions.is_empty(), "{:?}", result.collisions);
    }

    #[test]
    fn parallel_links_use_clearance_only_when_remaining_stock_blocks_them() {
        use cam_cldata::{MoveKind, Point3, Step, Tag};
        let mut app = drilled_blank();
        app.edit_tool(0, |tool| tool.kind = cam_model::ToolKind::BallMill);
        app.edit_setup(|setup| {
            setup.height_settings.clear();
            setup.operation_heights.clear();
            setup.operations = vec![Operation::Parallel(cam_model::ParallelOp {
                id: 7,
                tool: 1,
                work_offset: 1,
                spindle_rpm: 1000.,
                tool_preset: None,
                cooling: Default::default(),
                entry_feeds: Default::default(),
                params: cam_model::ParallelParams {
                    stay_down: true,
                    lift_height: 2.,
                    plunge_feed: 60.,
                    ..Default::default()
                },
            })];
        });
        let mut inputs = app.job_inputs();
        inputs.min = [-5., -5., -5.];
        inputs.max = [5., 5., 0.];
        let tag = |kind| Tag {
            op_id: 7,
            kind,
            pattern_id: Some(9),
            instance_index: Some(2),
        };
        let original = Program {
            steps: vec![
                Step::ToolChange { tool: 1 },
                Step::Rapid {
                    to: Point3::new(0., 0., 5.),
                    tag: tag(MoveKind::Link),
                },
                Step::Linear {
                    to: Point3::new(0., 0., -2.),
                    feed: 60.,
                    tag: tag(MoveKind::Plunge),
                },
                Step::Linear {
                    to: Point3::new(1., 0., -2.),
                    feed: 300.,
                    tag: tag(MoveKind::Cutting),
                },
                Step::Linear {
                    to: Point3::new(1., 2., -2.),
                    feed: 300.,
                    tag: tag(MoveKind::Link),
                },
                Step::Linear {
                    to: Point3::new(0., 2., -2.),
                    feed: 300.,
                    tag: tag(MoveKind::Cutting),
                },
                Step::Linear {
                    to: Point3::new(1., 2., -2.),
                    feed: 300.,
                    tag: tag(MoveKind::Link),
                },
                Step::Rapid {
                    to: Point3::new(1., 2., 5.),
                    tag: tag(MoveKind::Retract),
                },
            ],
        };
        let cancel = CancelToken::new();
        let simulate = |source: &JobInputs, program: &mut Program, initial| {
            source
                .simulate_field(
                    program,
                    0.2,
                    initial,
                    &cancel,
                    &Progress::default(),
                    &mut SimulationCache::default(),
                )
                .unwrap()
        };
        let mut preview = inputs.clone();
        preview.preview_program = Some(original.clone());
        let raw = simulate(&preview, &mut original.clone(), None);
        assert!(raw
            .collisions
            .iter()
            .any(|c| c.kind == cam_sim::CollisionKind::NonCuttingThroughStock));

        let mut repaired = original.clone();
        let verified = simulate(&inputs, &mut repaired, None);
        assert!(verified.is_clean(), "{:?}", verified.collisions);
        assert!(repaired.validate().is_ok());
        assert_eq!(repaired.len(), original.len() + 2);
        assert!(matches!(&repaired.steps()[4], Step::Rapid { to, tag: t }
            if *to == Point3::new(1., 0., 5.) && *t == tag(MoveKind::Retract)));
        assert!(
            matches!(&repaired.steps()[6], Step::Linear { to, feed, tag: t }
            if *to == Point3::new(1., 2., -2.) && *feed == 60. && *t == tag(MoveKind::Plunge))
        );
        assert_eq!(
            &repaired.steps()[8],
            &original.steps()[6],
            "a link through cleared stock stays down"
        );

        let mut prepared = Heightfield::new([-5., -5.], [5., 5.], 0.2, 0.);
        prepared.lower_rect([-5., -5.], [5., 5.], -3.);
        let mut pre_roughed = original.clone();
        assert!(simulate(&inputs, &mut pre_roughed, Some(("prepared", &prepared))).is_clean());
        assert_eq!(
            pre_roughed, original,
            "pre-roughed stock keeps all the requested stay-down links"
        );

        let mut unrelated = original.clone();
        if let Step::Linear { tag, .. } = &mut unrelated.steps[4] {
            tag.op_id = 99;
        }
        let failed = simulate(&inputs, &mut unrelated, None);
        assert!(
            failed.collisions.iter().any(|c| c.op_id == Some(99)),
            "other operations' failures remain visible"
        );
    }

    #[test]
    fn inherited_stock_uses_upstream_cuts_and_recalculates_after_edits_and_suppression() {
        let mut app = drilled_blank();
        let first = app.active_setup_id();
        let second = inherited_setup(&mut app);
        let mut cache = OperationCache::default();
        let mut simulation = StockSimulationCache::default();
        let progress = Progress::default();
        let cancel = CancelToken::new();
        for (depth, expected) in [(3., -3.), (2., -2.)] {
            app.activate_setup(first).unwrap();
            app.edit_operation(1, |op| {
                if let Operation::Drill(d) = op {
                    d.depth = depth;
                }
            });
            app.activate_setup(second).unwrap();
            let inputs = app.job_inputs();
            let result = inputs.execute(&cancel, &mut cache, &progress, &mut simulation);
            assert_eq!(
                result.completion,
                JobCompletion::Complete,
                "{:?}",
                result.diagnostics
            );
            let field = &simulation.previous.as_ref().unwrap().1;
            assert_eq!(field.sample(5., 5.), expected);
            assert_eq!(field.sample(1., 1.), 0.);
            assert!(
                result
                    .program
                    .steps()
                    .iter()
                    .all(|step| !matches!(step, cam_cldata::Step::Drill(_))),
                "upstream toolpaths must not enter the destination NC program"
            );
        }
        app.set_operation_excluded(1, true);
        let result = app
            .job_inputs()
            .execute(&cancel, &mut cache, &progress, &mut simulation);
        assert_eq!(
            result.completion,
            JobCompletion::Complete,
            "{:?}",
            result.diagnostics
        );
        assert_eq!(simulation.previous.as_ref().unwrap().1.sample(5., 5.), 0.);
    }

    #[test]
    fn inherited_chain_accumulates_cuts_and_follows_changed_axes() {
        let mut app = drilled_blank();
        inherited_setup(&mut app);
        app.edit_setup(|s| {
            s.operations.push(Operation::Drill(DrillOp {
                id: 2,
                tool: 1,
                points: vec![[2., 2.]],
                depth: 1.,
                start_offset: 0.,
                peck: None,
                dwell: None,
                spindle_rpm: 1000.,
                tool_preset: None,
                cooling: Default::default(),
                entry_feeds: Default::default(),
                work_offset: 1,
                feed: 100.,
            }))
        });
        inherited_setup(&mut app);
        let inputs = app.job_inputs();
        let field = inputs
            .previous_stock(
                0.1,
                &CancelToken::new(),
                &mut OperationCache::default(),
                &Progress::default(),
            )
            .unwrap();
        assert_eq!(field.sample(5., 5.), -3.);
        assert_eq!(field.sample(2., 2.), -1.);

        let mut settings = app.document().setup().clone();
        settings.frame = cam_model::SetupFrame::from_xy([1., 0., 0.], [0., -1., 0.]).unwrap();
        app.apply_setup_settings(settings).unwrap();
        let inputs = app.job_inputs();
        assert_eq!(inputs.min, [0., -10., 0.]);
        assert_eq!(inputs.max, [10., 0., 5.]);
        let mut field = inputs
            .previous_stock(
                0.1,
                &CancelToken::new(),
                &mut OperationCache::default(),
                &Progress::default(),
            )
            .unwrap();
        assert_eq!(field.sample(5., -5.), 5.);
        field.lower_rect([4.5, -5.5], [5.5, -4.5], 2.);
        assert_eq!(
            field.sample(5., -5.),
            0.,
            "the flipped hole must remain empty below its floor"
        );
    }

    #[test]
    fn invalid_upstream_generation_fails_inherited_stock() {
        let mut app = drilled_blank();
        app.edit_operation(1, |op| {
            if let Operation::Drill(d) = op {
                d.tool = 99;
            }
        });
        inherited_setup(&mut app);
        let result = app.run(&CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Failed);
        assert!(result.stock_vertices.is_empty());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Cannot calculate stock from")));
    }

    #[test]
    fn inherited_bounds_use_source_models_before_strategy_geometry_is_filtered() {
        use cam_model::*;
        let mut app = drilled_blank();
        app.edit_setup(|s| s.operations.clear());
        app.add_cad_asset(
            GeometryAsset {
                id: 1,
                revision: 1,
                name: "Offset block".into(),
                source_format: CadFormat::Step,
                source_data: Vec::new().into(),
                source_units: SourceUnits::Millimetres,
                backend: "analytic test".into(),
                import_options: None,
                import_diagnostics: vec![],
                source_fingerprint: String::new(),
                topology: TopologySnapshot {
                    bodies: vec![CadBody {
                        id: 1,
                        name: "block".into(),
                        valid_solid: true,
                        bounds: [[20., 30., -5.], [30., 40., 0.]],
                    }],
                    ..Default::default()
                }
                .into(),
            },
            vec![GeometryInstance {
                id: 1,
                asset_id: 1,
                body_id: 1,
                name: "block".into(),
                placement: Default::default(),
                visible: true,
            }],
        )
        .unwrap();
        app.edit_setup(|s| {
            s.model_instances = vec![1];
            s.frame = SetupFrame::from_xy([0., 1., 0.], [-1., 0., 0.]).unwrap();
        });
        // New setups deliberately have no selected models. Their inherited
        // blank must still be located against the source setup's model.
        inherited_setup(&mut app);
        inherited_setup(&mut app);
        let inputs = app.job_inputs();
        assert_eq!(inputs.min, [30., -30., -5.]);
        assert_eq!(inputs.max, [40., -20., 0.]);
        let field = inputs
            .previous_stock(
                0.1,
                &CancelToken::new(),
                &mut OperationCache::default(),
                &Progress::default(),
            )
            .unwrap();
        assert_eq!(field.sample(35., -25.), 0.);
        let mesh = field.to_solid_mesh(-5.);
        assert!(mesh
            .positions
            .iter()
            .all(|p| p[0] >= 30. && p[0] <= 40. && p[1] >= -30. && p[1] <= -20.));
    }

    #[test]
    fn accuracy_refines_stock_and_playback_within_the_cell_budget() {
        let coarse = simulation_resolution(100., 5., 0.5);
        let normal = simulation_resolution(100., 5., 1.);
        let fine = simulation_resolution(100., 5., 2.);
        assert!(coarse > normal && normal > fine);
        let narrow = simulation_resolution(100., 0.001, 2.);
        assert!(100. / narrow <= MAX_SIM_CELLS_ACROSS);
        let large = simulation_resolution(10_000., 0.001, 2.);
        assert!(10_000. / large <= MAX_SIM_CELLS_ACROSS);
    }
}
