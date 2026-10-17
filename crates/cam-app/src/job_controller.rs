//! Submit snapshots and publish only matching service results.
use super::*;
use crate::jobs::{JobInputs, JobProgress, JobStamp};
use std::sync::Arc;

impl AppController {
    /// Session revision increases through edits and undo/redo; it never rolls back.
    pub fn revision(&self) -> u64 {
        self.document_epoch
    }

    /// Change simulation quality without modifying the machining document or history.
    /// Cancel old work so a result with the previous grid cannot publish afterwards.
    pub fn set_simulation_accuracy(&mut self, value: f32) -> bool {
        let value = crate::settings::clamp_simulation_accuracy(value);
        if self.simulation_accuracy == value {
            return false;
        }
        self.simulation_accuracy = value;
        self.cancel_run();
        true
    }

    pub(crate) fn job_inputs(&self) -> JobInputs {
        let mut document = self.document().clone();
        let excluded = document.excluded_operations.clone();
        let disabled = document
            .disabled_origins
            .get(&self.active_setup_id)
            .cloned()
            .unwrap_or_default();
        document
            .setup_mut()
            .operations
            .retain(|o| !excluded.contains(&o.id()) && !disabled.contains(&o.work_offset()));
        let ids: BTreeSet<_> = document
            .setup()
            .operations
            .iter()
            .map(Operation::id)
            .collect();
        document.geometry.bindings.retain(|id, _| ids.contains(id));
        let (min, max) = self.stock_box();
        JobInputs {
            document: Arc::new(document),
            machine: self.machine.clone(),
            regions: Arc::new(self.regions.clone()),
            open_paths: Arc::new(self.open_paths.clone()),
            min,
            max,
            spindle_rpm: self.defaults.spindle_rpm,
            tool_diameter: self.defaults.tool_diameter,
            frame_error: self.validate_setup_frame(self.setup()).err(),
            preview_program: None,
            preview_resolution: None,
            simulation_accuracy: self.simulation_accuracy,
        }
    }
    /// Generation, verification and mesh preparation run in the bounded worker lane.
    pub fn start_run(&mut self) -> JobStamp {
        let inputs = self.job_inputs();
        self.submit_run(inputs)
    }

    /// Run the active setup or one of its included operations. Filtering the
    /// immutable snapshot preserves document order, suppression and undo history.
    pub fn start_toolpath_run(
        &mut self,
        operation: Option<u32>,
        recompute: bool,
    ) -> Result<JobStamp, String> {
        let mut inputs = self.job_inputs();
        if let Some(id) = operation {
            let document = Arc::make_mut(&mut inputs.document);
            document.setup_mut().operations.retain(|op| op.id() == id);
            document.geometry.bindings.retain(|op, _| *op == id);
        }
        let ids: Vec<_> = inputs
            .document
            .setup()
            .operations
            .iter()
            .map(Operation::id)
            .collect();
        if ids.is_empty() {
            return Err(
                "No included toolpaths to process. The operation or its origin may be suppressed."
                    .into(),
            );
        }
        if recompute {
            self.operation_cache.invalidate_operations(&ids);
        }
        Ok(self.submit_run(inputs))
    }

