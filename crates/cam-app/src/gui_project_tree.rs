//! Overlay browser, context commands and stable-ID drag/drop routing.
use super::*;
use iced::widget::column;

const TREE_ROW_H: f32 = 26.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TreeNode {
    Models,
    Setups,
    Model(InstanceId),
    Setup(u64),
    Operation(u32),
}

#[derive(Clone, Debug)]
pub(super) struct TreeDrag {
    node: TreeNode,
    start: iced::Point,
    dragging: bool,
}

#[derive(Clone, Debug)]
pub(super) enum TreeMessage {
    Press(TreeNode),
    Hover(TreeNode, bool),
    Leave(TreeNode),
    Open(TreeNode),
    Release,
    Command(Box<Message>),
    Close,
}

impl App {
    pub(super) fn cancel_tree_drag(&mut self) {
        self.tree_drag = None;
        self.tree_menu = None;
        self.tree_hover = None;
        self.open_op_menu = None;
    }

    pub(super) fn tree_pointer_moved(&mut self, point: iced::Point) {
        if let Some(drag) = self.tree_drag.as_mut() {
            if point.distance(drag.start) >= 5.0 {
                drag.dragging = true;
            }
        }
    }

    fn tree_edit_blocked(&mut self) -> bool {
        if self.controller.pending_op().is_some()
            || self.cad_pending_kind.is_some()
            || self.cad_editing_op.is_some()
            || self.drawing_editing_op.is_some()
            || self.setup_draft_dirty()
            || (matches!(self.controller.selection(), Selection::Operation(_))
                && self.inspector_dirty())
        {
            self.status = "Apply or cancel inspector changes before changing selection or restructuring the project.".into();
            true
        } else {
            false
        }
    }

    fn select_tree_node(&mut self, node: &TreeNode) -> iced::Task<Message> {
        match node {
            TreeNode::Setup(id) => self.update(Message::SelectSetup(*id)),
            TreeNode::Operation(id) => {
                if let Some(owner) = self
                    .controller
                    .setups()
                    .iter()
                    .find(|s| s.operations.iter().any(|op| op.id() == *id))
                    .map(|s| s.id)
                {
                    if owner != self.controller.active_setup_id() {
                        self.controller.activate_setup(owner).ok();
                        self.setup_draft = None;
                        self.expanded_setups.insert(owner);
                    }
                }
                self.update(Message::ClickOp(*id))
            }
            _ => {
                self.inspector_active = false;
                self.focus_ops.clear();
                iced::Task::none()
            }
        }
    }

