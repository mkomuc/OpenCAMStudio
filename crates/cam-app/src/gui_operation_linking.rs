//! One owner for linking controls, grouped in machining order.
use super::*;
use cam_model::Comp;
use iced::widget::{column, Column};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Section {
    Moves,
    Leads,
    Ramping,
    Positions,
}
impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::Moves => "Linking moves",
            Self::Leads => "Leads and transitions",
            Self::Ramping => "Ramping",
            Self::Positions => "Positions",
        }
    }
    pub(super) fn id(self) -> operation_sections::Section {
        operation_sections::Section::new(OperationTab::Linking, self.label())
    }
    fn field(field: Field) -> Self {
        match field {
            Field::LeadInSize
            | Field::LeadOutSize
            | Field::LeadOverlap
            | Field::ClearLeadInSize
            | Field::ClearLeadOutSize
            | Field::ClearLeadOverlap
            | Field::BoreLeadRadius => Self::Leads,
            Field::PlungeA | Field::PlungeB | Field::ClearPlungeA | Field::ClearPlungeB => {
                Self::Ramping
            }
            Field::StartX | Field::StartY => Self::Positions,
            _ => Self::Moves,
        }
    }
}

impl App {
    fn linking_field_controls(
        &self,
        op: &Operation,
        section: Section,
    ) -> Vec<Element<'static, Message>> {
        let mut fields = Vec::new();
        for field in self.geometry_operation_fields(op) {
            if operation_tabs::field_tab(field) != OperationTab::Linking
                || Section::field(field) != section
                || (is_clear_field(field) && !matches!(op, Operation::Carve(_)))
            {
                continue;
            }
            let value = self.fields.get(&field).cloned().unwrap_or_default();
            fields.push(field_row_labeled(
                field,
                self.field_label(field),
                self.field_help(field),
                &value,
                self.tooltips,
                self.field_invalid(field),
            ));
        }
        fields
    }

    pub(super) fn operation_linking_controls(
        &self,
        op: &Operation,
        linked: bool,
    ) -> Element<'_, Message> {
        let mut moves: Vec<Element<'_, Message>> = Vec::new();
        let mut leads: Vec<Element<'_, Message>> = Vec::new();
        let mut ramping: Vec<Element<'_, Message>> = Vec::new();
        match op {
            Operation::Profile(p) => {
                moves.push(help_wrap(
                    row![
                        text("Retract at lead-out feed")
                            .size(12)
                            .width(Length::Fill),
                        checkbox(p.passes.feed_retract).size(15).on_toggle(|v| {
                            Message::ProfilePassToggle(profile_parameters::Toggle::FeedRetract, v)
                        }),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    "Use the lead-out feed for the retract after leaving the contour.",
                    self.tooltips,
                ));
                leads.push(profile_picker(
                    "Lead-in",
                    help::LEAD_IN,
                    self.tooltips,
                    LeadKind::of(p.lead_in),
                    if p.comp == Comp::Control {
                        &LeadKind::ALL[1..2]
                    } else {
                        &LeadKind::ALL[..]
                    },
                    Message::LeadInKindChanged,
                ));
                if !p.passes.same_lead_out {
                    leads.push(profile_picker(
                        "Lead-out",
                        help::LEAD_OUT,
                        self.tooltips,
                        LeadKind::of(p.lead_out),
                        if p.comp == Comp::Control {
                            &LeadKind::ALL[1..2]
                        } else {
                            &LeadKind::ALL[..]
                        },
                        Message::LeadOutKindChanged,
                    ));
                }
                ramping.push(profile_picker(
                    "Plunge",
                    help::PLUNGE,
                    self.tooltips,
                    PlungeKind::of(p.plunge),
                    if p.capabilities(linked, self.controller.post_kind().supports_cutter_comp())
                        .ramp
                    {
                        &PlungeKind::ALL[..]
                    } else {
                        &PlungeKind::ALL[..1]
                    },
                    Message::PlungeKindChanged,
                ));
            }
            Operation::Pocket(p) => {
                leads.push(profile_picker(
                    "Wall lead-in",
                    help::LEAD_IN,
                    self.tooltips,
                    LeadKind::of(p.clear.lead_in),
                    &LeadKind::ALL[..],
                    Message::LeadInKindChanged,
                ));
                leads.push(profile_picker(
                    "Wall lead-out",
                    help::LEAD_OUT,
                    self.tooltips,
                    LeadKind::of(p.clear.lead_out),
                    &LeadKind::ALL[..],
                    Message::LeadOutKindChanged,
                ));
                ramping.push(profile_picker(
                    "Plunge",
                    help::PLUNGE,
                    self.tooltips,
                    PlungeKind::of(p.clear.plunge),
                    &PlungeKind::ALL[..],
                    Message::PlungeKindChanged,
                ));
            }
            Operation::Adaptive(_) => {
                moves.push(label_help(
                    "Retract along cleared path",
                    "The exit follows the final connected cleared path before retracting. A preferred exit resolves along this safe path.",
                    self.tooltips,
                ));
                ramping.push(label_help(
                    "Helical entry",
                    "3D Adaptive Clearing uses safe helical entries near the preferred entry. Set Ramp feed on Tool to override the entry rate.",
                    self.tooltips,
                ));
            }
            Operation::Drill(_) => {
                moves.push(label_help(
                    "Cycle approach and retract",
                    "Approach and retract follow the drilling cycle and operation safety heights.",
                    self.tooltips,
                ));
            }
            Operation::Face(_) => {
                moves.push(label_help("Automatic links and turnarounds",
                    "Facing links and turnaround arcs are determined by the strategy. Overshoot is on Passes.", self.tooltips));
            }
            Operation::Thread(_) => {
                leads.push(label_help(
                    "Helical entry and exit",
                    "Helical entry and exit are determined by the threading strategy.",
                    self.tooltips,
                ));
            }
            Operation::Engrave(o) if !o.closed => {
                leads.push(label_help("Entry and exit follow the stroke",
                    "Open strokes start at their first vertex. Entry and exit are determined by the engraving strategy.", self.tooltips));
            }
            Operation::Chamfer(c) if !c.faces.is_empty() => {
                moves.push(label_help("Air-side approach",
                    "Each face band approaches from the air side and retracts before repositioning.", self.tooltips));
            }
            Operation::Chamfer(c) => {
                leads.push(profile_picker(
                    "Lead-in",
                    help::LEAD_IN,
                    self.tooltips,
                    LeadKind::of(c.lead_in),
                    &LeadKind::ALL[..],
                    Message::LeadInKindChanged,
                ));
                leads.push(profile_picker(
                    "Lead-out",
                    help::LEAD_OUT,
                    self.tooltips,
                    LeadKind::of(c.lead_out),
                    &LeadKind::ALL[..],
                    Message::LeadOutKindChanged,
                ));
            }
            Operation::Bore(b) => {
                if b.internal {
                    leads.push(help_wrap(
                        row![
                            text("Lead to centre").size(12).width(Length::Fill),
                            checkbox(b.lead_to_center).size(15).on_toggle(|v| Message::BoreOption(BoreOption::Center(v))),
                        ].spacing(8),
                        "Tangent horizontal leads; retract to clearance between passes and features.",
                        self.tooltips,
                    ));
                }
            }
            Operation::Carve(c) => {
                moves.push(
                    (row![
                        label_help("Stay down", help::CARVE_STAY_DOWN, self.tooltips),
                        checkbox(c.stay_down)
                            .size(15)
                            .on_toggle(Message::CarveStayDownToggled),
                    ]
                    .spacing(8))
                    .into(),
                );
                moves.push(help_wrap(
                    inspector_section_toggle(
                        "Clearing pass (end mill)",
                        c.clear.is_some(),
                        Some(Message::CarveClearToggled),
                    ),
                    help::CARVE_CLEAR,
                    self.tooltips,
                ));
                ramping.push(profile_picker(
                    "Plunge",
                    help::CARVE_PLUNGE,
                    self.tooltips,
                    PlungeKind::of(c.plunge),
                    &PlungeKind::ALL[..],
                    Message::PlungeKindChanged,
                ));
                if let Some(cl) = &c.clear {
                    leads.push(profile_picker(
                        "Clearing lead-in",
                        help::LEAD_IN,
                        self.tooltips,
                        LeadKind::of(cl.params.lead_in),
                        &LeadKind::ALL[..],
                        Message::CarveClearLeadInChanged,
                    ));
                    leads.push(profile_picker(
                        "Clearing lead-out",
                        help::LEAD_OUT,
                        self.tooltips,
                        LeadKind::of(cl.params.lead_out),
                        &LeadKind::ALL[..],
                        Message::CarveClearLeadOutChanged,
                    ));
                    ramping.push(profile_picker(
                        "Clearing plunge",
                        help::CARVE_CLEAR_PLUNGE,
                        self.tooltips,
                        PlungeKind::of(cl.params.plunge),
                        &PlungeKind::ALL[..],
                        Message::CarveClearPlungeChanged,
                    ));
                }
            }
            _ => {}
        }
        moves.extend(self.linking_field_controls(op, Section::Moves));
        leads.extend(self.linking_field_controls(op, Section::Leads));
        ramping.extend(self.linking_field_controls(op, Section::Ramping));
        if let Operation::Profile(p) = op {
            leads.push(
                self.profile_extra_controls(p, OperationTab::Linking, linked)
                    .finish(),
            );
        }
        let mut sections = Column::new().spacing(12);
        for (section, body) in [
            (Section::Moves, moves),
            (Section::Leads, leads),
            (Section::Ramping, ramping),
        ] {
            if !body.is_empty() {
                sections = sections.push(self.operation_section(
                    section.id(),
                    Column::with_children(body).spacing(10),
                    None,
                ));
            }
        }
        let supports_positions = matches!(
            op,
            Operation::Profile(_)
                | Operation::Pocket(_)
                | Operation::Carve(_)
                | Operation::Adaptive(_)
        ) || matches!(op, Operation::Engrave(o) if o.closed)
            || matches!(op, Operation::Chamfer(o) if o.closed && o.faces.is_empty());
        if supports_positions {
            let positions = column![self.operation_position_controls(op)].spacing(10);
            sections =
                sections.push(self.operation_section(Section::Positions.id(), positions, None));
        }
        sections.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsing_positions_cancels_picking_and_preserves_the_draft() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Pocket);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.setup_draft = None;
        app.begin_operation_draft(id);
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Entry,
        ));
        let _ = app.update(Message::OperationPositionPicked([0., 0., 0.]));
        let _ = app.update(Message::FieldChanged(Field::StartX, "12.5".into()));
        let draft = app.operation_draft.clone();
        let fields = app.fields.clone();
        let saved = app.controller.document().clone();
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Exit,
        ));
        let _ = app.update(Message::OperationSectionToggled(Section::Positions.id()));
        assert_eq!(app.operation_position_pick, None);
        let _ = app.update(Message::OperationPositionPicked([99., 99., 0.]));
        let _ = app.update(Message::OperationSectionToggled(Section::Positions.id()));
        assert!(!app
            .collapsed_operation_sections
            .contains(&Section::Positions.id()));
        assert_eq!(app.fields, fields);
        assert_eq!(app.operation_draft, draft);
        assert_eq!(app.controller.document(), &saved);
    }
}
