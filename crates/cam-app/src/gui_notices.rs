//! Import completion and serialized warning/error dialogs.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Notice {
    pub title: String,
    pub detail: String,
    pub error: bool,
}

impl Notice {
    fn import_error(detail: String) -> Self {
        Self {
            title: "Cannot import geometry".into(),
            detail,
            error: true,
        }
    }
}

pub(super) fn job_notice(outcome: &crate::RunOutcome) -> Option<Notice> {
    let mut lines: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity != Severity::Info)
        .map(|diagnostic| format!("{:?}: {}", diagnostic.severity, diagnostic.message))
        .collect();
    lines.extend(
        outcome
            .collisions
            .iter()
            .map(|collision| format!("Verification: {}", collision.message)),
    );
    let error = outcome
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
        || !outcome.collisions.is_empty()
        || matches!(
            outcome.completion,
            JobCompletion::Failed | JobCompletion::Incomplete
        );
    if error && lines.is_empty() {
        lines.push("The toolpath or verification did not complete. Export is unavailable.".into());
    }
    (!lines.is_empty()).then(|| Notice {
        title: if error {
            "Toolpath / verification errors"
        } else {
            "Toolpath warnings"
        }
        .into(),
        detail: lines.join("\n\n"),
        error,
    })
}

impl App {
    pub(super) fn queue_notice(&mut self, notice: Notice) -> iced::Task<Message> {
        self.notice_queue.push_back(notice);
        self.show_next_notice()
    }

    pub(super) fn show_next_notice(&mut self) -> iced::Task<Message> {
        if self.notice.is_some() {
            return iced::Task::none();
        }
        let Some(notice) = self.notice_queue.pop_front() else {
            return iced::Task::none();
        };
        self.cancel_tree_drag();
        self.notice = Some(notice);
        iced::Task::none()
    }