    pub(super) fn update_tree(&mut self, message: TreeMessage) -> iced::Task<Message> {
        match message {
            TreeMessage::Hover(node, after) => self.tree_hover = Some((node, after)),
            TreeMessage::Leave(node) => {
                if self
                    .tree_hover
                    .as_ref()
                    .is_some_and(|(hover, _)| hover == &node)
                {
                    self.tree_hover = None;
                }
            }
            TreeMessage::Press(node) => {
                self.tree_menu = None;
                self.open_op_menu = None;
                if self.tree_edit_blocked() {
                    return iced::Task::none();
                }
                self.tree_drag = Some(TreeDrag {
                    node,
                    start: self.window_cursor,
                    dragging: false,
                });
            }
            TreeMessage::Open(node) => {
                self.tree_drag = None;
                if self.tree_edit_blocked() {
                    return iced::Task::none();
                }
                match &node {
                    TreeNode::Operation(id) => {
                        let owner = self
                            .controller
                            .setups()
                            .iter()
                            .find(|s| s.operations.iter().any(|op| op.id() == *id))
                            .map(|s| s.id);
                        if let Some(owner) = owner {
                            self.controller.activate_setup(owner).ok();
                            self.setup_draft = None;
                            self.expanded_setups.insert(owner);
                        }
                        let _ = self.update(Message::OpMenu(*id));
                    }
                    TreeNode::Setup(id) => {
                        let _ = self.update(Message::SelectSetup(*id));
                    }
                    TreeNode::Model(_) => {
                        let _ = self.select_tree_node(&node);
                    }
                    _ => {}
                }
                self.op_menu_pos = self.window_cursor;
                self.tree_menu = Some(node);
            }
            TreeMessage::Command(command) => {
                self.tree_menu = None;
                return self.update(*command);
            }
            TreeMessage::Close => {
                self.tree_menu = None;
                self.open_op_menu = None;
                return self.update(Message::CloseOpMenu);
            }
            TreeMessage::Release => {
                let Some(drag) = self.tree_drag.take() else {
                    return iced::Task::none();
                };
                if self.tree_edit_blocked() {
                    return iced::Task::none();
                }
                if !drag.dragging {
                    return self.select_tree_node(&drag.node);
                }
                let Some((target, after)) = self.tree_hover.take() else {
                    self.status = "Drop cancelled: release over a setup or operation row.".into();
                    return iced::Task::none();
                };
                let revision = self.controller.revision();
                let result = match (&drag.node, target) {
                    (TreeNode::Setup(id), TreeNode::Setup(target)) => {
                        let before = if after {
                            self.controller
                                .setups()
                                .iter()
                                .skip_while(|s| s.id != target)
                                .nth(1)
                                .map(|s| s.id)
                        } else {
                            Some(target)
                        };
                        self.controller.reorder_setup_to(*id, before)
                    }
                    (TreeNode::Operation(id), TreeNode::Setup(destination)) => {
                        self.controller.reorder_operation_to(*id, destination, None)
                    }
                    (TreeNode::Operation(id), TreeNode::Operation(anchor)) => {
                        let destination = self
                            .controller
                            .setups()
                            .iter()
                            .find(|s| s.operations.iter().any(|op| op.id() == anchor));
                        if let Some(destination) = destination {
                            let anchor_offset = destination
                                .operations
                                .iter()
                                .find(|op| op.id() == anchor)
                                .unwrap()
                                .work_offset();
                            let crosses_setup =
                                !destination.operations.iter().any(|op| op.id() == *id);
                            if crosses_setup && anchor_offset != destination.origin_index {
                                self.status = "Drop rejected: transfer onto the setup first, then use Move to Origin for another work-origin group.".into();
                                return iced::Task::none();
                            }
                            let before = if after {
                                destination
                                    .operations
                                    .iter()
                                    .skip_while(|op| op.id() != anchor)
                                    .skip(1)
                                    .find(|op| op.work_offset() == anchor_offset)
                                    .map(Operation::id)
                            } else {
                                Some(anchor)
                            };
                            self.controller
                                .reorder_operation_to(*id, destination.id, before)
                        } else {
                            Err("Drop target no longer exists".into())
                        }
                    }
                    _ => Err(
                        "Drop an operation on a setup or operation; drop a setup on another setup."
                            .into(),
                    ),
                };
                match result {
                    Ok(()) => {
                        if self.controller.revision() == revision {
                            self.status = "Execution order unchanged.".into();
                            return iced::Task::none();
                        }
                        self.setup_draft = None;
                        self.operation_draft = None;
                        if let Selection::Operation(id) = self.controller.selection() {
                            self.begin_operation_draft(id);
                        }
                        if self.controller.selection() == Selection::Setup {
                            self.begin_setup_draft();
                        }
                        self.refresh_fields();
                        self.rerun();
                        self.status =
                            "Execution order updated. Undo restores the previous order.".into();
                    }
                    Err(error) => self.status = format!("Drop rejected: {error}"),
                }
            }
        }
        iced::Task::none()
    }

    fn tree_target_active(&self, node: &TreeNode) -> bool {
        self.tree_drag.as_ref().is_some_and(|drag| drag.dragging)
            && self
                .tree_hover
                .as_ref()
                .is_some_and(|(hover, _)| hover == node)
    }

