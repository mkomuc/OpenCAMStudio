//! Confirmation before replacing a project or closing its window.
use super::*;

#[derive(Clone, Debug)]
pub(super) enum DiscardAction {
    New,
    Sample,
    Open(PathBuf),
    Close(iced::window::Id),
}

impl DiscardAction {
    pub(super) fn message(self) -> Message {
        match self {
            Self::New => Message::NewProject,
            Self::Sample => Message::OpenSample,
            Self::Open(path) => Message::ProjectToOpen(Some(path)),
            Self::Close(id) => Message::CloseRequested(id),
        }
    }
}

impl App {
    pub(super) fn has_changes_to_discard(&self) -> bool {
        self.controller.has_unsaved_changes()
            || self.project_view_changed()
            || self.setup_draft_dirty()
            || self.inspector_dirty()
            || self.any_field_invalid()
            || self.pattern_draft.is_some()
            || self.scale_draft.is_some()
            || self.creating_operation()
            || self.cad_editing_op.is_some()
            || self.drawing_editing_op.is_some()
            || self.profile_entry_editor.is_some()
            || self.geometry_editor.is_some()
            || !self.tool_drafts.is_empty()
    }

    pub(super) fn unsaved_overlay(&self) -> Option<Element<'_, Message>> {
        let action = self.pending_discard.as_ref()?;
        let verb = match action {
            DiscardAction::New => "creating a new project",
            DiscardAction::Sample => "opening the sample",
            DiscardAction::Open(_) => "opening another project",
            DiscardAction::Close(_) => "closing OpenCAMStudio",
        };
        let card = container(
            iced::widget::column![
                inspector_heading("Discard unsaved changes?"),
                iced::widget::column![
                    text(format!("You have unsaved changes that will be lost when {verb}."))
                        .size(INSPECTOR_TEXT_SIZE)
                        .color(INSPECTOR_TEXT),
                    text("Cancel to keep working and save your changes, or discard them to continue.")
                        .size(INSPECTOR_TEXT_SIZE)
                        .color(INSPECTOR_MUTED),
                ]
                .spacing(6),
                row![
                    Space::new().width(Length::Fill),
                    inspector_button("Cancel").on_press(Message::DiscardCancelled),
                    inspector_button("Discard changes")
                        .style(|theme, status| {
                            let mut style = inspector_secondary(theme, status);
                            style.border.color = palette::ERROR;
                            style.text_color = Color::from_rgb(0.97, 0.57, 0.53);
                            style
                        })
                        .on_press(Message::DiscardConfirmed),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .max_width(420)
        .padding(16)
        .style(popup_surface);
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
            .on_press(Message::DiscardCancelled)
            .on_right_press(Message::DiscardCancelled)
            .interaction(iced::mouse::Interaction::Idle)
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.notice = None;
        app.notice_queue.clear();
        app.refresh_fields();
        app
    }

    fn send(app: &mut App, message: Message) {
        let _ = app.update(message);
    }

    #[test]
    fn new_project_requires_confirmation_and_cancel_keeps_changes() {
        let mut app = app();
        app.controller
            .edit_setup(|setup| setup.name = "Keep me".into());
        send(&mut app, Message::NewProject);
        assert!(matches!(app.pending_discard, Some(DiscardAction::New)));
        assert_eq!(app.controller.inspection_setup().name, "Keep me");
        assert!(app.unsaved_overlay().is_some());
        // Background clicks and repeated destructive actions cannot bypass the modal.
        send(&mut app, Message::OpenSample);
        assert!(matches!(app.pending_discard, Some(DiscardAction::New)));
        send(&mut app, Message::CadEscape);
        assert!(app.pending_discard.is_none());
        assert!(app.controller.has_unsaved_changes());
        send(&mut app, Message::NewProject);
        send(&mut app, Message::DiscardConfirmed);
        assert!(app.pending_discard.is_none());
        assert!(!app.controller.has_unsaved_changes());
        assert_ne!(app.controller.inspection_setup().name, "Keep me");
    }

    #[test]
    fn close_warns_for_uncommitted_editor_changes_in_a_clean_project() {
        let mut app = app();
        app.controller.add_setup();
        let path =
            std::env::temp_dir().join(format!("clean-setup-editor-{}.ocam", std::process::id()));
        app.controller.save_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let id = iced::window::Id::unique();
        assert!(!app.has_changes_to_discard());
        app.begin_setup_draft();
        send(&mut app, Message::SetupNameChanged("Draft".into()));
        assert!(!app.controller.has_unsaved_changes());
        assert!(app.setup_draft_dirty());
        send(&mut app, Message::CloseRequested(id));
        assert!(matches!(app.pending_discard, Some(DiscardAction::Close(window)) if window == id));
        send(&mut app, Message::DiscardCancelled);
        assert_eq!(app.setup_draft.as_ref().unwrap().name, "Draft");
        send(&mut app, Message::CloseRequested(id));
        send(&mut app, Message::DiscardConfirmed);
        assert!(app.pending_discard.is_none());
    }

    #[test]
    fn open_confirmation_keeps_the_selected_path_and_failed_open_keeps_changes() {
        let mut app = app();
        app.controller
            .edit_setup(|setup| setup.name = "Keep me".into());
        let path =
            std::env::temp_dir().join(format!("missing-unsaved-{}.ocam", std::process::id()));
        send(&mut app, Message::ProjectToOpen(Some(path.clone())));
        assert!(
            matches!(&app.pending_discard, Some(DiscardAction::Open(selected)) if selected == &path)
        );
        send(&mut app, Message::DiscardConfirmed);
        assert_eq!(app.controller.inspection_setup().name, "Keep me");
        assert!(app.controller.has_unsaved_changes());
        assert!(app.status.starts_with("Open failed:"));
    }

    #[test]
    fn sample_replacement_warns_and_cancelled_open_does_not() {
        let mut app = app();
        app.controller
            .edit_setup(|setup| setup.name = "Keep me".into());
        send(&mut app, Message::ProjectToOpen(None));
        assert!(app.pending_discard.is_none());
        send(&mut app, Message::OpenSample);
        assert!(matches!(app.pending_discard, Some(DiscardAction::Sample)));
        send(&mut app, Message::DiscardCancelled);
        assert_eq!(app.controller.inspection_setup().name, "Keep me");
        send(&mut app, Message::OpenSample);
        send(&mut app, Message::DiscardConfirmed);
        assert!(app.controller.has_geometry());
        assert!(app.controller.has_unsaved_changes());
    }
}