    fn submit_run(&mut self, inputs: JobInputs) -> JobStamp {
        // Source blobs are represented by immutable asset revisions/content hashes,
        // not serialized again on every submit. Revisions + exact stamp equality
        // provide the publication guard; this hash is provenance, not cryptography.
        let geometry = &inputs.document.geometry;
        let assets: Vec<_> = geometry
            .assets
            .iter()
            .map(|a| (a.id, a.revision, &a.source_fingerprint))
            .collect();
        let bytes = serde_json::to_vec(&(
            inputs.document.setup(),
            &geometry.bindings,
            &inputs.document.cad_linked_operations,
            &geometry.instances,
            assets,
            &geometry.drawing_regions,
            &geometry.drawing_paths,
            &inputs.machine,
            inputs.spindle_rpm,
            inputs.tool_diameter,
            inputs.simulation_accuracy,
            inputs.min,
            inputs.max,
            self.post_kind,
        ))
        .expect("job inputs serialize");
        let fingerprint = cam_model::GeometryAsset::fingerprint_source(&bytes);
        self.outcome = None;
        self.nc = None;
        self.jobs.start(
            self.document_epoch,
            fingerprint,
            inputs,
            self.operation_cache.clone(),
        )
    }
    #[cfg(test)]
    pub(crate) fn set_run_test_hook(&mut self, hook: Arc<dyn Fn(&CancelToken) + Send + Sync>) {
        self.jobs.set_test_hook(hook);
    }
    /// Returns true only when a current result was installed.
    pub fn poll_run(&mut self) -> bool {
        let Some(result) = self.jobs.poll(self.document_epoch) else {
            return false;
        };
        self.operation_cache = result.cache;
        self.outcome = Some(result.outcome);
        self.nc = None;
        true
    }
    pub fn run_pending(&self) -> bool {
        self.jobs.pending()
    }
    pub fn run_needs_poll(&self) -> bool {
        self.jobs.needs_poll()
    }
    pub fn run_progress(&self) -> Option<JobProgress> {
        self.jobs.progress()
    }
    pub fn cancel_run(&mut self) {
        self.jobs.cancel();
        self.outcome = None;
        self.nc = None;
    }
    pub fn cache_stats(&self) -> cam_toolpath::CacheStats {
        self.operation_cache.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Mutex,
    };
    use std::time::{Duration, Instant};

