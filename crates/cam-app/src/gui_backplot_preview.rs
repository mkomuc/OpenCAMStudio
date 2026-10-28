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

#[derive(Default)]
pub(super) struct OperationPreview {
    key: Option<PreviewKey>,
    jobs: JobService,
    cache: Option<cam_toolpath::OperationCache>,
    outcome: Option<crate::RunOutcome>,
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
        let reuse = self.operation_preview.key.as_ref().is_some_and(|old| {
            old.revision == key.revision && old.setup == key.setup && old.source == key.source
        });
        if !reuse {
            self.operation_preview.cache = Some(self.controller.operation_preview_cache());
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
            self.operation_preview.outcome = Some(result.outcome);
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
            .then_some(self.operation_preview.outcome.as_ref())
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
