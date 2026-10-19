//! Backend-driven operation tabs. Tab changes only change the view of one draft.
use super::*;
use cam_model::Comp;
use iced::widget::column;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum OperationTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Passes,
    Linking,
}
impl OperationTab {
    pub(super) const ALL: [Self; 5] = [
        Self::Tool,
        Self::Geometry,
        Self::Heights,
        Self::Passes,
        Self::Linking,
    ];
}
impl std::fmt::Display for OperationTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tool => "Tool",
            Self::Geometry => "Geometry",
            Self::Heights => "Heights",
            Self::Passes => "Passes",
            Self::Linking => "Linking",
        })
    }
}

/// The primary tab for each numeric field. Preset pass parameters also appear
/// on Tool; operation_fields determines availability independently of the tab.
pub(super) fn field_tab(field: Field) -> OperationTab {
    match field {
        Field::SpindleRpm
        | Field::Feed
        | Field::PlungeFeed
        | Field::RampFeed
        | Field::LeadInFeed
        | Field::LeadOutFeed
        | Field::ClearFeed
        | Field::ClearPlungeFeed => OperationTab::Tool,
        Field::MajorDia
        | Field::TabWidth
        | Field::TabHeight
        | Field::TabSpacing => OperationTab::Geometry,
        Field::Clearance
        | Field::Retract
        | Field::TopOfStock
        | Field::Depth
        | Field::OperationTop
        | Field::DrillStartOffset
        | Field::FaceStartOffset
        | Field::ThreadTop
        | Field::ThreadBottom
        | Field::ThreadBlindAllowance
        | Field::ChamferTop
        | Field::ChamferDepth => OperationTab::Heights,
        Field::LeadInSize
        | Field::LeadOutSize
        | Field::LeadOverlap
        | Field::PlungeA
        | Field::PlungeB
        | Field::ClearLeadInSize
        | Field::ClearLeadOutSize
        | Field::ClearLeadOverlap
        | Field::ClearPlungeA
        | Field::ClearPlungeB
        | Field::ThreadDrillClearance
        | Field::StartX
        | Field::StartY => OperationTab::Linking,
        _ => OperationTab::Passes,
    }
}

pub(super) fn operation_start(op: &Operation) -> Option<[f64; 2]> {
    match op {
        Operation::Profile(o) => o.start,
        Operation::Pocket(o) => o.start,
        Operation::Chamfer(o) if o.closed => o.start,
        Operation::Engrave(o) if o.closed => o.start,
        Operation::Carve(o) => o.start,
        _ => None,
    }
}
pub(super) fn set_operation_start(op: &mut Operation, enabled: bool) {
    let start = match op {
        Operation::Profile(o) => &mut o.start,
        Operation::Pocket(o) => &mut o.start,
        Operation::Chamfer(o) if o.closed => &mut o.start,
        Operation::Engrave(o) if o.closed => &mut o.start,
        Operation::Carve(o) => &mut o.start,
        _ => return,
    };
    *start = if enabled {
        Some(start.unwrap_or([0.0, 0.0]))
    } else {
        None
    };
}
pub(super) fn set_operation_start_coordinate(op: &mut Operation, axis: usize, value: f64) {
    if axis >= 2 || !value.is_finite() {
        return;
    }
    let start = match op {
        Operation::Profile(o) => &mut o.start,
        Operation::Pocket(o) => &mut o.start,
        Operation::Chamfer(o) if o.closed => &mut o.start,
        Operation::Engrave(o) if o.closed => &mut o.start,
        Operation::Carve(o) => &mut o.start,
        _ => return,
    };
    if let Some(point) = start {
        point[axis] = value;
    }
}

/// Numeric bounds reflect the strategy's actual meanings, including zero values
/// used for automatic/full-depth settings. Cross-field geometry gates remain in
/// the job resolver; an inspector never relaxes those gates.
pub(super) fn operation_field_invalid(op: &Operation, field: Field, value: f64) -> bool {
    if !value.is_finite() {
        return true;
    }
    match field {
        Field::Depth
        | Field::ChamferWidth
        | Field::Feed
        | Field::PlungeFeed
        | Field::MajorDia
        | Field::Pitch => value <= 0.0,
        Field::TabWidth | Field::TabSpacing => value <= 0.0,
        Field::TabHeight => value <= 0.0,
        Field::Stepdown if matches!(op, Operation::Profile(_) | Operation::Face(_)) => value <= 0.0,
        Field::ThreadPasses => value < 1.0 || value.fract() != 0.0 || value > u32::MAX as f64,
        Field::ThreadSpringPasses => value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64,
        Field::Overlap | Field::ClearOverlap => !(0.0..=99.0).contains(&value),
        Field::PlungeA | Field::ClearPlungeA => {
            let (main, clear) = App::op_plunge_styles(op);
            match if field == Field::ClearPlungeA {
                clear
            } else {
                main
            } {
                Some(Plunge::Ramp { .. } | Plunge::ZigZag { .. }) => !(0.0 < value && value < 90.0),
                Some(Plunge::Helix { .. }) => value <= 0.0,
                _ => false,
            }
        }
        Field::PlungeB | Field::ClearPlungeB => value <= 0.0,
        Field::Clearance
        | Field::Retract
        | Field::TopOfStock
        | Field::OperationTop
        | Field::StartX
        | Field::StartY
        | Field::DrillStartOffset
        | Field::ThreadTop
        | Field::ThreadBottom
        | Field::ChamferTop
        | Field::FaceOvershoot => false,
        Field::ProfileOffset if matches!(op, Operation::Profile(_)) => false,
        _ => value < 0.0,
    }
}

/// The existing Climb checkbox, with illustrated English hover help.
fn profile_climb_control(climb: bool, show_help: bool) -> Element<'static, Message> {
    let control = row![
        text("Climb")
            .size(12)
            .width(Length::Fill)
            .color(INSPECTOR_TEXT),
        checkbox(climb)
            .size(15)
            .on_toggle(Message::ClearingClimbToggled),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if !show_help {
        return control.into();
    }
    let help = column![
        text("Climb and conventional milling").size(15).font(iced::Font { weight: iced::font::Weight::Semibold, ..iced::Font::DEFAULT }),
        text("Checked: Climb (down milling). The chip starts thick and becomes thinner as the cutting edge exits the material.").size(12),
        text("Unchecked: Conventional (up milling). The chip starts thin and becomes thicker through the cut.").size(12),
        iced::widget::svg(iced::widget::svg::Handle::from_memory(include_bytes!("../assets/help/climb-conventional.svg"))).width(440).height(160),
        text("The profile's travel direction follows this setting, the spindle rotation and the selected Inside/Outside side. It also applies to radial roughing. On-contour slotting engages both sides of the cutter; here the setting reverses travel.").size(12),
    ].spacing(10).width(440);
    delayed_tooltip(
        control,
        container(help).padding(12).style(popup_surface),
        tooltip::Position::Left,
    )
    .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compensation {
    Computed,
    Left,
    Right,
}
impl Compensation {
    const ALL: [Self; 3] = [Self::Computed, Self::Left, Self::Right];
    fn of(comp: Comp) -> Self {
        match comp {
            Comp::Computed => Self::Computed,
            Comp::ControlLeft => Self::Left,
            Comp::ControlRight => Self::Right,
        }
    }
    fn available(post: PostKind) -> &'static [Self] {
        if matches!(post, PostKind::Grbl | PostKind::FluidNc | PostKind::GrblHal | PostKind::Estlcam) {
            &Self::ALL[..1]
        } else {
            &Self::ALL
        }
    }
    fn value(self) -> Comp {
        match self {
            Self::Computed => Comp::Computed,
            Self::Left => Comp::ControlLeft,
            Self::Right => Comp::ControlRight,
        }
    }
}
impl std::fmt::Display for Compensation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Computed => "Computed",
            Self::Left => "Control G41",
            Self::Right => "Control G42",
        })
    }
}

