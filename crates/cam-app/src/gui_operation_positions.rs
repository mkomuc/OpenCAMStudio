//! Independent model picks for operation entry and exit preferences.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PositionKind {
    Entry,
    Exit,
}

pub(super) fn operation_end(op: &Operation) -> Option<[f64; 2]> {
    match op {
        Operation::Profile(o) if o.closed => o.end,
        Operation::Pocket(o) => o.end,
        Operation::Chamfer(o) if o.closed && o.faces.is_empty() => o.end,
        Operation::Engrave(o) if o.closed => o.end,
        Operation::Carve(o) => o.end,
        Operation::Adaptive(o) => o.params.end,
        _ => None,
    }
}
fn set_end(op: &mut Operation, point: Option<[f64; 2]>) {
    match op {
        Operation::Profile(o) if o.closed => o.end = point,
        Operation::Pocket(o) => o.end = point,
        Operation::Chamfer(o) if o.closed && o.faces.is_empty() => o.end = point,
        Operation::Engrave(o) if o.closed => o.end = point,
        Operation::Carve(o) => o.end = point,
        Operation::Adaptive(o) => o.params.end = point,
        _ => {}
    }
}
impl App {
    pub(super) fn operation_position_controls(&self, op: &Operation) -> Element<'_, Message> {
        let mut controls = iced::widget::column![text("Positions").size(13)].spacing(6);
        for (kind, title, point) in [
            (
                PositionKind::Entry,
                "Lead-in position",
                operation_tabs::operation_start(op),
            ),
            (PositionKind::Exit, "Lead-out position", operation_end(op)),
        ] {
            let picking = self.operation_position_pick == Some(kind);
            let label = point.map_or_else(
                || title.to_string(),
                |p| format!("{title}: X {}  Y {}", fmt_num(p[0]), fmt_num(p[1])),
            );
            controls = controls.push(
                row![
                    text(label).size(12).width(Length::Fill),
                    inspector_button(if picking {
                        "Cancel selection"
                    } else {
                        "Select"
                    })
                    .on_press(Message::OperationPositionPick(kind)),
                    inspector_button("Clear")
                        .on_press_maybe(point.map(|_| Message::OperationPositionClear(kind))),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        controls.push(text("Select a point on the model: blue = entry, orange = exit. Positions use setup XY and resolve to eligible toolpath points.").size(11)).into()
    }
    pub(super) fn begin_position_pick(&mut self, kind: PositionKind) {
        self.operation_position_pick = if self.operation_position_pick == Some(kind) {
            None
        } else {
            Some(kind)
        };
        self.geometry_additive = false;
        self.geometry_auto_pick = false;
        self.profile_tab_picking = false;
        self.height_reference_pick = None;
        self.status = if self.operation_position_pick.is_some() {
            "Select the preferred position on the model; Escape cancels selection.".into()
        } else {
            "Position selection cancelled.".into()
        };
    }
    pub(super) fn clear_operation_position(&mut self, kind: PositionKind) {
        self.operation_position_pick = None;
        self.edit_operation_draft(|op| match kind {
            PositionKind::Entry => operation_tabs::set_operation_start(op, false),
            PositionKind::Exit => set_end(op, None),
        });
        self.refresh_operation_fields();
        self.status = "Position cleared; automatic placement restored. OK saves the operation.".into();
    }
    pub(super) fn pick_operation_position(&mut self, point: [f64; 2]) {
        if !point.into_iter().all(f64::is_finite) {
            return;
        }
        let Some(kind) = self.operation_position_pick.take() else {
            return;
        };
        self.edit_operation_draft(|op| match kind {
            PositionKind::Entry => {
                operation_tabs::set_operation_start(op, true);
                for (axis, value) in point.into_iter().enumerate() {
                    operation_tabs::set_operation_start_coordinate(op, axis, value);
                }
            }
            PositionKind::Exit => set_end(op, Some(point)),
        });
        if kind == PositionKind::Entry {
            self.fields.remove(&Field::StartX);
            self.fields.remove(&Field::StartY);
        }
        self.refresh_operation_fields();
        self.status = format!(
            "Preferred {} selected: X {} Y {}. OK saves the operation.",
            if kind == PositionKind::Entry {
                "entry"
            } else {
                "exit"
            },
            fmt_num(point[0]),
            fmt_num(point[1])
        );
    }
}
