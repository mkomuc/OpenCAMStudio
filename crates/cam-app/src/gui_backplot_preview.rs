//! Browser previews have their own worker: selecting an operation must neither
//! publish a partial export program nor depend on the last simulation's scope.
use super::*;
use crate::jobs::JobService;

#[derive(Clone, Debug, PartialEq)]
struct PreviewKey {
    revision: u64,
    setup: u64,
    through: u32,
    accuracy: f32,
    source: Option<crate::JobStamp>,
}

// Stock meshes and backplots can be large. Keep a bounded set of recently
// viewed operation prefixes, sharing the active result instead of cloning it.
const MAX_COMPLETED_PREVIEWS: usize = 8;

#[derive(Default)]
pub(super) struct OperationPreview {
    key: Option<PreviewKey>,
    jobs: JobService,
    cache: Option<cam_toolpath::OperationCache>,
    cache_inputs: Option<PreviewKey>,
    outcome: Option<Arc<crate::RunOutcome>>,
    completed: Vec<(PreviewKey, Arc<crate::RunOutcome>)>,
}

impl App {
    fn operation_preview_key(&self) -> Option<PreviewKey> {
        if self.model_view.is_some()
            || self.operation_preview_active()
            || self.pattern_draft.is_some()
            || self
                .hidden_setups
                .contains(&self.controller.active_setup_id())
        {
            return None;
        }
        let through = self
            .controller
            .document()
            .setup()
            .operations
            .iter()
            .rev()
            .map(Operation::id)
            .find(|id| self.focus_ops.contains(id) && !self.hidden_ops.contains(id))?;
        Some(PreviewKey {
            revision: self.controller.revision(),
            setup: self.controller.active_setup_id(),
            through,
            accuracy: self.settings.simulation.accuracy,
            source: self.controller.outcome().and_then(|o| o.stamp.clone()),
        })
    }

    pub(super) fn sync_operation_preview(&mut self) {
        self.operation_preview.completed.retain(|(key, _)| {
            key.revision == self.controller.revision()
                && key.accuracy == self.settings.simulation.accuracy
                && key.source == self.controller.outcome().and_then(|o| o.stamp.clone())
        });
        // Verification/simulation has priority over background browser work.
        if self.simulation.is_some() || self.controller.run_pending() {
            self.operation_preview.jobs.cancel();
            self.operation_preview.key = None;
            self.operation_preview.outcome = None;
            return;
        }
        let key = self.operation_preview_key();
        if key == self.operation_preview.key {
            return;
        }
        self.operation_preview.jobs.cancel();
        self.operation_preview.outcome = None;
        let Some(key) = key else {
            self.operation_preview.key = None;
            return;
        };
        if let Some(index) = self
            .operation_preview
            .completed
            .iter()
            .position(|(cached, _)| *cached == key)
        {
            let cached = self.operation_preview.completed.remove(index);
            self.operation_preview.outcome = Some(cached.1.clone());
            self.operation_preview.completed.push(cached);
            self.operation_preview.key = Some(key);
            return;
        }
        let reuse = self
            .operation_preview
            .cache_inputs
            .as_ref()
            .is_some_and(|old| {
                old.revision == key.revision && old.setup == key.setup && old.source == key.source
            });
        if !reuse {
            self.operation_preview.cache = Some(self.controller.operation_preview_cache());
            self.operation_preview.cache_inputs = Some(key.clone());
        }
        let mut inputs = self.controller.job_inputs();
        inputs.simulation_accuracy = key.accuracy;
        let document = Arc::make_mut(&mut inputs.document);
        let later: Vec<_> = document
            .setup()
            .operations
            .iter()
            .skip_while(|op| op.id() != key.through)
            .skip(1)
            .map(Operation::id)
            .collect();
        document.excluded_operations.extend(later);
        self.operation_preview.jobs.start(
            key.revision,
            format!("operation-preview:{}:{}", key.setup, key.through),
            inputs,
            self.operation_preview.cache.clone().unwrap_or_default(),
        );
        self.operation_preview.key = Some(key);
    }

    pub(super) fn poll_operation_preview(&mut self) {
        if let Some(result) = self.operation_preview.jobs.poll(self.controller.revision()) {
            self.operation_preview.cache = Some(result.cache);
            let outcome = Arc::new(result.outcome);
            if outcome.completion == JobCompletion::Complete {
                if let Some(key) = &self.operation_preview.key {
                    if self.operation_preview.completed.len() >= MAX_COMPLETED_PREVIEWS {
                        self.operation_preview.completed.remove(0);
                    }
                    self.operation_preview
                        .completed
                        .push((key.clone(), outcome.clone()));
                }
            }
            self.operation_preview.outcome = Some(outcome);
        }
    }

