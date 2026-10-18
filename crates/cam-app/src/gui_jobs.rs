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
            || self.pattern_draft.is_some()
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
        let (setup, operation, pattern) = match node {
            TreeNode::Setup(id) => (id, None, None),
            TreeNode::Pattern(id) => {
                let Some(setup) = self.controller.setups().iter().find(|s| s.patterns.iter().any(|p| p.id == id)) else {
                    self.status = "Pattern no longer exists.".into();
                    return iced::Task::none();
                };
                (setup.id, None, Some(id))
            }
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
                (setup.id, Some(id), None)
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
        if let Some(pattern_id) = pattern {
            if let Some(pattern) = self.controller.setups().iter().find_map(|s| s.patterns.iter().find(|p| p.id == pattern_id)) { self.focus_ops.extend(pattern.members.iter().copied()); }
        }
        self.end_setup_draft();
        self.operation_draft = None;
        self.controller
            .select(operation.map_or(Selection::Setup, Selection::Operation));
        self.refresh_fields();
        self.switch_workspace(Workspace::Machining);
        let run = if let Some(pattern_id) = pattern {
            self.controller.start_pattern_toolpath_run(pattern_id, action == ToolpathAction::Recompute)
        } else {
            self.controller.start_toolpath_run(operation, action == ToolpathAction::Recompute)
        };
        match run {
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
    /// A read-only editor provides native drag selection, Select All and Copy.
    pub(super) fn output(&self) -> Element<'_, Message> {
        let mut controls = row![
            inspector_button(if self.output_content.selection().is_some() {
                "Copy selection"
            } else {
                "Copy output"
            })
            .on_press(Message::OutputCopy)
        ]
        .spacing(8);
        if self.output_content.selection().is_some() {
            controls = controls.push(text("Text updates pause while selected").size(11));
        }
        if self.controller.cad_import_pending() {
            controls =
                controls.push(inspector_button("Cancel import").on_press(Message::CancelCadImport));
        }
        if self.controller.run_progress().is_some() {
            controls = controls.push(inspector_button("Cancel run").on_press(Message::CancelRun));
        }
        column![
            controls,
            iced::widget::text_editor(&self.output_content)
                .on_action(Message::OutputAction)
                .font(iced::Font::MONOSPACE)
                .size(12)
                .height(Length::Fill)
        ]
        .spacing(4)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
