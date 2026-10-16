//! Operation geometry collections shared by creation, replacement and editing.
use super::*;
use crate::LoopPart;
use iced::widget::column;

pub(super) fn cad_geometry_labels(selected: &[EntityRef]) -> Vec<String> {
    selected
        .iter()
        .map(|entity| {
            if entity.kind == EntityKind::Silhouette {
                format!("XY silhouette · Body {}", entity.body_id)
            } else {
                format!("{:?} {} · Body {}", entity.kind, entity.id, entity.body_id)
            }
        })
        .collect()
}

pub(super) fn drawing_geometry_labels(selected: &[LoopRef]) -> Vec<String> {
    selected
        .iter()
        .map(|reference| match reference.part {
            LoopPart::Outer => format!("Boundary {}", reference.region + 1),
            LoopPart::Hole(index) => {
                format!("Hole {} · Boundary {}", index + 1, reference.region + 1)
            }
            LoopPart::Open => format!("Path {}", reference.region + 1),
        })
        .collect()
}

pub(super) fn operation_geometry_labels(op: &Operation) -> Vec<String> {
    match op {
        Operation::Drill(op) => (0..op.points.len())
            .map(|i| format!("Hole {}", i + 1))
            .collect(),
        Operation::Thread(op) => (0..op.points.len())
            .map(|i| format!("Thread centre {}", i + 1))
            .collect(),
        Operation::Pocket(op) => std::iter::once("Boundary".into())
            .chain((0..op.islands.len()).map(|i| format!("Island {}", i + 1)))
            .collect(),
        Operation::Carve(op) => std::iter::once("Boundary".into())
            .chain((0..op.islands.len()).map(|i| format!("Island {}", i + 1)))
            .collect(),
        Operation::Engrave(op) if !op.closed => vec!["Open path".into()],
        _ => vec!["Boundary".into()],
    }
}

impl App {
    pub(super) fn geometry_list(&self, labels: Vec<String>) -> Element<'_, Message> {
        let count = labels.len();
        let selected = self
            .geometry_row_selection
            .filter(|index| *index < count)
            .or_else(|| (count == 1).then_some(0));
        let mut items = column![].spacing(2).width(Length::Fill);
        for (index, label) in labels.into_iter().enumerate() {
            let active = selected == Some(index);
            items = items.push(
                container(
                    row![
                        button(text(label).size(12).width(Length::Fill))
                            .padding([8, 9])
                            .width(Length::Fill)
                            .style(move |theme, status| inspector_tab(theme, status, active))
                            .on_press(Message::GeometryRowSelected(index)),
                        inspector_button("×").on_press(Message::GeometryRemove(index)),
                    ]
                    .spacing(2)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill),
            );
        }
        if count == 0 {
            items = items.push(
                container(text("No geometry selected").size(12).color(INSPECTOR_MUTED)).padding(9),
            );
        }
        column![
            row![
                text("Geometry selection").size(12).width(Length::Fill),
                inspector_button(if self.geometry_additive {
                    "Selecting…"
                } else {
                    "Select"
                })
                .style(|theme, status| inspector_tab(theme, status, self.geometry_additive))
                .on_press(Message::GeometryPick),
                inspector_button("Clear")
                    .on_press_maybe((count > 0).then_some(Message::GeometryClear)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            container(scrollable(items).width(Length::Fill).height(104))
                .padding(4)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.12, 0.14, 0.17))),
                    border: Border {
                        color: Color::from_rgb(0.25, 0.28, 0.32),
                        width: 1.0,
                        radius: 5.0.into()
                    },
                    ..container::Style::default()
                }),
        ]
        .spacing(6)
        .into()
    }

    pub(super) fn pick_operation_geometry(&mut self) -> iced::Task<Message> {
        self.profile_tab_picking = false;
        self.geometry_additive = true;
        if self.creating_operation()
            || self.cad_editing_op.is_some()
            || self.drawing_editing_op.is_some()
        {
            self.status =
                "Click geometry in the viewport to add it. Use × to remove an item.".into();
            return iced::Task::none();
        }
        if let Selection::Operation(id) = self.controller.selection() {
            let message = if self.controller.geometry_state().bindings.contains_key(&id) {
                Message::EditCadGeometry(id)
            } else {
                Message::EditDrawingGeometry(id)
            };
            return self.update(message);
        }
        iced::Task::none()
    }

    pub(super) fn remove_operation_geometry(&mut self, index: Option<usize>) {
        if self.cad_pending_kind.is_none()
            && self.cad_editing_op.is_none()
            && self.controller.pending_op().is_none()
        {
            if let (Some(index), Selection::Operation(id)) = (index, self.controller.selection()) {
                if !self.controller.geometry_state().bindings.contains_key(&id) {
                    if let Some(mut op) = self.current_operation(id).cloned() {
                        let removed = match &mut op {
                            Operation::Drill(op)
                                if op.points.len() > 1 && index < op.points.len() =>
                            {
                                op.points.remove(index);
                                true
                            }
                            Operation::Thread(op)
                                if op.points.len() > 1 && index < op.points.len() =>
                            {
                                op.points.remove(index);
                                true
                            }
                            Operation::Pocket(op) if index > 0 && index <= op.islands.len() => {
                                op.islands.remove(index - 1);
                                true
                            }
                            Operation::Carve(op) if index > 0 && index <= op.islands.len() => {
                                op.islands.remove(index - 1);
                                true
                            }
                            _ => false,
                        };
                        if removed {
                            self.operation_draft = Some(op);
                            self.geometry_row_selection = None;
                            return;
                        }
                    }
                }
            }
            let _ = self.pick_operation_geometry();
        }
        self.geometry_row_selection = None;
        self.cad_candidates.clear();
        if self.cad_pending_kind.is_some() || self.cad_editing_op.is_some() {
            let mut selected = self.controller.cad_selection().to_vec();
            if let Some(index) = index {
                if index < selected.len() {
                    selected.remove(index);
                }
            } else {
                selected.clear();
            }
            if let Err(error) = self.controller.try_set_cad_selection(selected) {
                self.status = error;
            }
        } else if self.controller.pending_op().is_some() {
            let mut selected = self.controller.pending_operation_geometry();
            if let Some(index) = index {
                if index < selected.len() {
                    selected.remove(index);
                }
            } else {
                selected.clear();
            }
            if let Err(error) = self
                .controller
                .set_pending_operation_geometry(&selected, None)
            {
                self.status = error;
            }
        }
    }
}