    pub(super) fn notice_overlay(&self) -> Option<Element<'_, Message>> {
        let notice = self.notice.as_ref()?;
        let card = container(
            iced::widget::column![
                text(&notice.title).size(18).color(if notice.error {
                    Color::from_rgb(0.95, 0.48, 0.40)
                } else {
                    palette::WARN
                }),
                container(scrollable(text(&notice.detail).size(13)))
                    .max_height((self.window.height * 0.45).clamp(100., 340.)),
                row![
                    Space::new().width(Length::Fill),
                    button("OK").on_press(Message::NoticeDismissed)
                ]
                .spacing(8)
            ]
            .spacing(16),
        )
        .max_width(560)
        .padding(20)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color,
            )),
            border: Border {
                color: palette::SEPARATOR,
                width: 1.,
                radius: 6.0.into(),
            },
            ..Default::default()
        });
        Some(
            mouse_area(
                container(card)
                    .padding(24)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0., 0., 0., 0.55))),
                        ..Default::default()
                    }),
            )
            .on_press(Message::NoticeBackdrop)
            .on_right_press(Message::NoticeBackdrop)
            .interaction(iced::mouse::Interaction::Idle)
            .into(),
        )
    }

    pub(super) fn complete_import(&mut self, result: Result<usize, String>) -> iced::Task<Message> {
        let preview = self.controller.cad_import_preview();
        let replacement = self.controller.cad_import_replacement();
        let linked_replacement = replacement.is_some_and(|asset| {
            self.controller
                .geometry_state()
                .bindings
                .values()
                .any(|binding| {
                    binding
                        .selected
                        .iter()
                        .any(|source| source.asset_id == asset)
                })
        });
        let result = result.and_then(|_| {
            if self.setup_draft_dirty() || self.inspector_dirty() || self.any_field_invalid() || self.creating_operation() || self.cad_editing_op.is_some() || self.drawing_editing_op.is_some() {
                self.controller.cancel_cad_import();
                return Err("An editor or geometry selection changed during import. Finish or Cancel that task, then import again.".into());
            }
            self.controller.commit_cad_preview()
        });
        match result {
            Err(error) => {
                self.controller.cancel_cad_import();
                self.status = format!("Import failed: {error}");
                self.queue_notice(Notice::import_error(error))
            }
            Ok(count) => {
                self.cad_hidden.clear();
                self.hidden_assets.clear();
                self.model_view = (!self.controller.geometry_state().instances.is_empty())
                    .then_some(TreeNode::Models);
                self.cad_hover = None;
                self.cad_candidates.clear();
                self.cad_pending_kind = None;
                self.cad_editing_op = None;
                self.cad_inspecting = false;
                self.planar_selection.clear();
                self.focus_ops.clear();
                self.operation_draft = None;
                self.operation_geometry = None;
                self.operation_binding_preview = None;
                self.operation_tools.clear();
                self.view = ViewControls::default();
                self.controller.select(Selection::Setup);
                self.end_setup_draft();
                self.refresh_fields();
                self.status = format!("Imported {count} geometry item(s).");
                let mut warnings = preview
                    .map(|preview| preview.diagnostics)
                    .unwrap_or_default();
                if linked_replacement {
                    warnings.push("The model was replaced. Dependent operations require geometry reselection before generation or export.".into());
                }
                if warnings.is_empty() {
                    iced::Task::none()
                } else {
                    self.queue_notice(Notice {
                        title: "Geometry import warnings".into(),
                        detail: warnings.join("\n\n"),
                        error: false,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.setup_draft = None;
        app.refresh_fields();
        app
    }

    fn poll_import(app: &mut App, path: std::path::PathBuf) {
        let _ = app.update(Message::CadToImport(Some(path)));
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.controller.cad_import_pending() {
            let _ = app.update(Message::PollCadImport);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn dxf_completion_installs_geometry_without_an_add_to_project_step() {
        let mut app = app();
        let before = app.controller.document().clone();
        let path = std::env::temp_dir().join(format!("immediate-dxf-{}.dxf", std::process::id()));
        std::fs::write(&path, SAMPLE_DXF).unwrap();
        poll_import(&mut app, path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(!app.controller.regions().is_empty());
        assert!(app.controller.cad_import_preview().is_none());
        assert_eq!(app.controller.selection(), Selection::Setup);
        assert!(!app.setup_draft_dirty());
        assert!(app.controller.document().setup().operations.is_empty());
        assert!(app.status.starts_with("Imported "));
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn import_failure_keeps_the_project_and_requests_an_error_dialog() {
        let mut app = app();
        let before = app.controller.document().clone();
        let path = std::env::temp_dir().join(format!("missing-import-{}.step", std::process::id()));
        poll_import(&mut app, path);
        assert_eq!(app.controller.document(), &before);
        assert!(app
            .notice
            .as_ref()
            .is_some_and(|notice| notice.error && notice.title == "Cannot import geometry"));
        assert!(app.notice_queue.is_empty());
    }

    #[test]
    fn import_warning_uses_geometry_immediately_and_requests_a_warning_dialog() {
        let mut app = app();
        let path = std::env::temp_dir().join(format!("warning-import-{}.dxf", std::process::id()));
        let drawing = "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n0\n20\n0\n40\n5\n0\nTEXT\n10\n0\n20\n0\n40\n2.5\n1\nannotation\n0\nENDSEC\n0\nEOF\n";
        std::fs::write(&path, drawing).unwrap();
        poll_import(&mut app, path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(
            !app.controller.regions().is_empty(),
            "warnings must not require an import confirmation"
        );
        let notice = app.notice.as_ref().expect("warning dialog");
        assert!(!notice.error);
        assert!(
            notice.detail.contains("unsupported entity"),
            "{}",
            notice.detail
        );
        assert!(app.controller.cad_import_preview().is_none());
    }

    #[cfg(feature = "occt")]
    #[test]
    fn step_completion_installs_native_geometry_directly() {
        let mut app = app();
        let before = app.controller.document().clone();
        poll_import(
            &mut app,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../cam-kernel/fixtures/box-mm.step"),
        );
        assert!(!app.controller.geometry_state().instances.is_empty());
        assert!(app.controller.cad_import_preview().is_none());
        assert!(app.status.starts_with("Imported "));
        assert!(app.notice.is_none(), "a clean STEP import needs no warning dialog");
        assert!(app.notice_queue.is_empty());
        assert!(!app.setup_draft_dirty());
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn run_errors_are_reported_once_and_not_on_every_poll() {
        let mut app = app();
        app.controller
            .add_operation(Operation::Drill(cam_model::DrillOp {
                id: 0,
                tool: 999,
                points: vec![[0., 0.]],
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
            }));
        app.rerun();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.controller.run_needs_poll() {
            let _ = app.update(Message::PollRun);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.notice.as_ref().is_some_and(|notice| notice.error));
        for _ in 0..3 {
            let _ = app.update(Message::PollRun);
        }
        assert!(app.notice_queue.is_empty());
    }

    #[test]
    fn warnings_and_verification_collisions_are_serialized_as_dialogs() {
        let mut app = app();
        let mut outcome = crate::RunOutcome {
            completion: JobCompletion::Complete,
            ..Default::default()
        };
        outcome
            .diagnostics
            .push(cam_toolpath::Diagnostic::info("ordinary information"));
        assert!(job_notice(&outcome).is_none());
        outcome
            .diagnostics
            .push(cam_toolpath::Diagnostic::warning("strategy warning"));
        let _ = app.queue_notice(job_notice(&outcome).unwrap());
        assert!(!app.notice.as_ref().unwrap().error);
        outcome.collisions.push(cam_sim::Collision {
            kind: cam_sim::CollisionKind::RapidThroughStock,
            at: [0.; 3],
            message: "rapid through stock".into(),
            op_id: None,
            pattern_id: None,
            instance_index: None,
            step_index: None,
            fraction: 0.,
        });
        let _ = app.queue_notice(job_notice(&outcome).unwrap());
        assert_eq!(app.notice_queue.len(), 1);
        let _ = app.update(Message::NoticeDismissed);
        let notice = app.notice.as_ref().unwrap();
        assert!(notice.error && notice.detail.contains("rapid through stock"));
        assert!(app.notice_queue.is_empty());
    }
    #[test]
    fn warning_dialog_blocks_project_actions_until_acknowledged() {
        let mut app = app();
        app.controller.new_operation(OpKind::Drill);
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        let before = app.controller.document().clone();
        let _ = app.queue_notice(Notice {
            title: "Warning".into(),
            detail: "Test warning".into(),
            error: false,
        });
        let _ = app.update(Message::NewProject);
        let _ = app.update(Message::NoticeBackdrop);
        assert_eq!(app.controller.document(), &before);
        assert!(app.notice.is_some());
        let _ = app.update(Message::NoticeDismissed);
        assert!(app.notice.is_none());
    }
}
