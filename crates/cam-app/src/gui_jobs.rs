//! Background job presentation and output view.
use super::*;
use iced::widget::column;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolpathAction {
    Recompute,
    Simulate,
    Export,
}

impl App {
    /// Submit a snapshot; the event loop never runs machining services inline.
    pub(super) fn rerun(&mut self) {
        self.pending_toolpath_export = None;
        self.pending_simulation = None;
        self.simulation = None;
        if self.controller.document().setup().operations.is_empty() {
            self.controller.cancel_run();
            self.status = "No operations to run.".into();
            return;
        }
        self.controller.start_run();
        self.status = "Generating toolpaths…".into();
    }

    pub(super) fn toolpath_command(
        &mut self,
        node: TreeNode,
        action: ToolpathAction,
    ) -> iced::Task<Message> {
        if self.setup_draft_dirty()
            || self.inspector_dirty()
            || self.any_field_invalid()
            || !self.operation_height_overrides_valid()
            || self.controller.pending_op().is_some()
            || self.cad_pending_kind.is_some()
            || self.cad_editing_op.is_some()
            || self.drawing_editing_op.is_some()
        {
            self.status = "Apply or cancel inspector changes and geometry selection before processing toolpaths.".into();
            return iced::Task::none();
        }
        let (setup, operation) = match node {
            TreeNode::Setup(id) => (id, None),
            TreeNode::Operation(id) => {
                let Some(setup) = self
                    .controller
                    .setups()
                    .iter()
                    .find(|setup| setup.operations.iter().any(|op| op.id() == id))
                else {
                    self.status = "Operation no longer exists.".into();
                    return iced::Task::none();
                };
                (setup.id, Some(id))
            }
            _ => return iced::Task::none(),
        };
        if let Err(error) = self.controller.activate_setup(setup) {
            self.status = error;
            return iced::Task::none();
        }
        self.pending_toolpath_export = None;
        self.pending_simulation = None;
        self.simulation = None;
        self.tree_menu = None;
        self.open_op_menu = None;
        self.expanded_setups.insert(setup);
        self.model_view = None;
        self.focus_ops.clear();
        self.focus_ops.extend(operation);
        self.end_setup_draft();
        self.operation_draft = None;
        self.controller
            .select(operation.map_or(Selection::Setup, Selection::Operation));
        if let Some(id) = operation {
            self.begin_operation_draft(id);
        }
        self.refresh_fields();
        self.switch_workspace(Workspace::Machining);
        match self
            .controller
            .start_toolpath_run(operation, action == ToolpathAction::Recompute)
        {
            Ok(stamp) => {
                if action == ToolpathAction::Export {
                    self.pending_toolpath_export = Some(stamp.clone());
                }
                if action == ToolpathAction::Simulate {
                    self.pending_simulation = Some(stamp);
                    self.show_stock = true;
                    self.set_pane_visible(Pane::Output, true);
                }
                self.status = match action {
                    ToolpathAction::Recompute => "Recomputing toolpaths…",
                    ToolpathAction::Simulate => "Simulating toolpaths…",
                    ToolpathAction::Export => "Preparing toolpaths for post processing…",
                }
                .into();
            }
            Err(error) => self.status = error,
        }
        iced::Task::none()
    }
    pub(super) fn report_run(&mut self) {
        let Some(outcome) = self.controller.outcome() else {
            return;
        };
        let errors = outcome
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        self.status = match outcome.completion {
            JobCompletion::Complete => "Toolpath ready. Export when you like.".to_string(),
            JobCompletion::Failed => format!("{errors} error(s) — see Output."),
            JobCompletion::Cancelled => {
                "Run cancelled. Run the job again before exporting.".to_string()
            }
            JobCompletion::Incomplete => {
                "Run incomplete. Run the job again before exporting.".to_string()
            }
        };
    }
    /// The output dock: run diagnostics and the status line.
    pub(super) fn output(&self) -> Element<'_, Message> {
        let mut list = column![text(self.status.clone()).size(13)]
            .spacing(4)
            .padding(8);
        if self.controller.cad_import_pending() {
            list = list.push(
                row![
                    text("Importing geometry…").size(12),
                    inspector_button("Cancel import").on_press(Message::CancelCadImport)
                ]
                .spacing(8),
            );
        }
        if let Some(progress) = self.controller.run_progress() {
            let phase = match progress.phase {
                crate::JobPhase::Queued => "Waiting to run",
                crate::JobPhase::GeometryResolution => "Resolving geometry",
                crate::JobPhase::Generation => "Generating toolpaths",
                crate::JobPhase::Verification => "Simulating stock",
                crate::JobPhase::RenderPreparation => "Preparing preview",
            };
            list = list.push(
                row![
                    text(format!(
                        "{phase}: {} / {}",
                        progress.completed, progress.total
                    ))
                    .size(12),
                    inspector_button("Cancel run").on_press(Message::CancelRun)
                ]
                .spacing(8),
            );
        }
        if let Some(outcome) = self.controller.outcome() {
            if outcome.diagnostics.is_empty() {
                list = list.push(text("No diagnostics.").size(12));
            } else {
                list = list.push(text("Diagnostics:").size(13));
                for d in &outcome.diagnostics {
                    list = list.push(text(format!("• {:?}: {}", d.severity, d.message)).size(12));
                }
            }
            // Material-removal simulation results: collisions a backplot can't show.
            if outcome.completion != JobCompletion::Complete {
                list = list.push(text("Simulation incomplete.").size(12));
            } else if outcome.collisions.is_empty() {
                list = list.push(text("Simulation: no collisions.").size(12));
            } else {
                list =
                    list.push(text(format!("Collisions ({}):", outcome.collisions.len())).size(13));
                for c in &outcome.collisions {
                    list = list.push(text(format!("⚠ {:?}: {}", c.kind, c.message)).size(12));
                }
            }
        }
        scrollable(list)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Mutex};
    use std::time::{Duration, Instant};
    #[test]
    fn gui_processes_input_and_cancel_while_the_machining_worker_is_blocked() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller
            .add_operation(Operation::Drill(cam_model::DrillOp {
                id: 0,
                tool: 1,
                points: vec![[1., 1.]],
                depth: 1.,
                start_offset: 0.,
                peck: None,
                dwell: None,
                spindle_rpm: 1000.,
                work_offset: 1,
                feed: 100.,
            }));
        let (entered, receiver) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        app.controller.set_run_test_hook(Arc::new(move |token| {
            entered.send(token.clone()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }));
        app.rerun();
        let token = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(app.controller.outcome().is_none());
        let cursor = iced::Point::new(30., 40.);
        let _ = app.update(Message::WindowCursor(cursor));
        assert_eq!(app.window_cursor, cursor);
        let _ = app.update(Message::CancelRun);
        assert!(token.is_cancelled());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.controller.run_needs_poll() {
            let _ = app.update(Message::PollRun);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.controller.outcome().is_none());
        assert!(app.status.contains("cancelled"));
    }
}
