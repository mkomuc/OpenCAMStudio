//! Background job presentation and output view.
use super::*;
use iced::widget::column;
impl App {
    /// Submit a snapshot; the event loop never runs machining services inline.
    pub(super) fn rerun(&mut self) {
        if self.controller.document().setup().operations.is_empty() {
            self.controller.cancel_run();
            self.status = "No operations to run.".into();
            return;
        }
        self.controller.start_run();
        self.status = "Generating toolpaths…".into();
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
                    button("Cancel import").on_press(Message::CancelCadImport)
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
                    button("Cancel run").on_press(Message::CancelRun)
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