impl App {
    pub(super) fn operation_inspector(&self, id: u32) -> Element<'_, Message> {
        let creating = self.creating_operation();
        let creation_preview = creating.then(|| self.creation_operation().ok()).flatten();
        let Some(op) = creation_preview
            .as_ref()
            .map(|(op, _)| op)
            .or_else(|| self.current_operation(id))
        else {
            return text("Operation unavailable").into();
        };
        let tab = self.operation_tab;
        let linked = self
            .operation_binding_preview
            .as_ref()
            .or_else(|| self.controller.geometry_state().bindings.get(&id))
            .or_else(|| {
                creation_preview
                    .as_ref()
                    .and_then(|(_, binding)| binding.as_ref())
            });
        let heading = if creating {
            format!("New {} operation", op_kind(op))
        } else {
            format!("Operation {id} · {}", op_kind(op))
        };
        let mut list = column![inspector_heading(heading)].spacing(10).padding(14);
        // Two rows keep every tab reachable in a narrow inspector. Navigation
        // never reloads the shared numeric buffers or operation draft.
        let tab_button = |value: OperationTab| {
            inspector_button(value.to_string())
                .padding([7, 4])
                .width(Length::Fill)
                .style(move |theme, status| inspector_tab(theme, status, tab == value))
                .on_press(Message::OperationTabChanged(value))
        };
        list = list.push(
            column![
                row(OperationTab::ALL[..3]
                    .iter()
                    .copied()
                    .map(|value| tab_button(value).into()))
                .spacing(3),
                row(OperationTab::ALL[3..]
                    .iter()
                    .copied()
                    .map(|value| tab_button(value).into()))
                .spacing(3),
            ]
            .spacing(3),
        );
        let header = list;
        let mut list = column![inspector_section(&tab.to_string())]
            .spacing(10)
            .padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 16.0,
                left: 14.0,
            });
        if tab == OperationTab::Tool {
            let tool = self
                .operation_tools
                .iter()
                .chain(self.controller.document().setup().tools.iter())
                .find(|tool| tool.number == op.tool());
            list = list.push(
                text(match tool {
                    Some(tool) => format!(
                        "T{} · {} · ⌀ {:.3} mm",
                        tool.number,
                        ToolKindPick::of(tool.kind),
                        tool.diameter
                    ),
                    None => format!("T{} · tool missing", op.tool()),
                })
                .size(12),
            );
            let choices: Vec<ToolChoice> = self
                .library
                .tools
                .iter()
                .enumerate()
                .filter(|(_, tool)| {
                    families_for(AppController::operation_kind(op))
                        .contains(&ToolKindPick::of(tool.kind))
                })
                .map(|(index, tool)| ToolChoice {
                    index,
                    number: tool.number,
                    diameter: tool.diameter,
                    kind: tool.kind,
                })
                .collect();
            let selected = tool.and_then(|selected| {
                choices
                    .iter()
                    .find(|choice| {
                        self.library.tools[choice.index].identity() == selected.identity()
                    })
                    .copied()
            });
            list = list.push(
                pick_list(choices, selected, |choice| {
                    Message::OperationToolPicked(choice.index)
                })
                .placeholder("Choose cutter")
                .text_size(12)
                .padding([7, 9])
                .style(inspector_picker)
                .width(Length::Fill),
            );
            list = list.push(self.operation_preset_picker(op));
            list = list.push(
                text("A matching material preset is applied when choosing a cutter. All cutting values can be edited below.")
                    .size(11)
                    .color(palette::GROUP_LABEL),
            );
            list = list.push(
                text("RPM 0 inherits the job default. Cutting feeds are operation values.")
                    .size(11)
                    .color(palette::GROUP_LABEL),
            );
        }
        if tab == OperationTab::Geometry && creating {
            list = list.push(self.creation_geometry_controls());
        }
        if tab == OperationTab::Geometry && !creating {
            let geometry = match op {
                Operation::Profile(p) => format!(
                    "{} profile contour(s) · {}",
                    1 + p.additional_chains.len(),
                    if p.closed { "closed" } else { "open" }
                ),
                Operation::Chamfer(op) if !op.faces.is_empty() => format!(
                    "{} modeled chamfer face(s) · upper and lower boundaries",
                    op.faces.len()
                ),
                Operation::Chamfer(op) => if op.closed {
                    "One closed chain"
                } else {
                    "One open chain"
                }
                .into(),
                Operation::Engrave(o) => {
                    format!("One {} chain", if o.closed { "closed" } else { "open" })
                }
                Operation::Pocket(o) => format!("Boundary with {} islands", o.islands.len()),
                Operation::Carve(o) => format!("Boundary with {} islands", o.islands.len()),
                Operation::Face(_)
                    if linked.is_some_and(|binding| {
                        binding
                            .selected
                            .iter()
                            .any(|r| r.kind == EntityKind::Silhouette)
                    }) =>
                {
                    "Projected facing boundary".into()
                }
                Operation::Face(_) => "Rectangular facing boundary".into(),
                Operation::Drill(o) => format!("{} hole centres", o.points.len()),
                Operation::Thread(o) => format!("{} thread centres", o.points.len()),
            };
            list = list.push(text(geometry).size(12));
            if let Some(binding) = linked {
                list = list.push(self.geometry_list(cad_geometry_labels(&binding.selected)));
            } else {
                list = list.push(self.geometry_list(operation_geometry_labels(op)));
            }
        }

        if tab == OperationTab::Heights {
            list = list.push(self.reference_height_controls(id));
        }
        if tab == OperationTab::Linking {
            let supports_start = matches!(
                op,
                Operation::Profile(_) | Operation::Pocket(_) | Operation::Carve(_)
            ) || matches!(op, Operation::Engrave(o) if o.closed)
                || matches!(op, Operation::Chamfer(o) if o.closed && o.faces.is_empty());
            if supports_start {
                list = list.push(
                    row![
                        text("Preferred start XY").size(12),
                        checkbox(operation_start(op).is_some())
                            .size(15)
                            .on_toggle(Message::OperationStartEnabled)
                    ]
                    .spacing(8),
                );
            }
            match op {
                Operation::Drill(_) => list = list.push(text("Approach and retract follow the drilling cycle and operation safety heights.").size(12)),
                Operation::Face(_) => list = list.push(text("Facing links and turnaround arcs are determined by the strategy. Overshoot is on Passes.").size(12)),
                Operation::Thread(_) => list = list.push(text("Helical entry and exit are determined by the threading strategy.").size(12)),
                Operation::Engrave(o) if !o.closed => list = list.push(text("Open strokes start at their first vertex. Entry and exit are determined by the engraving strategy.").size(12)),
                _ => {}
            }
        }
        for field in operation_fields(op) {
            if tab == OperationTab::Heights && field != Field::ThreadBlindAllowance {
                continue;
            }
            if matches!(
                field,
                Field::TabWidth | Field::TabHeight | Field::TabSpacing
            ) {
                continue; // Grouped beneath the holding-tabs toggle.
            }
            if let Operation::Chamfer(chamfer) = op {
                if field == Field::ChamferWidth
                    || (chamfer.faces.is_empty() && field == Field::Stepdown)
                    || (!chamfer.faces.is_empty()
                        && matches!(
                            field,
                            Field::ChamferWidth
                                | Field::ChamferDepth
                                | Field::ChamferStep
                                | Field::LeadInSize
                                | Field::LeadOutSize
                                | Field::LeadOverlap
                        ))
                {
                    continue;
                }
            }
            let preset_parameter = tab == OperationTab::Tool
                && matches!(
                    field,
                    Field::Stepdown | Field::Overlap | Field::Peck | Field::ChamferStep
                );
            if (!preset_parameter && field_tab(field) != tab) || is_clear_field(field) {
                continue;
            }
            if linked.is_some()
                && matches!(
                    field,
                    Field::Depth
                        | Field::OperationTop
                        | Field::DrillStartOffset
                        | Field::FaceStartOffset
                        | Field::ThreadTop
                        | Field::ThreadBottom
                        | Field::ChamferTop
                )
            {
                continue;
            }
            let value = if creating
                && field == Field::MajorDia
                && !self.creation_edited_fields.contains(&field)
            {
                op_field(op, field).map(fmt_num).unwrap_or_default()
            } else {
                self.fields.get(&field).cloned().unwrap_or_default()
            };
            list = list.push(field_row_labeled(
                field,
                self.field_label(field),
                self.field_help(field),
                &value,
                self.tooltips,
                self.field_invalid(field),
            ));
        }
        match op {
            Operation::Profile(p) => {
                if tab == OperationTab::Geometry && linked.is_none() {
                    let comp = if p.comp == Comp::Computed {
                        p.comp
                    } else if p.clearing.climb {
                        Comp::ControlLeft
                    } else {
                        Comp::ControlRight
                    };
                    list = list.push(profile_picker(
                        "Compensation",
                        "Computed offsets the geometry; Control uses the machine's G41/G42 cutter compensation. Climb in Passes also determines the compensation side.",
                        self.tooltips,
                        Compensation::of(comp),
                        Compensation::available(self.controller.post_kind()),
                        |value| Message::ProfileCompChanged(value.value()),
                    ));
                }
                if tab == OperationTab::Geometry {
                    list = list.push(iced::widget::rule::horizontal(1));
                    list = list.push(
                        row![
                            text("Holding tabs").size(12),
                            checkbox(p.tabs.enabled).size(15).on_toggle_maybe(
                                (p.tabs.enabled || (p.closed && p.additional_chains.is_empty()))
                                    .then_some(Message::ProfileTabsEnabled)
                            )
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                    if !p.closed || !p.additional_chains.is_empty() {
                        list = list.push(
                            text("Holding tabs require a single closed contour.")
                                .size(11)
                                .color(INSPECTOR_MUTED),
                        );
                    }
                    if !p.closed {
                        list = list.push(text("Open chains use computed compensation and straight plunge, without leads or radial roughing.").size(11).color(INSPECTOR_MUTED));
                    }
                    if p.tabs.enabled {
                        list = list.push(text("Tab shape: Rectangular").size(12));
                        for field in [Field::TabWidth, Field::TabHeight] {
                            let value = self.fields.get(&field).cloned().unwrap_or_default();
                            list = list.push(field_row_labeled(
                                field,
                                self.field_label(field),
                                self.field_help(field),
                                &value,
                                self.tooltips,
                                self.field_invalid(field),
                            ));
                        }
                        list = list.push(profile_picker(
                            "Tab positioning", "Place tabs automatically by contour distance, or select locations on the contour.", self.tooltips,
                            if p.tabs.manual { profile_tabs::Positioning::Manual } else { profile_tabs::Positioning::ByDistance },
                            &profile_tabs::Positioning::ALL[..], Message::ProfileTabPositioning,
                        ));
                        if !p.tabs.manual {
                            let field = Field::TabSpacing;
                            let value = self.fields.get(&field).cloned().unwrap_or_default();
                            list = list.push(field_row_labeled(
                                field,
                                self.field_label(field),
                                self.field_help(field),
                                &value,
                                self.tooltips,
                                self.field_invalid(field),
                            ));
                        }
                        list = list.push(self.tab_location_controls(p));
                    }
                }
                if tab == OperationTab::Passes {
                    list = list.push(profile_climb_control(p.clearing.climb, self.tooltips));
                }
                if tab == OperationTab::Linking {
                    list = list.push(profile_picker(
                        "Lead-in",
                        help::LEAD_IN,
                        self.tooltips,
                        LeadKind::of(p.lead_in),
                        &LeadKind::ALL[..],
                        Message::LeadInKindChanged,
                    ));
                    list = list.push(profile_picker(
                        "Lead-out",
                        help::LEAD_OUT,
                        self.tooltips,
                        LeadKind::of(p.lead_out),
                        &LeadKind::ALL[..],
                        Message::LeadOutKindChanged,
                    ));
                    list = list.push(profile_picker(
                        "Plunge",
                        help::PLUNGE,
                        self.tooltips,
                        PlungeKind::of(p.plunge),
                        &PlungeKind::ALL[..],
                        Message::PlungeKindChanged,
                    ));
                }
            }
            Operation::Pocket(p) => {
                if tab == OperationTab::Passes {
                    list = list.push(
                        row![
                            label_help("Climb", help::CLIMB, self.tooltips),
                            checkbox(p.clear.clearing.climb)
                                .size(15)
                                .on_toggle(Message::ClearingClimbToggled)
                        ]
                        .spacing(8),
                    );
                }
                if tab == OperationTab::Linking {
                    list = list.push(profile_picker(
                        "Plunge",
                        help::PLUNGE,
                        self.tooltips,
                        PlungeKind::of(p.clear.plunge),
                        &PlungeKind::ALL[..],
                        Message::PlungeKindChanged,
                    ));
                    list = list.push(profile_picker(
                        "Wall lead-in",
                        help::LEAD_IN,
                        self.tooltips,
                        LeadKind::of(p.clear.lead_in),
                        &LeadKind::ALL[..],
                        Message::LeadInKindChanged,
                    ));
                    list = list.push(profile_picker(
                        "Wall lead-out",
                        help::LEAD_OUT,
                        self.tooltips,
                        LeadKind::of(p.clear.lead_out),
                        &LeadKind::ALL[..],
                        Message::LeadOutKindChanged,
                    ));
                }
            }
            Operation::Face(f) if tab == OperationTab::Passes => {
                list = list.push(profile_picker(
                    "Direction",
                    help::FACE_DIRECTION,
                    self.tooltips,
                    f.direction,
                    &Axis::ALL[..],
                    Message::FaceDirectionChanged,
                ))
            }
            Operation::Chamfer(c) if !c.faces.is_empty() => {
                if tab == OperationTab::Passes {
                    list = list.push(text("Stepdown limits the height of each face band. 0 uses the cutter's usable flank automatically; smaller cutters use more passes.").size(12));
                }
                if tab == OperationTab::Linking {
                    list = list.push(text("Each face band approaches from the air side and retracts before repositioning.").size(12));
                }
            }
            Operation::Chamfer(c) => {
                if tab == OperationTab::Geometry {
                    list = list.push(profile_picker(
                        "Side",
                        help::SIDE,
                        self.tooltips,
                        c.side,
                        &Side::ALL[..],
                        Message::SideChanged,
                    ));
                }
                if tab == OperationTab::Passes {
                    let field = Field::ChamferWidth;
                    let value = self.fields.get(&field).cloned().unwrap_or_default();
                    list = list
                        .push(inspector_section("Fase"))
                        .push(field_row_labeled(
                            field,
                            self.field_label(field),
                            self.field_help(field),
                            &value,
                            self.tooltips,
                            self.field_invalid(field),
                        ));
                    list = list.push(
                        row![
                            label_help("Gradual", help::CHAMFER_GRADUAL, self.tooltips),
                            checkbox(c.gradual)
                                .size(15)
                                .on_toggle(Message::ChamferGradualToggled)
                        ]
                        .spacing(8),
                    );
                }
                if tab == OperationTab::Linking {
                    list = list.push(profile_picker(
                        "Lead-in",
                        help::LEAD_IN,
                        self.tooltips,
                        LeadKind::of(c.lead_in),
                        &LeadKind::ALL[..],
                        Message::LeadInKindChanged,
                    ));
                    list = list.push(profile_picker(
                        "Lead-out",
                        help::LEAD_OUT,
                        self.tooltips,
                        LeadKind::of(c.lead_out),
                        &LeadKind::ALL[..],
                        Message::LeadOutKindChanged,
                    ));
                }
            }
            Operation::Thread(t) => {
                if tab == OperationTab::Geometry {
                    list = list.push(profile_picker(
                        "Bore",
                        help::BORE,
                        self.tooltips,
                        Bore::of(t.internal),
                        &Bore::ALL[..],
                        |b| Message::ThreadInternalChanged(b == Bore::Internal),
                    ));
                }
                if tab == OperationTab::Passes {
                    list = list.push(profile_picker(
                        "Hand",
                        help::HAND,
                        self.tooltips,
                        t.hand,
                        &Hand::ALL[..],
                        Message::ThreadHandChanged,
                    ));
                    list = list.push(profile_picker(
                        "Cut",
                        help::THREAD_CUT,
                        self.tooltips,
                        CutStyle::of(t.climb),
                        &CutStyle::ALL[..],
                        |c| Message::ThreadClimbChanged(c == CutStyle::Climb),
                    ));
                    list = list.push(
                        row![
                            label_help("Gradual", help::THREAD_GRADUAL, self.tooltips),
                            checkbox(t.gradual)
                                .size(15)
                                .on_toggle(Message::ThreadGradualToggled)
                        ]
                        .spacing(8),
                    );
                }
            }
            Operation::Engrave(o) if tab == OperationTab::Geometry => {
                if linked.is_none() {
                    list = list.push(
                        row![
                            text("Closed chain").size(12),
                            checkbox(o.closed)
                                .size(15)
                                .on_toggle(Message::EngraveClosedChanged)
                        ]
                        .spacing(8),
                    );
                } else {
                    list = list.push(
                        text("Chain closure follows the qualified CAD binding.")
                            .size(11)
                            .color(palette::GROUP_LABEL),
                    );
                }
            }
            Operation::Carve(c) => {
                if tab == OperationTab::Linking {
                    list = list.push(profile_picker(
                        "Plunge",
                        help::CARVE_PLUNGE,
                        self.tooltips,
                        PlungeKind::of(c.plunge),
                        &PlungeKind::ALL[..],
                        Message::PlungeKindChanged,
                    ));
                    list = list.push(
                        row![
                            label_help("Stay down", help::CARVE_STAY_DOWN, self.tooltips),
                            checkbox(c.stay_down)
                                .size(15)
                                .on_toggle(Message::CarveStayDownToggled)
                        ]
                        .spacing(8),
                    );
                }
                if tab == OperationTab::Passes {
                    if let Some(shape) = self
                        .controller
                        .document()
                        .setup()
                        .tools
                        .iter()
                        .find(|t| t.number == c.tool)
                        .and_then(|t| cam_toolpath::carve_shape(c, t))
                    {
                        list = list.push(
                            text(format!(
                                "Full shape depth {:.3} mm · tool limit {:.3} mm · {} flat areas",
                                shape.full_depth, shape.tool_max_depth, shape.flat_areas
                            ))
                            .size(12)
                            .color(palette::GROUP_LABEL),
                        );
                    }
                }
                if matches!(
                    tab,
                    OperationTab::Tool | OperationTab::Passes | OperationTab::Linking
                ) {
                    list = list.push(iced::widget::rule::horizontal(1)).push(
                        text("Clearing pass (end mill)")
                            .size(12)
                            .color(palette::GROUP_LABEL),
                    );
                    if tab == OperationTab::Tool {
                        list = list.push(
                            row![
                                label_help("Clear flat areas", help::CARVE_CLEAR, self.tooltips),
                                checkbox(c.clear.is_some())
                                    .size(15)
                                    .on_toggle(Message::CarveClearToggled)
                            ]
                            .spacing(8),
                        );
                        if let Some(cl) = &c.clear {
                            let choices: Vec<ToolChoice> = self
                                .library
                                .tools
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| {
                                    matches!(t.kind, ToolKind::EndMill | ToolKind::BullNose { .. })
                                })
                                .map(|(index, t)| ToolChoice {
                                    index,
                                    number: t.number,
                                    diameter: t.diameter,
                                    kind: t.kind,
                                })
                                .collect();
                            let current = self
                                .operation_tools
                                .iter()
                                .chain(self.controller.document().setup().tools.iter())
                                .find(|t| t.number == cl.tool)
                                .and_then(|embedded| {
                                    self.library
                                        .tools
                                        .iter()
                                        .position(|l| l.identity() == embedded.identity())
                                })
                                .and_then(|i| choices.iter().find(|ch| ch.index == i).cloned());
                            list = list.push(
                                pick_list(choices, current, |ch| {
                                    Message::CarveClearToolChanged(ch.index)
                                })
                                .placeholder("Choose an end mill")
                                .text_size(12)
                                .width(Length::Fill),
                            );
                            list = list.push(text("Clearing feeds 0 inherit the V-bit feeds. One operation RPM applies to both tools.").size(11).color(palette::GROUP_LABEL));
                        }
                    }
                    if let Some(cl) = &c.clear {
                        for field in operation_fields(op) {
                            if !is_clear_field(field) || field_tab(field) != tab {
                                continue;
                            }
                            let value = self.fields.get(&field).cloned().unwrap_or_default();
                            list = list.push(field_row_labeled(
                                field,
                                self.field_label(field),
                                self.field_help(field),
                                &value,
                                self.tooltips,
                                self.field_invalid(field),
                            ));
                        }
                        if tab == OperationTab::Passes {
                            list = list.push(
                                row![
                                    label_help("Clearing climb", help::CLIMB, self.tooltips),
                                    checkbox(cl.params.clearing.climb)
                                        .size(15)
                                        .on_toggle(Message::CarveClearClimbToggled)
                                ]
                                .spacing(8),
                            );
                        }
                        if tab == OperationTab::Linking {
                            list = list.push(profile_picker(
                                "Clearing plunge",
                                help::CARVE_CLEAR_PLUNGE,
                                self.tooltips,
                                PlungeKind::of(cl.params.plunge),
                                &PlungeKind::ALL[..],
                                Message::CarveClearPlungeChanged,
                            ));
                            list = list.push(profile_picker(
                                "Clearing lead-in",
                                help::LEAD_IN,
                                self.tooltips,
                                LeadKind::of(cl.params.lead_in),
                                &LeadKind::ALL[..],
                                Message::CarveClearLeadInChanged,
                            ));
                            list = list.push(profile_picker(
                                "Clearing lead-out",
                                help::LEAD_OUT,
                                self.tooltips,
                                LeadKind::of(cl.params.lead_out),
                                &LeadKind::ALL[..],
                                Message::CarveClearLeadOutChanged,
                            ));
                        }
                    } else if tab != OperationTab::Tool {
                        list = list.push(
                            text("Enable a clearing tool on Tool to edit its parameters.")
                                .size(11)
                                .color(palette::GROUP_LABEL),
                        );
                    }
                }
            }
            _ => {}
        }
        let footer = inspector_footer(
            row![
                inspector_button("Cancel").on_press(if creating {
                    Message::CancelOp
                } else {
                    Message::CancelOperationDraft
                }),
                Space::new().width(Length::Fill),
                inspector_button(if creating {
                    "Create operation"
                } else {
                    "Apply changes"
                })
                .style(inspector_primary)
                .on_press_maybe(if creating {
                    (self.profile_entry_editor.is_none() && self.creation_ready())
                        .then_some(Message::ConfirmOp)
                } else {
                    (self.profile_entry_editor.is_none()
                        && !self.any_field_invalid()
                        && self.operation_height_overrides_valid()
                        && self.inspector_dirty())
                    .then_some(Message::Apply)
                }),
            ]
            .spacing(8),
        );
        column![
            header,
            scrollable(list).width(Length::Fill).height(Length::Fill),
            footer
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub(super) fn drawing_geometry_picker(&self) -> Element<'_, Message> {
        let count = self.controller.pending_operation_geometry().len();
        column![
            text("Drawing geometry").size(16),
            text("Click a boundary, hole or path. Ctrl/Command adds compatible features.").size(12),
            self.geometry_list(drawing_geometry_labels(
                &self.controller.pending_operation_geometry()
            )),
            Space::new().height(Length::Fill),
            row![
                inspector_button("Use geometry")
                    .on_press_maybe((count > 0).then_some(Message::UseDrawingGeometry)),
                inspector_button("Cancel selection").on_press(Message::CancelDrawingGeometry)
            ]
            .spacing(6)
        ]
        .spacing(8)
        .padding(8)
        .height(Length::Fill)
        .into()
    }

    pub(super) fn preview_geometry_draft(
        &self,
        id: u32,
        selected: Vec<EntityRef>,
    ) -> Result<cam_model::GeometryBinding, String> {
        if self.any_field_invalid() {
            return Err("Correct invalid operation fields before using geometry".into());
        }
        let mut operation = self
            .current_operation(id)
            .cloned()
            .ok_or("Operation unavailable")?;
        let fields = self
            .inspector_fields()
            .into_iter()
            .map(|field| {
                self.fields
                    .get(&field)
                    .and_then(|text| text.parse::<f64>().ok())
                    .map(|number| (field, number))
                    .ok_or("Invalid operation field")
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        apply_op_fields(&mut operation, &fields);
        let parse = |text: &str| -> Result<Option<f64>, String> {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                text.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Some)
                    .ok_or("Linked heights must be finite or blank".into())
            }
        };
        let overrides = cam_model::GeometryOverrides {
            top: parse(&self.cad_override_top)?,
            bottom: parse(&self.cad_override_bottom)?,
        };
        let used = operation.tools();
        let tools = self
            .operation_tools
            .iter()
            .cloned()
            .filter(|tool| used.contains(&tool.number))
            .collect();
        self.controller.preview_operation_settings_with_geometry(
            operation,
            Some(overrides),
            tools,
            selected,
        )
    }

    pub(super) fn operation_height_overrides_valid(&self) -> bool {
        if self.library_mode() || self.creating_operation() {
            return true;
        }
        let Selection::Operation(id) = self.controller.selection() else {
            return true;
        };
        let Some(binding) = self
            .operation_binding_preview
            .as_ref()
            .or_else(|| self.controller.geometry_state().bindings.get(&id))
        else {
            return true;
        };
        let parse = |value: &str, inherited: f64| -> Option<f64> {
            if value.trim().is_empty() {
                Some(inherited)
            } else {
                value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
            }
        };
        matches!((parse(&self.cad_override_top, binding.heights.top), parse(&self.cad_override_bottom, binding.heights.bottom)), (Some(top), Some(bottom)) if top.is_finite() && bottom.is_finite() && top > bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operations() -> Vec<Operation> {
        let mut controller = AppController::new(crate::default_machine());
        controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        for kind in [
            OpKind::Profile,
            OpKind::Pocket,
            OpKind::Face,
            OpKind::Drill,
            OpKind::Thread,
            OpKind::Chamfer,
            OpKind::Engrave,
            OpKind::Carve,
        ] {
            controller.new_operation(kind);
        }
        controller
            .document()
            .setup()
            .operations
            .iter()
            .rev()
            .take(8)
            .cloned()
            .collect()
    }

    #[test]
    fn all_eight_families_have_real_numeric_fields_on_the_expected_tabs() {
        let operations = operations();
        assert_eq!(operations.len(), 8);
        for operation in operations {
            let fields = operation_fields(&operation);
            assert!(fields.contains(&Field::SpindleRpm));
            assert!(fields.contains(&Field::Feed));
            for field in fields {
                assert!(OperationTab::ALL.contains(&field_tab(field)));
                assert!(
                    op_field(&operation, field).is_some(),
                    "{} {field:?}",
                    op_kind(&operation)
                );
            }
        }
        assert_eq!(
            field_tab(Field::ThreadDrillClearance),
            OperationTab::Linking
        );
        assert_eq!(
            field_tab(Field::ThreadBlindAllowance),
            OperationTab::Heights
        );
        assert_eq!(field_tab(Field::ClearFeed), OperationTab::Tool);
        assert_eq!(field_tab(Field::ClearPlungeA), OperationTab::Linking);
        assert_eq!(field_tab(Field::MajorDia), OperationTab::Geometry);
        assert_eq!(field_tab(Field::ChamferWidth), OperationTab::Passes);
    }

    #[test]
    fn preferred_start_round_trips_and_open_engrave_cannot_reorder_its_endpoints() {
        for mut operation in operations() {
            if let Operation::Engrave(engrave) = &mut operation {
                engrave.closed = true;
            }
            let supported = matches!(
                operation,
                Operation::Profile(_)
                    | Operation::Pocket(_)
                    | Operation::Chamfer(_)
                    | Operation::Engrave(_)
                    | Operation::Carve(_)
            );
            set_operation_start(&mut operation, true);
            assert_eq!(operation_start(&operation).is_some(), supported);
            if supported {
                apply_op_fields(
                    &mut operation,
                    &BTreeMap::from([(Field::StartX, -12.5), (Field::StartY, 7.0)]),
                );
                assert_eq!(operation_start(&operation), Some([-12.5, 7.0]));
                assert!(operation_fields(&operation).contains(&Field::StartX));
                set_operation_start(&mut operation, false);
                assert_eq!(operation_start(&operation), None);
                assert!(!operation_fields(&operation).contains(&Field::StartX));
            }
        }
        let mut engrave = operations()
            .into_iter()
            .find(|o| matches!(o, Operation::Engrave(_)))
            .unwrap();
        if let Operation::Engrave(o) = &mut engrave {
            o.closed = false;
        }
        set_operation_start(&mut engrave, true);
        assert_eq!(operation_start(&engrave), None);
    }

    #[test]
    fn manual_surface_planes_are_signed_and_round_trip_for_carve_and_engrave() {
        for mut operation in operations()
            .into_iter()
            .filter(|o| matches!(o, Operation::Carve(_) | Operation::Engrave(_)))
        {
            assert!(operation_fields(&operation).contains(&Field::OperationTop));
            assert!(!operation_field_invalid(
                &operation,
                Field::OperationTop,
                -3.5
            ));
            apply_op_fields(
                &mut operation,
                &BTreeMap::from([(Field::OperationTop, -3.5)]),
            );
            assert_eq!(op_field(&operation, Field::OperationTop), Some(-3.5));
        }
    }

    #[test]
    fn validation_preserves_automatic_settings_and_rejects_nonfinite_and_fractional_counts() {
        let operations = operations();
        let profile = operations
            .iter()
            .find(|op| matches!(op, Operation::Profile(_)))
            .unwrap();
        let pocket = operations
            .iter()
            .find(|op| matches!(op, Operation::Pocket(_)))
            .unwrap();
        let thread = operations
            .iter()
            .find(|op| matches!(op, Operation::Thread(_)))
            .unwrap();
        assert!(operation_field_invalid(profile, Field::Stepdown, 0.0));
        assert!(!operation_field_invalid(pocket, Field::Stepdown, 0.0));
        assert!(!operation_field_invalid(profile, Field::Stepover, 0.0));
        assert!(!operation_field_invalid(profile, Field::SpindleRpm, 0.0));
        assert!(operation_field_invalid(thread, Field::ThreadPasses, 1.5));
        assert!(operation_field_invalid(
            thread,
            Field::ThreadSpringPasses,
            -1.0
        ));
        assert!(!operation_field_invalid(
            thread,
            Field::ThreadSpringPasses,
            0.0
        ));
        for field in operation_fields(profile) {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert!(operation_field_invalid(profile, field, value));
            }
        }
        assert!(operation_field_invalid(pocket, Field::Overlap, 100.0));
        assert!(operation_field_invalid(pocket, Field::Overlap, 99.9));
        assert!(!operation_field_invalid(pocket, Field::Overlap, 99.0));
    }

    #[test]
    fn profile_tabs_survive_toggles_apply_undo_and_cancel() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::ProfileTabsEnabled(true));
        for (field, value) in [
            (Field::TabWidth, "5"),
            (Field::TabHeight, "1"),
            (Field::TabSpacing, "40"),
        ] {
            assert_eq!(field_tab(field), OperationTab::Geometry);
            let _ = app.update(Message::FieldChanged(field, value.into()));
        }
        let _ = app.update(Message::ProfileTabsEnabled(false));
        let _ = app.update(Message::ProfileTabsEnabled(true));
        assert_eq!(
            app.fields.get(&Field::TabWidth).map(String::as_str),
            Some("5")
        );
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Profile(p))
            if p.tabs.enabled && p.tabs.width == 5.0 && p.tabs.height == 1.0 && p.tabs.spacing == 40.0)
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.operation(id), Some(&before));
        app.begin_operation_draft(id);
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.current_operation(id), Some(&before));
    }

    #[test]
    fn switching_tabs_retains_draft_and_cancel_discards_every_tab() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let committed = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationStartEnabled(true));
        let _ = app.update(Message::FieldChanged(Field::StartX, "-4.5".into()));
        let _ = app.update(Message::SideChanged(Side::Inside));
        for tab in OperationTab::ALL {
            let _ = app.update(Message::OperationTabChanged(tab));
            assert_eq!(app.operation_tab, tab);
            assert_eq!(
                app.fields.get(&Field::Feed).map(String::as_str),
                Some("432.1")
            );
            assert_eq!(
                app.fields.get(&Field::StartX).map(String::as_str),
                Some("-4.5")
            );
            assert!(
                matches!(app.current_operation(id), Some(Operation::Profile(o)) if o.side == Side::Inside && o.start.is_some())
            );
        }
        assert_eq!(app.controller.operation(id), Some(&committed));
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.current_operation(id), Some(&committed));
        assert!(!app.inspector_dirty());
    }
    #[test]
    fn tool_tab_feed_edit_changes_emitted_motion_for_every_family() {
        use cam_cldata::{SpindleDir, Step};
        let mut controller = AppController::new(crate::default_machine());
        controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        let baseline = controller.document().clone();
        for mut operation in operations() {
            let kind = match operation {
                Operation::Thread(_) => ToolKind::ThreadMill { pitch: None },
                Operation::Carve(_) | Operation::Engrave(_) => ToolKindPick::VBit.to_kind(),
                Operation::Chamfer(_) => ToolKindPick::ChamferMill.to_kind(),
                Operation::Drill(_) => ToolKindPick::Drill.to_kind(),
                _ => ToolKind::EndMill,
            };
            let mut document = baseline.clone();
            document.setup_mut().tools = vec![cam_model::Tool {
                number: operation.tool(),
                diameter: 6.0,
                length: 40.0,
                flute_length: 30.0,
                neck_length: 30.0,
                neck_diameter: 2.0,
                kind,
                ..Default::default()
            }];
            apply_op_fields(
                &mut operation,
                &BTreeMap::from([(Field::Feed, 432.1), (Field::SpindleRpm, 9000.0)]),
            );
            document.setup_mut().operations = vec![operation.clone()];
            let job = cam_toolpath::build_job(
                &document,
                8000.0,
                SpindleDir::Cw,
                None,
                20.0,
                &cam_toolpath::CancelToken::new(),
            );
            assert!(
                !job.diagnostics
                    .iter()
                    .any(|d| d.severity == cam_toolpath::Severity::Error),
                "{}: {:?}",
                op_kind(&operation),
                job.diagnostics
            );
            assert!(
                job.program.steps().iter().any(|step| match step {
                    Step::Linear { feed, .. } | Step::Arc { feed, .. } => *feed == 432.1,
                    Step::Drill(cycle) => cycle.feed == 432.1,
                    _ => false,
                }),
                "{}: edited feed must reach the motion program",
                op_kind(&operation)
            );
            assert!(job
                .program
                .steps()
                .iter()
                .any(|step| matches!(step, Step::Spindle { rpm, .. } if *rpm == 9000.0)));
        }
    }

    #[test]
    fn ok_commits_enum_feed_and_geometry_start_from_separate_tabs_in_one_undo_step() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.document().setup().clone();
        let _ = app.update(Message::OperationTabChanged(OperationTab::Tool));
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        let _ = app.update(Message::SideChanged(Side::Inside));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::ClearingClimbToggled(false));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        let _ = app.update(Message::OperationStartEnabled(true));
        let _ = app.update(Message::FieldChanged(Field::StartX, "-4.5".into()));
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Profile(o)) if o.feed == 432.1 && o.side == Side::Inside && !o.clearing.climb && o.start == Some([-4.5, 0.0]))
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.document().setup(), &before);
    }

    #[test]
    fn compensation_availability_and_radial_passes_follow_post_and_strategy() {
        for post in [PostKind::Grbl, PostKind::FluidNc, PostKind::GrblHal, PostKind::Estlcam] {
            assert_eq!(Compensation::available(post), &[Compensation::Computed]);
        }
        for post in [
            PostKind::Fanuc,
            PostKind::Haas,
            PostKind::LinuxCnc,
            PostKind::Okuma,
        ] {
            assert_eq!(Compensation::available(post).len(), 3);
        }
        let mut operation = operations()
            .into_iter()
            .find(|o| matches!(o, Operation::Profile(_)))
            .unwrap();
        assert!(operation_fields(&operation).contains(&Field::Stepover));
        if let Operation::Profile(o) = &mut operation {
            o.comp = Comp::ControlLeft;
        }
        assert!(!operation_fields(&operation).contains(&Field::Stepover));
        assert!(!operation_fields(&operation).contains(&Field::Engagement));
    }
    fn linked_face_app() -> (App, u32) {
        use cam_model::*;
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        let points = [
            [0.0, 0.0, -1.0],
            [10.0, 0.0, -1.0],
            [10.0, 10.0, -1.0],
            [0.0, 10.0, -1.0],
        ];
        let mut topology = TopologySnapshot {
            bodies: vec![CadBody {
                id: 1,
                name: "Analytic test plate".into(),
                valid_solid: true,
                bounds: [[0.0, 0.0, -5.0], [10.0, 10.0, -1.0]],
            }],
            vertices: points
                .iter()
                .enumerate()
                .map(|(i, point)| CadVertex {
                    id: 10 + i as u64,
                    body_id: 1,
                    point: *point,
                })
                .collect(),
            edges: (0..4)
                .map(|i| CadEdge {
                    id: 20 + i as u64,
                    body_id: 1,
                    vertices: [10 + i as u64, 10 + ((i + 1) % 4) as u64],
                    face_ids: vec![2],
                    curve: CadCurve::Line {
                        start: points[i],
                        end: points[(i + 1) % 4],
                    },
                })
                .collect(),
            loops: vec![CadLoop {
                id: 3,
                body_id: 1,
                face_id: 2,
                edge_uses: (20..24)
                    .map(|edge_id| EdgeUse {
                        edge_id,
                        reversed: false,
                    })
                    .collect(),
                is_outer: true,
            }],
            faces: vec![CadFace {
                id: 2,
                body_id: 1,
                surface: CadSurface::Plane {
                    origin: points[0],
                    normal: [0.0, 0.0, 1.0],
                },
                loop_ids: vec![3],
                bounds: [[0.0, 0.0, -1.0], [10.0, 10.0, -1.0]],
            }],
            ..Default::default()
        };
        let mut second_loop = topology.loops[0].clone();
        second_loop.id = 5;
        second_loop.face_id = 4;
        topology.loops.push(second_loop);
        let mut second_face = topology.faces[0].clone();
        second_face.id = 4;
        second_face.loop_ids = vec![5];
        topology.faces.push(second_face);
        for edge in &mut topology.edges {
            edge.face_ids.push(4);
        }
        app.controller
            .add_cad_asset(
                GeometryAsset {
                    id: 1,
                    revision: 1,
                    name: "Analytic test plate".into(),
                    source_format: CadFormat::Step,
                    source_data: vec![].into(),
                    import_options: None,
                    import_diagnostics: vec![],
                    source_fingerprint: String::new(),
                    source_units: SourceUnits::Millimetres,
                    backend: "analytic fixture (not imported STEP)".into(),
                    topology: topology.into(),
                },
                vec![GeometryInstance {
                    id: 1,
                    asset_id: 1,
                    body_id: 1,
                    name: "plate".into(),
                    placement: Default::default(),
                    visible: true,
                }],
            )
            .unwrap();
        app.controller
            .edit_setup(|setup| setup.model_instances = vec![1]);
        // This fixture faces 1 mm of stock above the model's top face.
        app.controller.edit_stock(|stock| {
            *stock = Stock::RelativeBox {
                offsets: [[0.0; 3], [0.0, 0.0, 1.0]],
            }
        });
        let reference = EntityRef {
            asset_id: 1,
            revision: 1,
            instance_id: 1,
            body_id: 1,
            kind: EntityKind::Face,
            id: 2,
        };
        let id = app
            .controller
            .create_cad_operation(OpKind::Face, vec![reference])
            .unwrap();
        app.begin_operation_draft(id);
        (app, id)
    }

    #[test]
    fn linked_height_overrides_and_other_tabs_commit_and_undo_atomically() {
        let (mut app, id) = linked_face_app();
        let before = app.controller.document().clone();
        let _ = app.update(Message::OperationTabChanged(OperationTab::Tool));
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::FaceDirectionChanged(Axis::Y));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Heights));
        let _ = app.update(Message::CadOverrideTopChanged("0.5".into()));
        let _ = app.update(Message::CadOverrideBottomChanged("-0.8".into()));
        assert!(app.operation_height_overrides_valid());
        let _ = app.update(Message::FaceDirectionChanged(Axis::Y));
        assert_eq!(app.cad_override_bottom, "-0.8");
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        assert_eq!(app.cad_override_top, "0.5");
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Face(o)) if o.feed == 432.1 && o.direction == Axis::Y),
            "status: {}; fields: {:?}; invalid: {}",
            app.status,
            app.fields,
            app.any_field_invalid()
        );
        assert_eq!(
            app.controller.geometry_state().bindings[&id].overrides,
            cam_model::GeometryOverrides {
                top: Some(0.5),
                bottom: Some(-0.8)
            }
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn nonfinite_or_inverted_linked_heights_block_ok_and_cancel_discards_them() {
        let (mut app, _) = linked_face_app();
        let before = app.controller.document().clone();
        for (top, bottom) in [("NaN", "-2"), ("inf", "-2"), ("0", "1"), ("0", "0")] {
            let _ = app.update(Message::CadOverrideTopChanged(top.into()));
            let _ = app.update(Message::CadOverrideBottomChanged(bottom.into()));
            assert!(!app.operation_height_overrides_valid());
            let _ = app.update(Message::Apply);
            assert_eq!(app.controller.document(), &before);
        }
        let _ = app.update(Message::CancelOperationDraft);
        assert!(app.operation_height_overrides_valid());
        assert!(!app.inspector_dirty());
    }
    #[test]
    fn replacement_cad_sources_wait_for_ok_and_cancel_restores_all_tabs() {
        let (mut app, id) = linked_face_app();
        let before = app.controller.document().clone();
        let mut replacement = app.controller.geometry_state().bindings[&id].selected[0].clone();
        replacement.id = 4;
        let stage = |app: &mut App| {
            let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
            let _ = app.update(Message::EditCadGeometry(id));
            app.controller
                .try_set_cad_selection(vec![replacement.clone()])
                .unwrap();
            let _ = app.update(Message::ConfirmCadOperation);
            assert_eq!(
                app.operation_geometry.as_deref(),
                Some(std::slice::from_ref(&replacement))
            );
            assert!(app.operation_binding_preview.is_some());
            assert_eq!(app.fields[&Field::Feed], "432.1");
        };
        stage(&mut app);
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.controller.document(), &before);
        assert!(!app.inspector_dirty());
        assert!(app.operation_binding_preview.is_none());
        stage(&mut app);
        let _ = app.update(Message::Apply);
        assert_eq!(
            app.controller.geometry_state().bindings[&id].selected,
            vec![replacement]
        );
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Face(op)) if op.feed == 432.1)
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn drawing_reselection_retains_cutting_edits_and_is_one_transaction() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.document().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationStartEnabled(true));
        let _ = app.update(Message::FieldChanged(Field::StartX, "invalid".into()));
        assert!(app.any_field_invalid());
        let _ = app.update(Message::OperationStartEnabled(false));
        assert!(
            !app.any_field_invalid(),
            "disabled controls do not block OK"
        );
        let _ = app.update(Message::EditDrawingGeometry(id));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::hole(0, 0)], None)
            .unwrap();
        let _ = app.update(Message::UseDrawingGeometry);
        assert_eq!(app.fields[&Field::Feed], "432.1");
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Profile(op)) if op.feed == 432.1)
        );
        assert_ne!(app.controller.document(), &before);
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }
    #[test]
    fn creation_from_an_operation_seeds_the_new_draft_before_editing() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let old_id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.setup_draft = None;
        app.begin_operation_draft(old_id);
        let old = app.controller.operation(old_id).unwrap().clone();
        let _ = app.update(Message::BeginOp(OpKind::Face));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        let _ = app.update(Message::ConfirmOp);
        let Selection::Operation(new_id) = app.controller.selection() else {
            panic!("new operation must be selected");
        };
        assert_ne!(old_id, new_id);
        assert_eq!(app.operation_draft.as_ref().unwrap().id(), new_id);
        let _ = app.update(Message::FaceDirectionChanged(Axis::Y));
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(new_id), Some(Operation::Face(op)) if op.direction == Axis::Y && op.feed == 432.1)
        );
        assert_eq!(app.controller.operation(old_id), Some(&old));
        app.focus_ops.clear();
        app.focus_ops.insert(new_id);
        app.setup_draft = None;
        let _ = app.update(Message::ClickOp(new_id));
        assert_eq!(app.controller.selection(), Selection::Setup);
        assert!(app.setup_draft.is_none());
        assert!(app.pane_handle(Pane::Inspector).is_none());
    }
    #[test]
    fn step_creation_uses_shared_cutting_controls_and_one_undo_transaction() {
        let (mut app, old_id) = linked_face_app();
        app.setup_draft = None;
        let reference = app.controller.geometry_state().bindings[&old_id].selected[0].clone();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Face));
        assert!(app.creating_operation());
        assert_eq!(app.operation_tab, OperationTab::Tool);
        assert_eq!(
            app.inspector_fields(),
            operation_fields(app.operation_draft.as_ref().unwrap())
        );
        assert!(app.inspector_fields().contains(&Field::Stepdown));
        assert!(
            !app.cad_inspecting,
            "STEP uses the common creation inspector"
        );
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::FieldChanged(Field::SpindleRpm, "9000".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::FaceDirectionChanged(Axis::Y));
        let _ = app.update(Message::FieldChanged(Field::Stepdown, "0.75".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Heights));
        let _ = app.update(Message::HeightOffsetChanged(
            cam_model::HeightPlaneKind::Clearance,
            "12.50".into(),
        ));
        let _ = app.update(Message::CadSelect(reference));
        assert_eq!(
            app.controller.cad_selection().len(),
            1,
            "editing creation feeds must not lock geometry picking"
        );
        assert_eq!(app.fields[&Field::Feed], "432.1");
        assert_eq!(
            app.controller.document(),
            &before,
            "tool choice remains uncommitted"
        );
        let _ = app.update(Message::ConfirmOp);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("new operation selected");
        };
        assert_ne!(id, old_id);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Face(op)) if op.feed == 432.1 && op.spindle_rpm == 9000.)
        );
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Face(op)) if op.direction == Axis::Y && op.stepdown == 0.75)
        );
        assert_eq!(
            app.controller.document().setup().height_settings[&id]
                .definition(cam_model::HeightPlaneKind::Clearance)
                .offset,
            12.5
        );
        assert!(app.cad_pending_kind.is_none());
        assert!(!app.cad_inspecting);
        assert_eq!(app.operation_draft.as_ref().unwrap().id(), id);
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn cad_geometry_list_remove_clear_and_readd_preserve_creation_cutting_data() {
        let (mut app, old_id) = linked_face_app();
        app.setup_draft = None;
        let reference = app.controller.geometry_state().bindings[&old_id].selected[0].clone();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Face));
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::CadSelect(reference.clone()));
        let _ = app.update(Message::GeometryRowSelected(0));
        let _ = app.update(Message::GeometryRemove(0));
        assert!(app.controller.cad_selection().is_empty());
        let _ = app.update(Message::ConfirmOp);
        assert!(app.cad_pending_kind.is_some());
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::GeometryPick);
        let _ = app.update(Message::CadSelect(reference));
        assert_eq!(app.controller.cad_selection().len(), 1);
        assert_eq!(app.fields[&Field::Feed], "432.1");
        let _ = app.update(Message::GeometryClear);
        assert!(app.controller.cad_selection().is_empty());
        let _ = app.update(Message::CancelOp);
        assert_eq!(app.controller.document(), &before);
        assert!(!app.geometry_additive);
    }

    #[test]
    fn select_replaces_a_single_face_instead_of_appending_an_invalid_second_target() {
        let (mut app, id) = linked_face_app();
        let original = app.controller.geometry_state().bindings[&id].selected[0].clone();
        let mut replacement = original;
        replacement.id = 4;
        let _ = app.update(Message::GeometryPick);
        let _ = app.update(Message::CadSelect(replacement.clone()));
        assert_eq!(app.controller.cad_selection(), &[replacement]);
        assert_eq!(app.cad_editing_op, Some(id));
    }

    #[test]
    fn removing_committed_cad_geometry_opens_an_uncommitted_replacement_list() {
        let (mut app, id) = linked_face_app();
        let before = app.controller.document().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::GeometryRemove(0));
        assert_eq!(app.cad_editing_op, Some(id));
        assert!(app.controller.cad_selection().is_empty());
        let _ = app.update(Message::ConfirmCadOperation);
        let _ = app.update(Message::Apply);
        assert_eq!(app.controller.document(), &before);
        assert_eq!(app.fields[&Field::Feed], "432.1");
        let _ = app.update(Message::CadEscape);
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.controller.document(), &before);
        assert!(!app.inspector_dirty());
    }

    #[test]
    fn drawing_geometry_list_can_remove_islands_and_clear_the_boundary() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.setup_draft = None;
        app.refresh_fields();
        let _ = app.update(Message::BeginOp(OpKind::Pocket));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0), LoopRef::hole(0, 0)], None)
            .unwrap();
        let before = app.controller.document().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::GeometryRemove(1));
        assert_eq!(
            app.controller.pending_operation_geometry(),
            vec![LoopRef::outer(0)]
        );
        let _ = app.update(Message::GeometryClear);
        assert!(app.controller.pending_operation_geometry().is_empty());
        assert!(!app.controller.pending_ready());
        assert_eq!(app.fields[&Field::Feed], "432.1");
        let _ = app.update(Message::ConfirmOp);
        assert_eq!(app.controller.document(), &before);
        assert!(app.controller.pending_op().is_some());
    }

    #[test]
    fn step_creation_cancel_and_invalid_cutting_text_leave_old_operation_untouched() {
        let (mut app, id) = linked_face_app();
        app.setup_draft = None;
        let reference = app.controller.geometry_state().bindings[&id].selected[0].clone();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Face));
        let _ = app.update(Message::CadSelect(reference));
        let _ = app.update(Message::FieldChanged(Field::Feed, "NaN".into()));
        let _ = app.update(Message::Apply); // Enter in the shared wizard cannot edit the old operation.
        assert_eq!(app.controller.document(), &before);
        assert!(app.cad_pending_kind.is_some());
        let _ = app.update(Message::CancelOp);
        assert!(app.cad_pending_kind.is_none());
        assert_eq!(app.controller.document(), &before);
        assert!(!app.inspector_dirty());
    }
    #[test]
    fn new_operations_share_all_tabs_and_commit_parameters_heights_and_geometry_together() {
        use cam_model::HeightPlaneKind as P;
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.operation_tab = OperationTab::Heights;
        app.setup_draft = None;
        app.controller.select(Selection::Setup);
        app.refresh_fields();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Profile));
        assert_eq!(app.operation_tab, OperationTab::Tool);
        let _ = app.inspector();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::FieldChanged(Field::Stepdown, "0.75".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        let _ = app.update(Message::LeadInKindChanged(LeadKind::Linear));
        let _ = app.update(Message::FieldChanged(Field::LeadInSize, "2.5".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Heights));
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "12.50".into()));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        assert!(app.creation_ready(), "{}", app.status);
        let camera = OrbitCamera::framed([0., 0., -10.], [100., 80., 25.]);
        let preview = app.height_preview(camera, 0.).unwrap();
        assert_eq!(preview.planes[0].value_label, "17.50");
        for tab in OperationTab::ALL {
            let _ = app.update(Message::OperationTabChanged(tab));
            let _ = app.inspector();
        }
        assert_eq!(app.fields[&Field::Feed], "432.1");
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::ConfirmOp);
        assert!(!app.creating_operation(), "{}", app.status);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("created operation selected");
        };
        let Operation::Profile(op) = app.controller.operation(id).unwrap() else {
            panic!("profile");
        };
        assert_eq!(op.feed, 432.1);
        assert_eq!(op.stepdown, 0.75);
        assert!(matches!(op.lead_in, Lead::Linear { length } if length == 2.5));
        assert_eq!(
            app.controller.document().setup().height_settings[&id]
                .definition(P::Clearance)
                .offset,
            12.5
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn creation_rejects_invalid_heights_and_cancel_discards_every_tab() {
        use cam_model::HeightPlaneKind as P;
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.setup_draft = None;
        app.controller.select(Selection::Setup);
        app.refresh_fields();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Face));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        let _ = app.update(Message::FaceDirectionChanged(Axis::Y));
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "NaN".into()));
        assert!(!app.creation_ready());
        let _ = app.update(Message::Apply);
        assert!(app.creating_operation());
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::CancelOp);
        assert!(!app.creating_operation());
        assert_eq!(app.controller.document(), &before);
    }
    #[test]
    fn every_new_operation_uses_the_tabbed_editor_and_real_selected_geometry() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.setup_draft = None;
        app.controller.select(Selection::Setup);
        app.refresh_fields();
        let candidates: Vec<_> = app
            .controller
            .regions()
            .iter()
            .enumerate()
            .flat_map(|(i, region)| {
                std::iter::once(LoopRef::outer(i))
                    .chain((0..region.holes().len()).map(move |j| LoopRef::hole(i, j)))
            })
            .collect();
        for kind in [
            OpKind::Profile,
            OpKind::Pocket,
            OpKind::Face,
            OpKind::Drill,
            OpKind::Thread,
            OpKind::Chamfer,
            OpKind::Engrave,
            OpKind::Carve,
        ] {
            let before = app.controller.document().clone();
            let _ = app.update(Message::BeginOp(kind));
            assert!(app.creating_operation(), "{kind:?}: {}", app.status);
            assert_eq!(app.operation_tab, OperationTab::Tool);
            for tab in OperationTab::ALL {
                let _ = app.update(Message::OperationTabChanged(tab));
                let _ = app.inspector();
            }
            let geometry = candidates
                .iter()
                .find(|r| {
                    app.controller
                        .planar_operation_eligibility(kind, &[**r])
                        .is_ok()
                })
                .unwrap();
            app.controller
                .set_pending_operation_geometry(&[*geometry], None)
                .unwrap();
            if kind == OpKind::Thread {
                let _ = app.update(Message::FieldChanged(Field::MajorDia, "7.5".into()));
                assert!(
                    matches!(app.creation_operation().unwrap().0, Operation::Thread(op) if op.major_dia == 7.5)
                );
            }
            assert!(
                app.creation_ready(),
                "{kind:?}: {:?}",
                app.creation_operation()
            );
            let _ = app.update(Message::ConfirmOp);
            assert!(!app.creating_operation(), "{kind:?}: {}", app.status);
            assert!(app.controller.undo());
            assert_eq!(app.controller.document(), &before);
            app.controller.select(Selection::Setup);
            app.operation_draft = None;
            app.refresh_fields();
        }
    }

    #[test]
    fn cad_surface_geometry_creates_clears_and_switches_to_an_open_engraving() {
        for kind in [OpKind::Chamfer, OpKind::Engrave, OpKind::Carve] {
            let (mut app, _) = linked_face_app();
            let before = app.controller.document().clone();
            let _ = app.update(Message::BeginOp(kind));
            let mut reference = EntityRef {
                asset_id: 1,
                revision: 1,
                instance_id: 1,
                body_id: 1,
                kind: EntityKind::Loop,
                id: 3,
            };
            if kind == OpKind::Carve {
                reference.kind = EntityKind::Face;
                reference.id = 2;
            }
            let _ = app.update(Message::CadSelect(reference.clone()));
            assert!(
                app.creation_ready(),
                "{kind:?}: {:?}",
                app.creation_operation()
            );
            assert_eq!(app.resolve_height_editor(0).unwrap().top, -1.);
            let _ = app.update(Message::FieldChanged(Field::Feed, "321".into()));
            if kind == OpKind::Engrave {
                let _ = app.update(Message::HeightOffsetChanged(
                    cam_model::HeightPlaneKind::Bottom,
                    "-0.25".into(),
                ));
            }
            let _ = app.update(Message::GeometryClear);
            assert!(!app.creation_ready());
            let _ = app.update(Message::GeometryPick);
            let _ = app.update(Message::CadSelect(reference));
            assert_eq!(app.fields[&Field::Feed], "321");
            if kind == OpKind::Engrave {
                let _ = app.update(Message::GeometryFilterChanged(CadFilter::Edge));
                assert!(app.controller.cad_selection().is_empty());
                let _ = app.update(Message::CadSelect(EntityRef {
                    asset_id: 1,
                    revision: 1,
                    instance_id: 1,
                    body_id: 1,
                    kind: EntityKind::Edge,
                    id: 20,
                }));
                assert!(
                    matches!(app.creation_operation().unwrap().0, Operation::Engrave(op) if !op.closed)
                );
                assert_eq!(
                    app.height_offsets[&cam_model::HeightPlaneKind::Bottom],
                    "-0.25"
                );
                assert!(!app.operation_geometry_preview().line_vertices().is_empty());
                assert_eq!(app.machining_viewport().cad_filter, EntityKind::Edge);
            }
            assert_eq!(app.controller.document(), &before);
            let _ = app.update(Message::ConfirmOp);
            assert!(!app.creating_operation(), "{kind:?}: {}", app.status);
            assert!(app.controller.undo());
            assert_eq!(app.controller.document(), &before);
        }
    }

    #[test]
    fn cad_icon_settings_cancel_and_confirm_keep_the_operation_transactional() {
        for kind in [OpKind::Chamfer, OpKind::Engrave, OpKind::Carve] {
            let (mut app, _) = linked_face_app();
            let _ = app.update(Message::BeginOp(kind));
            let source = EntityRef {
                asset_id: 1,
                revision: 1,
                instance_id: 1,
                body_id: 1,
                kind: if kind == OpKind::Carve {
                    EntityKind::Face
                } else {
                    EntityKind::Loop
                },
                id: if kind == OpKind::Carve { 2 } else { 3 },
            };
            let _ = app.update(Message::CadSelect(source.clone()));
            let _ = app.update(Message::ConfirmOp);
            assert!(!app.creating_operation(), "{kind:?}: {}", app.status);
            let _ = app.update(Message::FieldChanged(Field::Feed, "322.5".into()));
            let before = app.controller.document().clone();
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Edit));
            assert!(app.geometry_editor.is_some());
            let _ = app.inspector();
            let _ = app.update(Message::GeometryClear);
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Confirm));
            assert!(
                app.geometry_editor.is_some(),
                "Empty geometry cannot be confirmed"
            );
            let _ = app.update(Message::CadEscape);
            assert!(app.geometry_editor.is_none());
            assert_eq!(app.controller.cad_selection(), &[source.clone()]);
            assert_eq!(app.fields[&Field::Feed], "322.5");
            assert_eq!(app.controller.document(), &before);
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Add(
                CadFilter::Loop,
            )));
            if kind == OpKind::Carve {
                assert!(app.controller.cad_selection().is_empty());
            }
            let mut replacement = source;
            replacement.kind = EntityKind::Loop;
            replacement.id = 5;
            app.controller.set_cad_selection(vec![replacement.clone()]);
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Confirm));
            assert!(app.geometry_editor.is_none(), "{kind:?}: {}", app.status);
            assert_eq!(app.operation_geometry, Some(vec![replacement]));
            assert_eq!(app.fields[&Field::Feed], "322.5");
            assert_eq!(app.controller.document(), &before);
        }
    }
}