    pub(super) fn cancel_operation_preview(&mut self) {
        self.operation_preview.jobs.cancel();
        self.operation_preview.outcome = None;
        // Keep the current key so the next UI event does not restart a preview
        // the user explicitly cancelled. A new selection/edit can request one.
        self.operation_preview.key = self.operation_preview_key();
    }

    pub(super) fn operation_preview_outcome(&self) -> Option<&crate::RunOutcome> {
        (self.operation_preview_key() == self.operation_preview.key)
            .then_some(self.operation_preview.outcome.as_deref())
            .flatten()
    }

    #[cfg(test)]
    pub(super) fn drain_operation_preview(&mut self) {
        self.sync_operation_preview();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while self.operation_preview.needs_poll() {
            let _ = self.update(Message::PollRun);
            assert!(
                std::time::Instant::now() < deadline,
                "operation preview timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

impl OperationPreview {
    pub(super) fn progress(&self) -> Option<crate::JobProgress> {
        self.jobs.progress()
    }

    pub(super) fn needs_poll(&self) -> bool {
        self.jobs.needs_poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview_app() -> (App, [u32; 2]) {
        let (mut app, _) = App::new();
        app.settings = crate::Settings::default();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.setup_draft = None;
        app.model_view = None;
        app.inspector_active = false;
        let ids = [10., 25.].map(|x| {
            app.controller
                .add_operation(Operation::Drill(cam_model::DrillOp {
                    id: 0,
                    tool: 1,
                    points: vec![[x, 10.]],
                    depth: 2.,
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
            let Selection::Operation(id) = app.controller.selection() else {
                panic!()
            };
            id
        });
        (app, ids)
    }

    fn focus(app: &mut App, id: u32) {
        app.controller.select(Selection::Operation(id));
        app.focus_ops = BTreeSet::from([id]);
        app.sync_operation_preview();
    }

    #[test]
    fn completed_operation_previews_survive_switching_and_unmodified_editor() {
        let (mut app, ids) = preview_app();
        let mut outcomes = Vec::new();
        for id in ids {
            focus(&mut app, id);
            app.drain_operation_preview();
            let outcome = app.operation_preview.outcome.clone().unwrap();
            assert_eq!(
                outcome.completion,
                JobCompletion::Complete,
                "{:?}",
                outcome.diagnostics
            );
            outcomes.push(outcome);
        }
        // Restoring a preview must publish exactly the original result without
        // submitting generation, verification or render preparation again.
        for (id, expected) in ids.into_iter().zip(&outcomes) {
            focus(&mut app, id);
            assert!(app.operation_preview.progress().is_none());
            assert!(Arc::ptr_eq(
                app.operation_preview.outcome.as_ref().unwrap(),
                expected
            ));
        }
        app.begin_operation_draft(ids[1]);
        app.sync_operation_preview();
        assert!(app.operation_preview_outcome().is_none());
        let _ = app.update(Message::CancelOperationDraft);
        focus(&mut app, ids[1]);
        assert!(app.operation_preview.progress().is_none());
        assert!(Arc::ptr_eq(
            app.operation_preview.outcome.as_ref().unwrap(),
            &outcomes[1]
        ));

        app.focus_ops.clear();
        app.sync_operation_preview();
        focus(&mut app, ids[0]);
        assert!(app.operation_preview.progress().is_none());
        assert!(Arc::ptr_eq(
            app.operation_preview.outcome.as_ref().unwrap(),
            &outcomes[0]
        ));
        assert!(
            app.controller.outcome().is_none(),
            "browser work must not publish an export program"
        );
    }

    #[test]
    fn operation_preview_cache_invalidates_on_edits_accuracy_and_new_source() {
        let (mut app, ids) = preview_app();
        focus(&mut app, ids[0]);
        app.drain_operation_preview();
        let initial = app.operation_preview.outcome.clone().unwrap();
        app.settings.simulation.accuracy = 0.5;
        focus(&mut app, ids[0]);
        assert!(app.operation_preview_outcome().is_none());
        assert!(app.operation_preview.progress().is_some());
        app.drain_operation_preview();
        assert!(!Arc::ptr_eq(
            app.operation_preview.outcome.as_ref().unwrap(),
            &initial
        ));
        let accurate = app.operation_preview.outcome.clone().unwrap();
        app.controller.edit_operation(ids[0], |op| {
            let Operation::Drill(op) = op else { panic!() };
            op.depth = 1.;
        });
        focus(&mut app, ids[0]);
        assert!(app.operation_preview.completed.is_empty());
        assert!(app.operation_preview_outcome().is_none());
        app.drain_operation_preview();
        assert!(!Arc::ptr_eq(
            app.operation_preview.outcome.as_ref().unwrap(),
            &accurate
        ));

        app.controller.start_run();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.controller.run_needs_poll() {
            app.controller.poll_run();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        focus(&mut app, ids[0]);
        assert!(app.operation_preview.completed.is_empty());
        assert!(app.operation_preview_outcome().is_none());
        app.drain_operation_preview();
    }
}