    fn tree_row<'a>(
        &self,
        node: TreeNode,
        content: Element<'a, Message>,
        selected: bool,
    ) -> Element<'a, Message> {
        let target = self.tree_target_active(&node);
        let leave = node.clone();
        let press = node.clone();
        let open = node.clone();
        mouse_area(
            container(content)
                .width(Length::Fill)
                .height(Length::Fixed(TREE_ROW_H))
                .padding(Padding::from([2.0, 4.0]))
                .style(move |_| container::Style {
                    background: selected.then_some(Background::Color(palette::SELECT_BG)),
                    border: Border {
                        color: if target {
                            palette::ACCENT_BLUE
                        } else {
                            Color::TRANSPARENT
                        },
                        width: if target { 1.0 } else { 0.0 },
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
        )
        .on_press(Message::Tree(TreeMessage::Press(press)))
        .on_right_press(Message::Tree(TreeMessage::Open(open)))
        .on_move(move |point| {
            Message::Tree(TreeMessage::Hover(
                node.clone(),
                point.y >= TREE_ROW_H / 2.0,
            ))
        })
        .on_exit(Message::Tree(TreeMessage::Leave(leave)))
        .into()
    }
    pub(super) fn project_tree(&self) -> Element<'_, Message> {
        // Workpiece origins live as group headers in the operation list below (H1
        // above the first op, added origins each heading their own group), not as a
        // node here — an origin *is* the datum a group of operations runs under.
        let mut list = column![mouse_area(tree_header("Models"))
            .on_right_press(Message::Tree(TreeMessage::Open(TreeNode::Models)))]
        .spacing(2);

        if !self.controller.geometry_state().instances.is_empty() {
            for instance in &self.controller.geometry_state().instances {
                if self
                    .controller
                    .geometry_state()
                    .asset(instance.asset_id)
                    .is_none()
                {
                    continue;
                }
                let hidden = !instance.visible || self.cad_hidden.contains(&instance.id);
                let model_row = row![
                    checkbox(!hidden).size(14).on_toggle(move |visible| {
                        Message::SetInstanceVisible(instance.id, visible)
                    }),
                    text(&instance.name).size(12).width(Length::Fill),
                ]
                .spacing(4)
                .align_y(Alignment::Center);
                list =
                    list.push(self.tree_row(TreeNode::Model(instance.id), model_row.into(), false));
            }
        }

        // Tools in use. A tool that matches the shop library shows its **shop number**
        // (`T4 …`); one that isn't in the library shows **no number** — the simple,
        // clear signal that it's project-local. Right-clicking an un-numbered tool
        // offers "Add to library" (which registers it and gives it a number).
        list = list.push(tree_header("Tools (in use)"));
        let used = self.controller.used_tools();
        for t in used {
            let in_library = self
                .library
                .tools
                .iter()
                .any(|lt| lt.identity() == t.identity());
            // Numbered ⇒ in the library; un-numbered ⇒ project-local. The number goes
            // at the *end* (`⌀6 End mill (T5)`) so descriptions align left; a
            // project-local tool has no `(Tn)` at all — the presence of the number *is*
            // the distinction (kept deliberately simple).
            let label = if in_library {
                format!("⌀{} {} (T{})", fmt_num(t.diameter), t.kind, t.number)
            } else {
                format!("⌀{} {}", fmt_num(t.diameter), t.kind)
            };
            let row = container(text(label).size(13).color(if in_library {
                palette::LABEL_COLOR
            } else {
                palette::GROUP_LABEL
            }))
            .padding(Padding::from([3.0, 6.0]));
            if in_library {
                list = list.push(row);
            } else {
                // Only un-numbered (not-in-library) tools carry the right-click menu.
                list = list.push(mouse_area(row).on_right_press(Message::ToolMenu(t.number)));
            }
        }

        list = list.push(
            mouse_area(tree_header("Setups"))
                .on_right_press(Message::Tree(TreeMessage::Open(TreeNode::Setups))),
        );
        for setup in self.controller.setups() {
            let active = setup.id == self.controller.active_setup_id();
            let expanded = self.expanded_setups.contains(&setup.id);
            let status = if !active {
                "inactive"
            } else if setup.operations.is_empty() {
                "empty"
            } else if self.controller.outcome().is_some_and(|outcome| {
                setup
                    .operations
                    .iter()
                    .filter(|op| !self.controller.is_operation_excluded(op.id()))
                    .all(|op| {
                        outcome
                            .scene
                            .strips
                            .iter()
                            .any(|strip| strip.op == Some(op.id()))
                    })
            }) {
                "generated"
            } else if self.controller.outcome().is_some() {
                "needs run"
            } else {
                "pending"
            };
            let setup_row = row![
                button(text(if expanded { "▾" } else { "▸" }).size(11))
                    .padding(2)
                    .on_press(Message::ToggleSetupExpanded(setup.id)),
                checkbox(!self.hidden_setups.contains(&setup.id))
                    .size(12)
                    .on_toggle(move |visible| Message::SetSetupVisible(setup.id, visible)),
                text(format!("{} · {}", setup.name, status))
                    .size(11)
                    .width(Length::Fill)
            ]
            .spacing(3)
            .align_y(Alignment::Center);
            list = list.push(self.tree_row(TreeNode::Setup(setup.id), setup_row.into(), active));
            if !expanded {
                continue;
            }
            // The planner executes contiguous WCS groups in origin order. Display
            // that same stable order so a row's position is its execution order.
            let origins = setup.origin_indices();
            let mut operations: Vec<_> = setup.operations.iter().collect();
            operations.sort_by_key(|op| {
                origins
                    .iter()
                    .position(|origin| *origin == op.work_offset())
                    .unwrap_or(0)
            });
            let mut group = None;
            for op in operations {
                if origins.len() > 1 && group != Some(op.work_offset()) {
                    list = list.push(tree_note(&format!("    Work origin {}", op.work_offset())));
                    group = Some(op.work_offset());
                }
                let id = op.id();
                let excluded = self.controller.is_operation_excluded(id);
                let generated = active
                    && self.controller.outcome().is_some_and(|outcome| {
                        outcome
                            .scene
                            .strips
                            .iter()
                            .any(|strip| strip.op == Some(id))
                    });
                let state = if excluded {
                    "suppressed"
                } else if !active {
                    "inactive"
                } else if generated {
                    "ready"
                } else {
                    "pending"
                };
                let tools = op
                    .tools()
                    .iter()
                    .map(|n| format!("T{n}"))
                    .collect::<Vec<_>>()
                    .join("/");
                let controls = row![
                    Space::new().width(Length::Fixed(14.0)),
                    checkbox(!self.hidden_ops.contains(&id))
                        .size(12)
                        .on_toggle(move |visible| Message::SetOperationVisible(id, visible)),
                    text(format!("{} · {} · {}", op_kind(op), tools, state))
                        .size(11)
                        .width(Length::Fill)
                ]
                .spacing(3)
                .align_y(Alignment::Center);
                list = list.push(self.tree_row(
                    TreeNode::Operation(id),
                    controls.into(),
                    self.focus_ops.contains(&id),
                ));
            }
        }
        scrollable(list.padding(6))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(super) fn tree_menu_overlay(&self) -> Option<Element<'_, Message>> {
        let node = self.tree_menu.as_ref()?;
        let item = |label: String, message: Message| {
            let icon = match &message {
                Message::DuplicateOp | Message::SetupDuplicate(_) => Some(Icon::Duplicate),
                Message::ToggleSetOrigin => Some(Icon::SetOrigin),
                _ => None,
            };
            let mut label_row = row![].spacing(6);
            if let Some(icon) = icon {
                label_row = label_row.push(icon_svg(icon, 14.));
            }
            label_row = label_row.push(text(label).size(13));
            button(label_row)
                .width(Length::Fixed(236.0))
                .padding(Padding::from([5.0, 8.0]))
                .on_press(Message::Tree(TreeMessage::Command(Box::new(message))))
                .style(|_, status| command_button_style(status))
        };
        let mut items = column![].spacing(2);
        let creation = |items: iced::widget::Column<'static, Message>| {
            let mut items = items;
            for (kind, label) in [
                (OpKind::Profile, "Profile"),
                (OpKind::Pocket, "Pocket"),
                (OpKind::Drill, "Drill"),
                (OpKind::Face, "Face"),
                (OpKind::Chamfer, "Chamfer"),
                (OpKind::Thread, "Thread"),
                (OpKind::Engrave, "Engrave"),
                (OpKind::Carve, "Carve"),
            ] {
                items = items.push(item(
                    format!("New {label} operation"),
                    Message::BeginOp(kind),
                ));
            }
            items
        };
        match node {
            TreeNode::Setups => {
                items = items.push(item("New setup".into(), Message::SetupNew));
            }
            TreeNode::Setup(id) => {
                items = items
                    .push(item("Edit setup".into(), Message::SelectSetup(*id)))
                    .push(item("New setup".into(), Message::SetupNew))
                    .push(item("Duplicate setup".into(), Message::SetupDuplicate(*id)))
                    .push(item(
                        "Move setup earlier".into(),
                        Message::SetupMove(*id, true),
                    ))
                    .push(item(
                        "Move setup later".into(),
                        Message::SetupMove(*id, false),
                    ))
                    .push(item("Pick datum point".into(), Message::ToggleSetOrigin));
                items = creation(items);
                if self.controller.setups().len() > 1 {
                    items = items.push(item("Delete setup".into(), Message::SetupDelete(*id)));
                }
            }
            TreeNode::Operation(id) => {
                items = items
                    .push(item(
                        "Edit operation".into(),
                        Message::Select(Selection::Operation(*id)),
                    ))
                    .push(item(
                        "Edit geometry".into(),
                        if self.controller.geometry_state().bindings.contains_key(id) {
                            Message::EditCadGeometry(*id)
                        } else {
                            Message::EditDrawingGeometry(*id)
                        },
                    ))
                    .push(item("Duplicate".into(), Message::DuplicateOp))
                    .push(item("Reinitialize".into(), Message::ReinitOp))
                    .push(item("Move earlier".into(), Message::MoveOp(*id, true)))
                    .push(item("Move later".into(), Message::MoveOp(*id, false)))
                    .push(item(
                        if self.controller.is_operation_excluded(*id) {
                            "Include in machining"
                        } else {
                            "Suppress machining"
                        }
                        .into(),
                        Message::SetOpExcluded(*id, !self.controller.is_operation_excluded(*id)),
                    ));
                items = creation(items);
                for origin in self.origin_move_targets_for(*id) {
                    items = items.push(item(
                        format!("Move to {}", self.origin_menu_label(origin)),
                        Message::MoveOpToOrigin(origin),
                    ));
                }
                for setup in self
                    .controller
                    .setups()
                    .iter()
                    .filter(|s| !s.operations.iter().any(|op| op.id() == *id))
                {
                    items = items
                        .push(item(
                            format!("Move to setup {}", setup.name),
                            Message::TransferOpToSetup(setup.id, false),
                        ))
                        .push(item(
                            format!("Copy to setup {}", setup.name),
                            Message::TransferOpToSetup(setup.id, true),
                        ));
                }
                items = items.push(item("Delete operation".into(), Message::DeleteOp));
            }
            TreeNode::Models => {
                items = items
                    .push(item("Import geometry…".into(), Message::ImportCad))
                    .push(item("Show all geometry".into(), Message::CadShowAll));
            }
            TreeNode::Model(id) => {
                let hidden = self.cad_hidden.contains(id);
                items = items
                    .push(item(
                        if hidden { "Show model" } else { "Hide model" }.into(),
                        Message::SetInstanceVisible(*id, hidden),
                    ))
                    .push(item(
                        "Isolate model".into(),
                        Message::CadIsolateInstance(*id),
                    ))
                    .push(item("Show all geometry".into(), Message::CadShowAll));
                if let Some(instance) = self.controller.geometry_state().instance(*id) {
                    items = items
                        .push(item(
                            "Replace model…".into(),
                            Message::ReplaceCadAsset(instance.asset_id),
                        ))
                        .push(item(
                            "Remove model".into(),
                            Message::RemoveCadAsset(instance.asset_id),
                        ));
                }
                items = creation(items);
            }
        }
        let menu = container(scrollable(items).height(Length::Shrink))
            .max_height((self.window.height - self.op_menu_pos.y - 12.0).max(100.0))
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(palette::RIBBON_BG)),
                border: Border {
                    color: palette::BORDER_DARK,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..container::Style::default()
            });
        let position = iced::Point::new(
            self.op_menu_pos.x.min((self.window.width - 250.0).max(0.0)),
            self.op_menu_pos
                .y
                .min((self.window.height - 110.0).max(0.0)),
        );
        Some(
            container(menu)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .align_y(iced::alignment::Vertical::Top)
                .padding(Padding {
                    top: position.y,
                    left: position.x,
                    right: 0.0,
                    bottom: 0.0,
                })
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(app: &mut App, message: Message) {
        let _task = app.update(message);
    }

    fn tree_app() -> (App, Vec<u32>) {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.workspace = Workspace::Machining;
        app.active_tab = RibbonTab::Home;
        app.operation_draft = None;
        app.setup_draft = None;
        app.fields.clear();
        for _ in 0..3 {
            app.controller
                .add_operation(Operation::Drill(cam_model::DrillOp {
                    id: 0,
                    tool: 1,
                    points: vec![[10., 10.]],
                    depth: 2.,
                    start_offset: 0.,
                    peck: None,
                    dwell: None,
                    spindle_rpm: 1000.,
                    work_offset: 1,
                    feed: 100.,
                }));
        }
        app.controller.select(Selection::Setup);
        let ids = app
            .controller
            .document()
            .setup()
            .operations
            .iter()
            .map(Operation::id)
            .collect();
        app.begin_setup_draft();
        (app, ids)
    }

    #[test]
    fn global_pointer_release_publishes_one_undoable_tree_order() {
        let (mut app, ids) = tree_app();
        let before = app.controller.document().clone();
        send(&mut app, Message::WindowCursor(iced::Point::new(10., 10.)));
        send(
            &mut app,
            Message::Tree(TreeMessage::Press(TreeNode::Operation(ids[2]))),
        );
        send(&mut app, Message::WindowCursor(iced::Point::new(10., 40.)));
        send(
            &mut app,
            Message::Tree(TreeMessage::Hover(TreeNode::Operation(ids[0]), false)),
        );
        send(&mut app, Message::Tree(TreeMessage::Release));
        assert_eq!(
            app.controller
                .document()
                .setup()
                .operations
                .iter()
                .map(Operation::id)
                .collect::<Vec<_>>(),
            [ids[2], ids[0], ids[1]]
        );
        assert!(app.tree_drag.is_none());
        // A widget release followed by the global release cannot apply twice.
        send(&mut app, Message::Tree(TreeMessage::Release));
        app.controller.cancel_run();
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn click_threshold_and_release_outside_do_not_restructure_document() {
        let (mut app, ids) = tree_app();
        let before = app.controller.document().clone();
        send(&mut app, Message::WindowCursor(iced::Point::new(10., 10.)));
        send(
            &mut app,
            Message::Tree(TreeMessage::Press(TreeNode::Operation(ids[1]))),
        );
        send(&mut app, Message::WindowCursor(iced::Point::new(13., 10.)));
        send(&mut app, Message::Tree(TreeMessage::Release));
        assert_eq!(app.controller.selection(), Selection::Operation(ids[1]));
        assert_eq!(app.controller.document(), &before);
        send(
            &mut app,
            Message::Tree(TreeMessage::Press(TreeNode::Operation(ids[1]))),
        );
        send(&mut app, Message::WindowCursor(iced::Point::new(40., 40.)));
        send(
            &mut app,
            Message::Tree(TreeMessage::Hover(TreeNode::Operation(ids[0]), false)),
        );
        send(
            &mut app,
            Message::Tree(TreeMessage::Leave(TreeNode::Operation(ids[0]))),
        );
        send(&mut app, Message::Tree(TreeMessage::Release));
        assert_eq!(app.controller.document(), &before);
        assert!(app.status.contains("cancelled"));
    }

    #[test]
    fn numeric_inspector_drafts_block_tree_selection_and_context_restructure() {
        let (mut app, ids) = tree_app();
        app.controller.select(Selection::Operation(ids[0]));
        app.begin_operation_draft(ids[0]);
        send(&mut app, Message::FieldChanged(Field::Feed, "137".into()));
        assert!(!app.operation_draft_dirty());
        assert!(app.inspector_dirty());
        let before = app.controller.document().clone();
        send(
            &mut app,
            Message::Tree(TreeMessage::Press(TreeNode::Operation(ids[1]))),
        );
        assert!(app.tree_drag.is_none());
        send(
            &mut app,
            Message::Tree(TreeMessage::Open(TreeNode::Operation(ids[1]))),
        );
        assert!(app.tree_menu.is_none());
        assert_eq!(app.controller.selection(), Selection::Operation(ids[0]));
        assert_eq!(
            app.fields.get(&Field::Feed).map(String::as_str),
            Some("137")
        );
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn operation_visibility_and_machining_suppression_are_independent() {
        let (mut app, ids) = tree_app();
        let before = app.controller.document().clone();
        send(&mut app, Message::SetOperationVisible(ids[0], false));
        assert!(app.hidden_ops.contains(&ids[0]));
        assert!(!app.controller.is_operation_excluded(ids[0]));
        assert_eq!(app.controller.document(), &before);
        send(
            &mut app,
            Message::Tree(TreeMessage::Open(TreeNode::Operation(ids[0]))),
        );
        assert!(app.tree_menu_overlay().is_some());
        send(
            &mut app,
            Message::Tree(TreeMessage::Command(Box::new(Message::SetOpExcluded(
                ids[0], true,
            )))),
        );
        app.controller.cancel_run();
        assert!(app.controller.is_operation_excluded(ids[0]));
        assert!(app.hidden_ops.contains(&ids[0]));
        assert!(app.controller.undo());
        assert!(!app.controller.is_operation_excluded(ids[0]));
        assert!(app.hidden_ops.contains(&ids[0]));
    }

    #[test]
    fn context_operation_in_inactive_setup_targets_its_owner() {
        let (mut app, ids) = tree_app();
        let owner = app.controller.active_setup_id();
        let other = app.controller.add_setup();
        app.operation_draft = None;
        app.begin_setup_draft();
        send(
            &mut app,
            Message::Tree(TreeMessage::Open(TreeNode::Operation(ids[1]))),
        );
        assert_ne!(owner, other);
        assert_eq!(app.controller.active_setup_id(), owner);
        assert_eq!(app.controller.selection(), Selection::Operation(ids[1]));
        assert_eq!(app.open_op_menu, Some(ids[1]));
        assert!(app.tree_menu_overlay().is_some());
    }
}