    fn app() -> AppController {
        let mut app = AppController::new(crate::default_machine());
        app.edit_stock(|stock| {
            *stock = Stock::FixedBox {
                size: [8., 8., 4.],
                model_position: [0.; 3],
            }
        });
        for point in [[2., 2.], [6., 6.]] {
            app.add_operation(Operation::Drill(DrillOp {
                id: 0,
                tool: 1,
                points: vec![point],
                depth: 2.,
                start_offset: 0.,
                peck: None,
                dwell: None,
                spindle_rpm: 1000.,
                tool_preset: None,
                work_offset: 1,
                feed: 100.,
            }));
        }
        app
    }
    fn drain(app: &mut AppController) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while app.run_needs_poll() {
            app.poll_run();
            assert!(Instant::now() < deadline, "worker did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn gate(app: &mut AppController) -> (mpsc::Receiver<CancelToken>, mpsc::Sender<()>) {
        let (entered, receiver) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let calls = AtomicUsize::new(0);
        app.set_run_test_hook(Arc::new(move |token| {
            if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                entered.send(token.clone()).unwrap();
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(15))
                    .unwrap();
            }
        }));
        (receiver, release)
    }
    #[test]
    fn simulation_accuracy_changes_worker_inputs_without_editing_the_project() {
        let mut app = app();
        let document = app.document().clone();
        let revision = app.revision();
        assert!(!app.set_simulation_accuracy(1.0));
        assert!(app.set_simulation_accuracy(2.0));
        assert_eq!(app.job_inputs().simulation_accuracy, 2.0);
        assert_eq!(app.document(), &document);
        assert_eq!(app.revision(), revision);
        app.start_run();
        assert!(app.set_simulation_accuracy(0.5));
        drain(&mut app);
        assert!(
            app.outcome().is_none(),
            "a previous-quality result cannot publish"
        );
        assert_eq!(app.job_inputs().simulation_accuracy, 0.5);
    }

    #[test]
    fn context_jobs_scope_generation_export_and_forced_recompute() {
        let mut app = app();
        let original = app.document().clone();
        app.start_toolpath_run(Some(1), false).unwrap();
        drain(&mut app);
        let outcome = app.outcome().unwrap();
        assert_eq!(outcome.completion, JobCompletion::Complete);
        assert!(outcome
            .scene
            .strips
            .iter()
            .filter_map(|s| s.op)
            .all(|id| id == 1));
        assert!(app.export_nc().is_ok());
        assert_eq!(app.document(), &original);
        app.start_toolpath_run(None, false).unwrap();
        drain(&mut app);
        assert_eq!(
            app.cache_stats(),
            cam_toolpath::CacheStats {
                reused: 1,
                computed: 1
            }
        );
        app.start_toolpath_run(Some(1), true).unwrap();
        drain(&mut app);
        assert_eq!(
            app.cache_stats(),
            cam_toolpath::CacheStats {
                reused: 0,
                computed: 1
            }
        );
        app.start_toolpath_run(None, false).unwrap();
        drain(&mut app);
        assert_eq!(app.cache_stats().reused, 2);
        app.set_operation_excluded(1, true);
        assert!(app.start_toolpath_run(Some(1), false).is_err());
        app.start_toolpath_run(None, true).unwrap();
        drain(&mut app);
        assert!(app
            .outcome()
            .unwrap()
            .scene
            .strips
            .iter()
            .filter_map(|s| s.op)
            .all(|id| id == 0));
        app.set_origin_disabled(1, true);
        assert!(app.start_toolpath_run(None, false).is_err());
    }
    #[test]
    fn long_worker_leaves_controller_responsive_and_edit_cancels_it() {
        let mut app = app();
        let (entered, release) = gate(&mut app);
        let stamp = app.start_run();
        let token = entered.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(app.run_pending());
        assert!(app.run_progress().is_some());
        app.select(Selection::Setup); // completes while worker is deliberately blocked
        assert_eq!(app.revision(), stamp.revision);
        app.edit_operation(0, |op| {
            if let Operation::Drill(d) = op {
                d.depth = 1.;
            }
        });
        assert!(app.revision() > stamp.revision);
        assert!(token.is_cancelled());
        assert_eq!(app.export_nc(), Err(ExportError::NothingToExport));
        release.send(()).unwrap();
        drain(&mut app);
        assert!(
            app.outcome().is_none(),
            "late cancelled result cannot publish"
        );
        assert!(!app.run_pending());
    }
    #[test]
    fn replacement_job_is_latest_and_only_it_can_publish() {
        let mut app = app();
        let (entered, release) = gate(&mut app);
        let old = app.start_run();
        let token = entered.recv_timeout(Duration::from_secs(5)).unwrap();
        app.edit_operation(0, |op| {
            if let Operation::Drill(d) = op {
                d.depth = 1.;
            }
        });
        let new = app.start_run();
        assert!(new.request_id > old.request_id);
        assert_ne!(new.input_fingerprint, old.input_fingerprint);
        assert!(token.is_cancelled());
        release.send(()).unwrap();
        drain(&mut app);
        assert_eq!(app.outcome().unwrap().completion, JobCompletion::Complete);
        let asynchronous = app.outcome().unwrap().program.clone();
        assert_eq!(asynchronous, app.run(&CancelToken::new()).program);
    }
    #[test]
    fn explicit_cancel_and_machine_post_changes_reject_late_results() {
        for change in 0..3 {
            let mut app = app();
            let (entered, release) = gate(&mut app);
            app.start_run();
            let token = entered.recv_timeout(Duration::from_secs(5)).unwrap();
            match change {
                0 => app.cancel_run(),
                1 => app.edit_machine(|m| m.envelope.max.z += 1.),
                _ => app.set_post_kind(PostKind::Fanuc),
            }
            assert!(token.is_cancelled());
            release.send(()).unwrap();
            drain(&mut app);
            assert!(app.outcome().is_none());
            assert_eq!(app.export_nc(), Err(ExportError::NothingToExport));
        }
    }
    #[test]
    fn caches_track_actual_dependencies_and_reassemble_execution_order() {
        let mut app = app();
        let first = app.run(&CancelToken::new()).program.clone();
        assert_eq!(app.cache_stats().computed, 2);
        assert_eq!(app.run(&CancelToken::new()).program, first);
        assert_eq!(app.cache_stats().reused, 2);
        app.edit_operation(0, |op| {
            if let Operation::Drill(d) = op {
                d.depth = 1.;
            }
        });
        app.run(&CancelToken::new());
        assert_eq!(
            app.cache_stats(),
            cam_toolpath::CacheStats {
                reused: 1,
                computed: 1
            }
        );
        assert!(app.undo());
        assert_eq!(app.run(&CancelToken::new()).program, first);
        assert_eq!(app.cache_stats().reused, 2);
        assert!(app.redo());
        app.run(&CancelToken::new());
        assert_eq!(app.cache_stats().reused, 2);
        app.move_operation(1, true);
        let reordered = app.run(&CancelToken::new()).program.clone();
        assert_eq!(app.cache_stats().reused, 2);
        assert_ne!(reordered, first);
        app.edit_tool(0, |t| t.diameter += 0.5);
        app.run(&CancelToken::new());
        assert_eq!(app.cache_stats().computed, 2);
        app.edit_heights(|h| h.clearance += 1.);
        app.run(&CancelToken::new());
        assert_eq!(
            app.cache_stats().reused,
            2,
            "operation reference heights are independent of legacy setup defaults"
        );
        app.edit_stock(|s| {
            if let Stock::FixedBox { size, .. } = s {
                size[0] += 1.;
            }
        });
        app.run(&CancelToken::new());
        assert_eq!(app.cache_stats().computed, 2);
        app.edit_machine(|m| m.envelope.max.z += 10.);
        assert!(app.outcome().is_none());
        app.run(&CancelToken::new());
        assert_eq!(
            app.cache_stats().reused,
            2,
            "machine changes rebuild transitions, not cutting fragments"
        );
    }
    #[test]
    fn suppression_is_manufacturing_state_across_history_setups_and_reopen() {
        let mut app = app();
        app.set_operation_excluded(0, true);
        assert!(app.undo());
        assert!(!app.is_operation_excluded(0));
        assert!(app.redo());
        assert!(app.is_operation_excluded(0));
        let first_setup = app.active_setup_id();
        app.set_origin_disabled(1, true);
        let second = app.add_setup();
        assert!(!app.is_origin_disabled(1));
        app.activate_setup(first_setup).unwrap();
        assert!(app.is_origin_disabled(1));
        let path = std::env::temp_dir().join(format!("m1-suppression-{}.ocam", std::process::id()));
        app.save_project(&path).unwrap();
        let mut reopened = AppController::new(crate::default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(reopened.is_operation_excluded(0));
        assert!(reopened.is_origin_disabled(1));
        reopened.activate_setup(second).unwrap();
        assert!(!reopened.is_origin_disabled(1));
    }
    #[test]
    fn deleted_and_undone_ids_are_not_reused() {
        let mut app = app();
        let template = app.operation(1).unwrap().clone();
        app.select(Selection::Operation(1));
        app.delete_selected_operation();
        app.add_operation(template.clone());
        assert_eq!(app.selected_operation().unwrap().id(), 2);
        assert!(app.undo());
        app.add_operation(template);
        assert_eq!(app.selected_operation().unwrap().id(), 3);
        let second = app.add_setup();
        app.delete_setup(second).unwrap();
        assert!(app.add_setup() > second);
    }
    #[test]
    fn worker_panics_fail_closed_with_a_diagnostic_code() {
        let mut app = app();
        app.set_run_test_hook(Arc::new(|_| panic!("injected worker failure")));
        app.start_run();
        drain(&mut app);
        let result = app.outcome().unwrap();
        assert_eq!(result.completion, JobCompletion::Failed);
        assert_eq!(
            result.diagnostics[0].code,
            cam_toolpath::DiagnosticCode::WorkerFailed
        );
        assert_eq!(app.export_nc(), Err(ExportError::HasErrors));
    }
    #[test]
    fn repeated_requests_coalesce_to_one_queued_worker() {
        let mut app = app();
        let (entered, release) = gate(&mut app);
        let first = app.start_run();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut newest = first;
        for _ in 0..100 {
            newest = app.start_run();
        }
        release.send(()).unwrap();
        drain(&mut app);
        assert_eq!(app.outcome().unwrap().stamp.as_ref(), Some(&newest));
        assert_eq!(app.outcome().unwrap().completion, JobCompletion::Complete);
    }
    #[test]
    fn save_after_undo_preserves_id_marks_on_reopen() {
        let mut app = app();
        let template = app.operation(1).unwrap().clone();
        app.add_operation(template.clone()); // allocate 2
        app.undo();
        let path = std::env::temp_dir().join(format!("m1-id-marks-{}.ocam", std::process::id()));
        app.save_project(&path).unwrap();
        let mut reopened = AppController::new(crate::default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        reopened.add_operation(template);
        assert_eq!(reopened.selected_operation().unwrap().id(), 3);
    }
    #[test]
    fn unresolved_geometry_diagnostics_preserve_operation_ownership() {
        let mut app = app();
        app.edit_document(|doc| {
            doc.cad_linked_operations.insert(0);
        });
        let setup = app.active_setup_id();
        let result = app.run(&CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Failed);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(
            diagnostic.code,
            cam_toolpath::DiagnosticCode::GeometryUnresolved
        );
        assert_eq!(diagnostic.source.setup, Some(setup));
        assert_eq!(diagnostic.source.operation, Some(0));
        assert_eq!(diagnostic.op, Some(0));
    }
}
