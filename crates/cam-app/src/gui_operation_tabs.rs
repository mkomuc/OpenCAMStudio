//! Backend-driven operation tabs. Tab changes only change the view of one draft.
use super::*;
use cam_model::Comp;
use iced::widget::column;
use iced::widget::svg;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum OperationTab {
    #[default]
    Tool,
    MultiAxis,
    Geometry,
    Heights,
    Passes,
    Linking,
}
impl OperationTab {
    pub(super) const ALL: [Self; 6] = [
        Self::Tool,
        Self::MultiAxis,
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
            Self::MultiAxis => "Multi-axis",
            Self::Geometry => "Geometry",
            Self::Heights => "Heights",
            Self::Passes => "Passes",
            Self::Linking => "Lead-in and lead-out moves",
        })
    }
}

impl OperationTab {
    fn hover_label(self) -> &'static str {
        match self {
            Self::Tool => "Tool",
            Self::MultiAxis => "Multi-axis",
            Self::Geometry => "Geometry",
            Self::Heights => "Heights",
            Self::Passes => "Passes",
            Self::Linking => "Lead-in and lead-out moves",
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Self::Tool => include_bytes!("../assets/icons/operation-tabs/tool.svg"),
            Self::MultiAxis => include_bytes!("../assets/icons/operation-tabs/multi_axis.svg"),
            Self::Geometry => include_bytes!("../assets/icons/operation-tabs/geometry.svg"),
            Self::Heights => include_bytes!("../assets/icons/operation-tabs/heights.svg"),
            Self::Passes => include_bytes!("../assets/icons/operation-tabs/passes.svg"),
            Self::Linking => include_bytes!("../assets/icons/operation-tabs/linking.svg"),
        }
    }
}

/// The primary tab for each numeric field. Preset pass parameters also appear
/// on Tool; operation_fields determines availability independently of the tab.
pub(super) fn field_tab(field: Field) -> OperationTab {
    match field {
        Field::SpindleRpm
        | Field::CuttingSpeed
        | Field::FeedPerTooth
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
        | Field::TabSpacing
        | Field::AdaptiveOffset => OperationTab::Geometry,
        Field::BoreDiameter | Field::BoreTaper => OperationTab::Geometry,
        Field::LiftHeight | Field::BoreSafeDistance | Field::BoreLeadRadius => {
            OperationTab::Linking
        }
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

/// Keep the main parameters together; independent options get their own sections.
fn operation_field_section(field: Field, tab: OperationTab) -> &'static str {
    match tab {
        OperationTab::Tool => "Cutting data",
        OperationTab::Geometry => "Geometry",
        OperationTab::Heights => "Blind hole clearance",
        _ if matches!(field, Field::BoreRadialStock | Field::BoreAxialStock) => "Stock to Leave",
        _ => "Passes",
    }
}

pub(super) fn operation_start(op: &Operation) -> Option<[f64; 2]> {
    match op {
        Operation::Profile(o) => o.start,
        Operation::Pocket(o) => o.start,
        Operation::Chamfer(o) if o.closed => o.start,
        Operation::Engrave(o) if o.closed => o.start,
        Operation::Carve(o) => o.start,
        Operation::Adaptive(o) => o.params.start,
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
        Operation::Adaptive(o) => &mut o.params.start,
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
        Operation::Adaptive(o) => &mut o.params.start,
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
        Field::Profile(n) => n.invalid(value),
        Field::PocketFinish(n) => n.invalid(value),
        Field::Depth
        | Field::ChamferWidth
        | Field::Feed
        | Field::PlungeFeed
        | Field::ParallelStepover
        | Field::CuttingSpeed
        | Field::FeedPerTooth
        | Field::AdaptiveTolerance
        | Field::AdaptiveOptimalLoad
        | Field::AdaptiveFineStepdown
        | Field::MajorDia
        | Field::Pitch => value <= 0.0,
        Field::StockLeaveRadial | Field::StockLeaveAxial => value < 0.0,
        Field::TabWidth | Field::TabSpacing => value <= 0.0,
        Field::TabHeight => value <= 0.0,
        Field::Stepdown
            if matches!(
                op,
                Operation::Profile(_)
                    | Operation::Face(_)
                    | Operation::Adaptive(_)
                    | Operation::Parallel(_)
            ) =>
        {
            value <= 0.0
        }
        Field::BoreDiameter | Field::BorePitch | Field::BoreTolerance | Field::BoreSafeDistance => {
            value <= 0.
        }
        Field::BoreAngle => value <= 0. || value >= 90.,
        Field::BoreStepovers => value < 0. || value.fract() != 0. || value > 10000.,
        Field::BoreTaper | Field::BoreRadialStock | Field::BoreAxialStock => false,
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
        | Field::FaceOvershoot
        | Field::ParallelAngle
        | Field::AdaptiveOffset => false,
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
    help_wrap(control, help::CLIMB, show_help)
}

fn stock_offset_row(
    field: Field,
    label: &'static str,
    help: &'static str,
    value: String,
    show_help: bool,
    invalid: bool,
) -> Element<'static, Message> {
    let mut input = inspector_input("", &value)
        .align_x(iced::alignment::Horizontal::Right)
        .on_input(move |value| Message::FieldChanged(field, value))
        .width(Length::Fixed(INSPECTOR_INPUT_W));
    if invalid {
        input = input.style(|theme, status| {
            let mut style = inspector_input_style(theme, status);
            style.border.color = palette::ERROR;
            style.border.width = 1.5;
            style.value = palette::ERROR;
            style
        });
    }
    row![label_help(label, help, show_help), input]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

fn stock_offset_fields(operation: &Operation) -> Vec<Field> {
    let mut fields = Vec::with_capacity(2);
    if matches!(
        operation,
        Operation::Profile(_)
            | Operation::Pocket(_)
            | Operation::Adaptive(_)
            | Operation::Parallel(_)
    ) {
        fields.push(Field::StockLeaveRadial);
    }
    if matches!(
        operation,
        Operation::Profile(_)
            | Operation::Pocket(_)
            | Operation::Face(_)
            | Operation::Adaptive(_)
            | Operation::Parallel(_)
    ) {
        fields.push(Field::StockLeaveAxial);
    }
    fields
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compensation {
    Computed,
    Control,
}
impl Compensation {
    const ALL: [Self; 2] = [Self::Computed, Self::Control];
    fn of(comp: Comp) -> Self {
        match comp {
            Comp::Computed => Self::Computed,
            Comp::Control => Self::Control,
        }
    }
    fn available(post: PostKind) -> &'static [Self] {
        if post.supports_cutter_comp() {
            &Self::ALL
        } else {
            &Self::ALL[..1]
        }
    }
    fn value(self) -> Comp {
        match self {
            Self::Computed => Comp::Computed,
            Self::Control => Comp::Control,
        }
    }
}
impl std::fmt::Display for Compensation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Computed => "Computed",
            Self::Control => "In control (automatic side)",
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
            .or_else(|| {
                creation_preview
                    .as_ref()
                    .and_then(|(_, binding)| binding.as_ref())
            })
            .or_else(|| {
                if creating {
                    None
                } else {
                    self.controller.geometry_state().bindings.get(&id)
                }
            });
        let heading = if creating {
            format!("New {} operation", op_kind(op))
        } else {
            format!("Operation {id} · {}", op_kind(op))
        };
        let mut list = column![inspector_heading(heading)].spacing(10).padding(14);
        // Compact buttons keep all six tabs in one row. Navigation never
        // reloads the shared numeric buffers or operation draft.
        let tab_button = |value: OperationTab| -> Element<'_, Message> {
            let button = button(
                svg(svg::Handle::from_memory(value.icon()))
                    .width(20)
                    .height(20),
            )
            .padding(3)
            .width(Length::Fill)
            .style(move |theme, status| inspector_tab(theme, status, tab == value))
            .on_press(Message::OperationTabChanged(value));
            delayed_tooltip(
                button,
                container(text(value.hover_label()).size(12))
                    .padding(8)
                    .style(popup_surface),
                tooltip::Position::Top,
            )
            .into()
        };
        list = list.push(row(OperationTab::ALL.iter().copied().map(tab_button)).spacing(3));
        let header = list;
        let mut list = operation_sections::Form::new(
            self,
            tab,
            match tab {
                OperationTab::Tool => Some("Tool"),
                OperationTab::Geometry => Some("Geometry"),
                OperationTab::Passes => Some("Passes"),
                OperationTab::MultiAxis => Some("Multi-axis"),
                OperationTab::Heights | OperationTab::Linking => None,
            },
        );
        if tab == OperationTab::MultiAxis {
            list = list.push(
                text("Multi-axis machining controls are not available yet.")
                    .size(12)
                    .color(INSPECTOR_MUTED),
            );
        }
        if tab == OperationTab::Tool {
            let tool = self
                .operation_tools
                .iter()
                .chain(self.controller.inspection_setup().tools.iter())
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
            list = list.section("Cutting data").push(help_wrap(
                self.operation_preset_picker(op),
                "A matching material preset is applied when choosing a cutter. All cutting values can be edited below. RPM 0 inherits the job default. Cutting feeds are operation values.",
                self.tooltips,
            ));
        }
        if tab == OperationTab::Geometry && creating {
            list = list.push(self.creation_geometry_controls());
        }
        if tab == OperationTab::Geometry {
            if let Operation::Adaptive(adaptive) = op {
                list = list
                    .section("Geometry")
                    .push(text("Tool containment").size(12));
                list = list.push(
                    pick_list(
                        [
                            cam_model::AdaptiveContainment::Inside,
                            cam_model::AdaptiveContainment::Center,
                            cam_model::AdaptiveContainment::Outside,
                        ],
                        Some(adaptive.params.containment),
                        Message::AdaptiveContainmentChanged,
                    )
                    .placeholder("Choose containment")
                    .text_size(12)
                    .padding([7, 9])
                    .style(inspector_picker)
                    .width(Length::Fill),
                );
            }
        }
        if tab == OperationTab::Geometry && !creating {
            list = list.section("Geometry");
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
                Operation::Bore(o) => format!("{} bore centres", o.points.len()),
                Operation::Thread(o) => format!("{} thread centres", o.points.len()),
                Operation::Parallel(_) | Operation::Adaptive(_) => linked.map_or_else(
                    || "Selected geometry".into(),
                    |binding| {
                        let bodies = binding
                            .selected
                            .iter()
                            .filter(|entity| entity.kind == EntityKind::Body)
                            .count();
                        let faces = binding
                            .selected
                            .iter()
                            .filter(|entity| entity.kind == EntityKind::Face)
                            .count();
                        format!("{bodies} bodies · {faces} faces")
                    },
                ),
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
            list = list.push(self.operation_linking_controls(op, linked.is_some()));
        }
        for field in self.geometry_operation_fields(op) {
            if tab == OperationTab::Linking {
                continue; // Rendered once in the grouped linking sections.
            }
            if matches!(op, Operation::Thread(_))
                && tab == OperationTab::Geometry
                && field == Field::MajorDia
            {
                continue; // Grouped with pitch, hand and standard thread size.
            }
            if matches!(field, Field::StockLeaveRadial | Field::StockLeaveAxial) {
                continue; // These belong to the dedicated Stock to Leave section below.
            }
            if tab == OperationTab::Heights && field != Field::ThreadBlindAllowance {
                continue;
            }
            if linked.is_some()
                && matches!(op,Operation::Profile(p) if p.stepover==0.)
                && matches!(field, Field::Stepover | Field::Engagement)
            {
                continue;
            }
            if matches!(
                field,
                Field::Profile(_)
                    | Field::PocketFinish(_)
                    | Field::TabWidth
                    | Field::TabHeight
                    | Field::TabSpacing
                    | Field::StartX
                    | Field::StartY
            ) {
                continue; // Grouped beneath their section toggles.
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
                        | Field::BoreDiameter
                        | Field::BoreTaper
                )
            {
                continue;
            }
            let value = if creating
                && matches!(field, Field::MajorDia | Field::BoreDiameter)
                && !self.creation_edited_fields.contains(&field)
            {
                op_field(op, field).map(fmt_num).unwrap_or_default()
            } else {
                self.fields.get(&field).cloned().unwrap_or_default()
            };
            let section = operation_field_section(field, tab);
            list = list.section(section).push(field_row_labeled(
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
                    list = list.section("Geometry");
                    let comp = p.comp;
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
                    list = list.optional_section(
                        "Holding tabs",
                        p.tabs.enabled,
                        (p.tabs.enabled
                            || p.capabilities(
                                linked.is_some(),
                                self.controller.post_kind().supports_cutter_comp(),
                            )
                            .tabs)
                            .then_some(Message::ProfileTabsEnabled),
                    );
                    if !p.closed || !p.additional_chains.is_empty() {
                        list = list.push(
                            text("Holding tabs require a single closed contour.")
                                .size(11)
                                .color(INSPECTOR_MUTED),
                        );
                    }
                    if p.closed
                        && p.additional_chains.is_empty()
                        && !p
                            .capabilities(
                                linked.is_some(),
                                self.controller.post_kind().supports_cutter_comp(),
                            )
                            .tabs
                    {
                        list=list.push(text("Tabs need computed compensation, straight plunge, and zero roughing, overlap and taper.").size(11).color(INSPECTOR_MUTED));
                    }
                    if !p.closed {
                        list = list.push(text("Open chains support leads, ramps and tangent extensions with computed compensation. Ramps retrace their entry at depth; short chains need a smaller stepdown.").size(11).color(INSPECTOR_MUTED));
                    }
                    if p.tabs.enabled {
                        list = list.push(profile_picker("Tab shape","Rectangular tabs keep a flat top; triangular tabs rise and fall gradually.",self.tooltips,p.tabs.shape,&cam_model::TabShape::ALL[..],Message::ProfileTabShape));
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
                            if p.tabs.manual { profile_tabs::Positioning::Manual } else if p.tabs.count.is_some() { profile_tabs::Positioning::ByCount } else { profile_tabs::Positioning::ByDistance },
                            &profile_tabs::Positioning::ALL[..], Message::ProfileTabPositioning,
                        ));
                        if !p.tabs.manual {
                            let field = if p.tabs.count.is_some() {
                                Field::Profile(profile_parameters::Number::TabCount)
                            } else {
                                Field::TabSpacing
                            };
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
                if matches!(tab, OperationTab::Tool | OperationTab::Passes) {
                    list = list.extend(self.profile_extra_controls(p, tab, linked.is_some()));
                }
                if tab == OperationTab::Passes {
                    list = list
                        .section("Passes")
                        .push(profile_climb_control(p.clearing.climb, self.tooltips));
                }
            }
            Operation::Pocket(p) => {
                if tab == OperationTab::Passes {
                    list = list.extend(self.pocket_finishing_controls(&p.finishing));
                    list = list.section("Passes");
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
            }
            Operation::Face(f) if tab == OperationTab::Passes => {
                list = list.section("Passes").push(profile_picker(
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
                    list = list.section("Passes").push(text("Stepdown limits the height of each face band. 0 uses the cutter's usable flank automatically; smaller cutters use more passes.").size(12));
                }
            }
            Operation::Chamfer(c) => {
                if tab == OperationTab::Geometry {
                    list = list.section("Geometry").push(profile_picker(
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
                    list = list.section("Passes").push(field_row_labeled(
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
            }
            Operation::Bore(b) => {
                let toggle = |label: &'static str, value, option: fn(bool) -> BoreOption| {
                    row![
                        text(label).size(12),
                        checkbox(value)
                            .size(15)
                            .on_toggle(move |v| Message::BoreOption(option(v)))
                    ]
                    .spacing(8)
                };
                if tab == OperationTab::Geometry {
                    list = list.section("Geometry");
                    if linked.is_some() {
                        list = list.push(
                            text(format!(
                                "CAD {} · diameter {:.3} mm · straight wall",
                                if b.internal { "hole" } else { "boss" },
                                b.diameter
                            ))
                            .size(12),
                        );
                    } else {
                        list = list.push(toggle("Internal hole", b.internal, BoreOption::Internal));
                    }
                    list = list.push(pick_list(
                        &cam_model::BoreOrder::ALL[..],
                        Some(b.order),
                        |v| Message::BoreOption(BoreOption::Order(v)),
                    ));
                    list = list.push(toggle(
                        "Reverse order",
                        b.reverse_order,
                        BoreOption::Reverse,
                    ));
                }
                if tab == OperationTab::Passes {
                    list = list.section("Passes").push(toggle(
                        "Use ramp angle",
                        b.ramp_angle.is_some(),
                        BoreOption::Angle,
                    ));
                    list = list.section("Passes").push(toggle(
                        "Climb milling",
                        b.climb,
                        BoreOption::Climb,
                    ));
                    list = list.section("Passes").push(toggle(
                        "Repeat final pass",
                        b.repeat_pass,
                        BoreOption::Repeat,
                    ));
                    list = list.push(text("Tool compensation is computed in CAM.").size(12));
                }
            }
            Operation::Thread(t) => {
                if tab == OperationTab::Geometry {
                    list = list.section("Geometry").push(profile_picker(
                        "Bore",
                        help::BORE,
                        self.tooltips,
                        Bore::of(t.internal),
                        &Bore::ALL[..],
                        |b| Message::ThreadInternalChanged(b == Bore::Internal),
                    ));
                    list = list
                        .section("Geometry")
                        .push(self.thread_geometry_controls(t));
                }
                if tab == OperationTab::Passes {
                    list = list.section("Passes").push(profile_picker(
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
            Operation::Parallel(p) if tab == OperationTab::Passes => {
                list = list.section("Passes").push(
                    row![
                        text("Both ways (zigzag)").size(12),
                        checkbox(p.params.both_ways)
                            .size(15)
                            .on_toggle(Message::ParallelBothWaysToggled)
                    ]
                    .spacing(8),
                );
            }
            Operation::Parallel(p) if tab == OperationTab::Geometry => {
                list = list.push(profile_picker(
                    "Tool containment",
                    "Position of the cutter relative to the selected model bounding box.",
                    self.tooltips,
                    p.params.containment,
                    &[
                        cam_model::AdaptiveContainment::Inside,
                        cam_model::AdaptiveContainment::Center,
                        cam_model::AdaptiveContainment::Outside,
                    ][..],
                    Message::AdaptiveContainmentChanged,
                ));
            }
            Operation::Adaptive(adaptive) if tab == OperationTab::Passes => {
                list = list.section("Passes").push(
                    row![
                        label_help("Cut direction", help::CLIMB, self.tooltips),
                        checkbox(adaptive.params.climb)
                            .size(15)
                            .on_toggle(Message::AdaptiveClimbToggled)
                    ]
                    .spacing(8),
                );
            }
            Operation::Engrave(o) if tab == OperationTab::Geometry => {
                list = list.section("Geometry");
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
                if tab == OperationTab::Passes {
                    list = list.section("Passes");
                    if let Some(shape) = self
                        .controller
                        .inspection_setup()
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
                if matches!(tab, OperationTab::Tool | OperationTab::Passes) {
                    list = list.optional_section(
                        "Clearing pass (end mill)",
                        c.clear.is_some(),
                        Some(Message::CarveClearToggled),
                    );
                    if tab == OperationTab::Tool {
                        if let Some(cl) = &c.clear {
                            let choices: Vec<ToolChoice> = self
                                .library
                                .tools
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| {
                                    matches!(
                                        t.kind,
                                        ToolKind::EndMill
                                            | ToolKind::RoughingMill
                                            | ToolKind::BullNose { .. }
                                    )
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
                                .chain(self.controller.inspection_setup().tools.iter())
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
                                .padding([7, 9])
                                .style(inspector_picker)
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
                    }
                }
            }
            _ => {}
        }
        if tab == OperationTab::Passes && App::stock_to_leave_supported(op) {
            let stock = self.stock_to_leave_value();
            let enabled = self.stock_to_leave_enabled();
            list = list.optional_section(
                "Stock to Leave",
                enabled,
                Some(Message::StockToLeaveToggled),
            );
            if enabled {
                for field in stock_offset_fields(op) {
                    let fallback = match field {
                        Field::StockLeaveRadial => stock.radial,
                        _ => stock.axial,
                    };
                    let value = self
                        .fields
                        .get(&field)
                        .cloned()
                        .unwrap_or_else(|| fmt_num(fallback));
                    list = list.push(stock_offset_row(
                        field,
                        self.field_label(field),
                        self.field_help(field),
                        value,
                        self.tooltips,
                        self.field_invalid(field),
                    ));
                }
            }
        }
        if tab == OperationTab::Tool {
            list = list
                .section("Cutting forces")
                .push(self.cutting_force_panel(op));
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
            scrollable(container(list.finish()).padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 16.0,
                left: 14.0,
            }))
            .width(Length::Fill)
            .height(Length::Fill),
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
        if matches!(operation, Operation::Adaptive(_) | Operation::Parallel(_)) {
            cam_geometry3d::validate_adaptive_selection(
                self.controller.geometry_state(),
                &selected,
            )?;
        }
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
            .filter(|tool| used.contains(&tool.number))
            .cloned()
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

    #[test]
    fn pending_adaptive_operation_accepts_replacement_cutter() {
        if !crate::test_fixture::available("mill_change.ocam") {
            return;
        }
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller
            .open_project(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../testfiles/mill_change.ocam"
            ))
            .unwrap();
        app.setup_draft = None;
        app.focus_ops.clear();
        let _ = app.update(Message::ClickOp(0));
        let before = app.controller.inspection_setup().clone();
        let binding = app.controller.geometry_state().bindings[&0].clone();
        let mut tool = before.tools[0].clone();
        tool.flute_length = 25.0;
        tool.length = 55.0;
        app.library.tools = vec![tool];
        let _ = app.update(Message::OperationToolPicked(0));
        assert!(
            app.inspector_dirty(),
            "replacement cutter must enable Apply"
        );
        assert!(
            !app.any_field_invalid(),
            "{:?}",
            app.resolve_height_editor(0)
        );
        assert!(app.operation_height_overrides_valid());
        let number = app.current_operation(0).unwrap().tool();
        assert_ne!(
            number,
            before.operations[0].tool(),
            "picker must stage the chosen cutter"
        );
        let _ = app.update(Message::Apply);
        assert_eq!(
            app.controller.operation(0).unwrap().tool(),
            number,
            "{}",
            app.status
        );
        assert_eq!(
            app.controller
                .inspection_setup()
                .tools
                .iter()
                .find(|t| t.number == number)
                .unwrap()
                .flute_length,
            25.0
        );
        let applied = &app.controller.geometry_state().bindings[&0];
        assert_eq!(applied.heights, binding.heights);
        assert_eq!(applied.overrides, binding.overrides);
        assert!(!app.inspector_dirty());
        let mut next_tool = app.library.tools[0].clone();
        next_tool.diameter = 5.0;
        app.library.tools.push(next_tool);
        let _ = app.update(Message::OperationToolPicked(1));
        assert_ne!(app.current_operation(0).unwrap().tool(), number);
        assert!(app.inspector_dirty());
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.current_operation(0).unwrap().tool(), number);
        assert!(!app.inspector_dirty());
        assert!(app.controller.undo());
        assert_eq!(app.controller.inspection_setup(), &before);
    }

    #[test]
    fn stock_allowances_are_grouped_radial_then_axial_and_face_uses_axial_only() {
        let operations = operations();
        let profile = operations
            .iter()
            .find(|operation| matches!(operation, Operation::Profile(_)))
            .unwrap();
        let face = operations
            .iter()
            .find(|operation| matches!(operation, Operation::Face(_)))
            .unwrap();

        assert_eq!(
            stock_offset_fields(profile),
            [Field::StockLeaveRadial, Field::StockLeaveAxial]
        );
        assert_eq!(stock_offset_fields(face), [Field::StockLeaveAxial]);
    }

    #[test]
    fn bore_editor_exposes_fields_and_ramp_toggle_keeps_geometry() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.select(Selection::Setup);
        app.setup_draft = None;
        app.refresh_fields();
        let _ = app.update(Message::BeginOp(OpKind::Bore));
        assert!(matches!(app.operation_draft, Some(Operation::Bore(_))));
        let _ = app.update(Message::FieldChanged(Field::BorePitch, "0.75".into()));
        let _ = app.update(Message::BoreOption(BoreOption::Angle(true)));
        let operation = app.operation_draft.as_ref().unwrap();
        assert!(operation_fields(operation).contains(&Field::BoreAngle));
        assert!(!operation_fields(operation).contains(&Field::BorePitch));
        for field in operation_fields(operation) {
            assert!(op_field(operation, field).is_some(), "{field:?}");
        }
        let _ = app.update(Message::BoreOption(BoreOption::Repeat(true)));
        let _ = app.update(Message::BoreOption(BoreOption::Angle(false)));
        assert!(
            matches!(app.operation_draft, Some(Operation::Bore(ref b)) if b.pitch == 0.75 && b.repeat_pass && b.ramp_angle.is_none())
        );
        assert_eq!(field_tab(Field::BoreDiameter), OperationTab::Geometry);
        assert_eq!(field_tab(Field::BorePitch), OperationTab::Passes);
        assert_eq!(field_tab(Field::BoreSafeDistance), OperationTab::Linking);
    }

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
            .inspection_setup()
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
                    | Operation::Adaptive(_)
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
            .inspection_setup()
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
            .inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let committed = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Entry,
        ));
        let _ = app.update(Message::OperationPositionPicked([0., 0., 0.]));
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
                Operation::Bore(_) => ToolKind::EndMill,
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
            .inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.inspection_setup().clone();
        let _ = app.update(Message::OperationTabChanged(OperationTab::Tool));
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        let _ = app.update(Message::SideChanged(Side::Inside));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Passes));
        let _ = app.update(Message::ClearingClimbToggled(false));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Entry,
        ));
        let _ = app.update(Message::OperationPositionPicked([0., 0., 0.]));
        let _ = app.update(Message::FieldChanged(Field::StartX, "-4.5".into()));
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Profile(o)) if o.feed == 432.1 && o.side == Side::Inside && !o.clearing.climb && o.start == Some([-4.5, 0.0]))
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.inspection_setup(), &before);
    }

    #[test]
    fn compensation_availability_and_radial_passes_follow_post_and_strategy() {
        for post in [
            PostKind::Grbl,
            PostKind::FluidNc,
            PostKind::GrblHal,
            PostKind::Estlcam,
        ] {
            assert_eq!(Compensation::available(post), &[Compensation::Computed]);
        }
        for post in [
            PostKind::Fanuc,
            PostKind::Haas,
            PostKind::LinuxCnc,
            PostKind::Okuma,
        ] {
            assert_eq!(Compensation::available(post).len(), 2);
        }
        let mut operation = operations()
            .into_iter()
            .find(|o| matches!(o, Operation::Profile(_)))
            .unwrap();
        assert!(operation_fields(&operation).contains(&Field::Stepover));
        if let Operation::Profile(o) = &mut operation {
            o.comp = Comp::Control;
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
    fn position_picks_are_independent_transactional_and_serialized() {
        use operation_positions::PositionKind::{Entry, Exit};
        let (mut app, face_id) = linked_face_app();
        let reference = app.controller.geometry_state().bindings[&face_id].selected[0].clone();
        let id = app
            .controller
            .create_cad_operation(OpKind::Adaptive, vec![reference])
            .unwrap();
        app.begin_operation_draft(id);
        let saved = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        for (kind, point) in [(Entry, [2.0, 3.0, -1.0]), (Exit, [7.0, 8.0, -1.0])] {
            let _ = app.update(Message::OperationPositionPick(kind));
            let _ = app.update(Message::OperationPositionPicked(point));
            assert_eq!(app.operation_position_pick, None);
        }
        assert_eq!(app.fields[&Field::StartX], "2");
        assert_eq!(app.fields[&Field::StartY], "3");
        let draft = app.operation_draft.as_ref().unwrap();
        assert_eq!(operation_start(draft), Some([2.0, 3.0]));
        assert_eq!(operation_positions::operation_end(draft), Some([7.0, 8.0]));
        assert_eq!(app.controller.operation(id), Some(&saved));
        let _ = app.update(Message::Apply);
        let committed = app.controller.operation(id).unwrap().clone();
        let encoded = serde_json::to_value(&committed).unwrap();
        let restored: Operation = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(restored, committed);
        // Older projects have no entry/exit settings on adaptive parameters.
        let mut legacy = encoded;
        legacy["Adaptive"]["params"]
            .as_object_mut()
            .unwrap()
            .remove("start");
        legacy["Adaptive"]["params"]
            .as_object_mut()
            .unwrap()
            .remove("end");
        let restored: Operation = serde_json::from_value(legacy).unwrap();
        assert_eq!(operation_start(&restored), None);
        assert_eq!(operation_positions::operation_end(&restored), None);
        let _ = app.update(Message::OperationPositionClear(Exit));
        assert_eq!(
            operation_start(app.operation_draft.as_ref().unwrap()),
            Some([2.0, 3.0])
        );
        assert_eq!(
            operation_positions::operation_end(app.operation_draft.as_ref().unwrap()),
            None
        );
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.controller.operation(id), Some(&committed));
        app.begin_operation_draft(id);
        let _ = app.update(Message::OperationPositionPick(Entry));
        let _ = app.update(Message::CadEscape);
        assert_eq!(app.operation_position_pick, None);
        let _ = app.update(Message::OperationPositionPicked([99., 99., 0.]));
        assert_eq!(
            operation_start(app.operation_draft.as_ref().unwrap()),
            Some([2., 3.])
        );
    }

    #[test]
    fn lead_position_selection_initializes_editor_and_picks_the_surface() {
        use iced::mouse::{Button, Cursor, Event as Mouse};
        use operation_positions::PositionKind::{Entry, Exit};
        use shader::Program;

        let (mut app, face_id) = linked_face_app();
        let reference = app.controller.geometry_state().bindings[&face_id].selected[0].clone();
        let id = app
            .controller
            .create_cad_operation(OpKind::Adaptive, vec![reference])
            .unwrap();
        let mut geometry = app.controller.geometry_state().clone();
        Arc::make_mut(&mut geometry.assets[0].topology).mesh = cam_model::CadMesh {
            vertices: vec![
                [0., 0., -1.],
                [10., 0., -1.],
                [10., 10., -1.],
                [0., 10., -1.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            triangle_faces: vec![2, 2],
        };
        app.inspector_enabled = true;
        app.begin_operation_draft(face_id);
        app.focus_ops = BTreeSet::from([face_id]);
        // Browser selection leaves a saved operation without an edit draft.
        let _ = app.update(Message::ClickOp(id));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        app.show_project_browser = false;
        app.show_gizmo = false;
        app.view.yaw = 0.5;
        app.view.pitch = 0.6;
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800., 600.));
        let saved = app.controller.document().clone();
        assert!(
            app.operation_draft.is_none(),
            "browsing leaves a saved operation in the inspector"
        );
        for (kind, world) in [(Entry, [3., 4., -1.]), (Exit, [6., 7., -1.])] {
            let _ = app.update(Message::OperationPositionPick(kind));
            let viewport = app.machining_viewport();
            assert!(
                viewport.operation_position_picking,
                "Select must enable viewport picking"
            );
            let mut viewport = viewport;
            viewport.cad_scene = Arc::new(CadScene::from_geometry_in_setup(
                &geometry,
                &BTreeSet::new(),
                &app.controller.inspection_setup().frame,
            ));
            viewport.setup_snaps.clear();
            let (screen, _) = picking_view::project(viewport.camera(), bounds, world).unwrap();
            let mut state = ViewportState::default();
            let action = viewport
                .update(
                    &mut state,
                    &iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)),
                    bounds,
                    Cursor::Available(screen),
                )
                .unwrap();
            let (message, _, status) = action.into_inner();
            assert_eq!(status, iced::event::Status::Captured);
            let Some(Message::OperationPositionPicked(point)) = message else {
                panic!("model click must publish a lead position");
            };
            assert!((point[0] - world[0]).hypot(point[1] - world[1]) < 1e-5);
            let _ = app.update(Message::OperationPositionPicked(point));
            assert_eq!(app.operation_position_markers[kind.index()], Some(point));
            let preview = app.operation_geometry_preview();
            let marker = preview
                .strips
                .iter()
                .find(|strip| strip.color == kind.color())
                .unwrap();
            assert!(
                marker
                    .points
                    .iter()
                    .all(|point| (point[2] - world[2] as f32).abs() < 1e-5),
                "marker must remain on the selected surface"
            );
            assert_eq!(app.operation_position_pick, None);
            assert_eq!(
                app.controller.document(),
                &saved,
                "picks must stay in the draft"
            );
            if kind == Entry {
                let _ = app.update(Message::FieldChanged(Field::Feed, "321".into()));
            } else {
                assert_eq!(
                    app.fields[&Field::Feed],
                    "321",
                    "Select must preserve edited buffers"
                );
            }
        }
        assert!(operation_start(app.operation_draft.as_ref().unwrap()).is_some());
        assert!(
            operation_positions::operation_end(app.operation_draft.as_ref().unwrap()).is_some()
        );
    }

    #[test]
    fn lead_position_hover_and_click_use_the_origin_picker_snaps() {
        use iced::mouse::{Button, Cursor, Event as Mouse};
        use operation_positions::PositionKind::{Entry, Exit};
        use shader::Program;
        let (mut app, face_id) = linked_face_app();
        let reference = app.controller.geometry_state().bindings[&face_id].selected[0].clone();
        let id = app
            .controller
            .create_cad_operation(OpKind::Adaptive, vec![reference])
            .unwrap();
        app.begin_operation_draft(id);
        app.inspector_enabled = true;
        app.operation_tab = OperationTab::Linking;
        app.show_project_browser = false;
        app.show_gizmo = false;
        app.settings.snapping.enabled = true;
        app.snaps = vec![SnapKind::End, SnapKind::Mid];
        app.view.yaw = 0.5;
        app.view.pitch = 0.6;
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800., 600.));
        for (kind, target, snap_kind) in [
            (Entry, [0., 0., -1.], SnapKind::End),
            (Exit, [5., 0., -1.], SnapKind::Mid),
        ] {
            let _ = app.update(Message::OperationPositionPick(kind));
            let mut viewport = app.machining_viewport();
            viewport.snap_catch_px = 20.;
            let (screen, _) = picking_view::project(viewport.camera(), bounds, target).unwrap();
            let cursor = Cursor::Available(screen + iced::Vector::new(3., -2.));
            let mut state = ViewportState::default();
            let action = viewport
                .update(
                    &mut state,
                    &iced::Event::Mouse(Mouse::CursorMoved {
                        position: cursor.position().unwrap(),
                    }),
                    bounds,
                    cursor,
                )
                .unwrap();
            let (message, _, _) = action.into_inner();
            let Some(Message::SetupHover(position, hover)) = message else {
                panic!("position selection must use origin-style hover feedback");
            };
            let snap = hover
                .snap
                .as_ref()
                .expect("cursor must lock to a model point");
            assert_eq!(snap.kind, snap_kind);
            assert_eq!(snap.point, target);
            assert_eq!(viewport.origin_point_at(position, bounds), Some(target));
            viewport.setup_hover = hover.clone();
            assert_eq!(
                viewport.mouse_interaction(&state, bounds, cursor),
                iced::mouse::Interaction::default()
            );
            let primitive = viewport.draw(&state, cursor, bounds);
            let cross: Vec<_> = primitive
                .overlay_vertices
                .iter()
                .filter(|vertex| vertex.color == kind.color())
                .collect();
            assert_eq!(cross.len(), 4, "show a live point cross at the snap");
            for axis in 0..3 {
                let center = cross
                    .iter()
                    .map(|vertex| vertex.position[axis] as f64)
                    .sum::<f64>()
                    / cross.len() as f64;
                assert!((center - target[axis]).abs() < 1e-5);
            }
            let _ = app.update(Message::SetupHover(position, hover));
            let action = viewport
                .update(
                    &mut state,
                    &iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)),
                    bounds,
                    cursor,
                )
                .unwrap();
            let (message, _, status) = action.into_inner();
            assert_eq!(status, iced::event::Status::Captured);
            let Some(Message::OperationPositionPicked(point)) = message else {
                panic!("click must place a lead point");
            };
            assert_eq!(point, target, "hover and click must agree");
            let _ = app.update(Message::OperationPositionPicked(point));
            assert_eq!(app.operation_position_markers[kind.index()], Some(target));
        }
    }

    #[test]
    fn model_position_pick_hits_surface_in_oblique_view_and_converts_setup_axes() {
        for frame in [
            cam_model::SetupFrame::default(),
            cam_model::SetupFrame::from_xy([0., 1., 0.], [-1., 0., 0.]).unwrap(),
        ] {
            let (mut app, _) = linked_face_app();
            let mut setup = app.controller.inspection_setup().clone();
            setup.frame = frame.clone();
            app.controller.apply_setup_settings(setup).unwrap();
            let mut geometry = app.controller.geometry_state().clone();
            let topology = Arc::make_mut(&mut geometry.assets[0].topology);
            topology.mesh = cam_model::CadMesh {
                vertices: vec![
                    [0., 0., -1.],
                    [10., 0., -1.],
                    [10., 10., -1.],
                    [0., 10., -1.],
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                triangle_faces: vec![2, 2],
            };
            let mut viewport = app.machining_viewport();
            viewport.cad_scene = Arc::new(CadScene::from_geometry_in_setup(
                &geometry,
                &BTreeSet::new(),
                &frame,
            ));
            viewport.setup_snaps.clear();
            viewport.controls.yaw = 0.5;
            viewport.controls.pitch = 0.6;
            let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800., 600.));
            let world = [3., 4., -1.];
            let (screen, _) = picking_view::project(viewport.camera(), bounds, world).unwrap();
            let point = viewport.operation_position_at(screen, bounds).unwrap();
            let expected = frame.to_setup_vector(world);
            assert!(
                (0..3).all(|axis| (point[axis] - expected[axis]).abs() < 1e-5),
                "{point:?} vs {expected:?}"
            );
        }
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
            .inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.document().clone();
        let _ = app.update(Message::FieldChanged(Field::Feed, "432.1".into()));
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Entry,
        ));
        let _ = app.update(Message::OperationPositionPicked([0., 0., 0.]));
        let _ = app.update(Message::FieldChanged(Field::StartX, "invalid".into()));
        assert!(app.any_field_invalid());
        let _ = app.update(Message::OperationPositionClear(
            operation_positions::PositionKind::Entry,
        ));
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
            .inspection_setup()
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
    fn adaptive_creation_retains_containment_before_first_apply() {
        let (mut app, old_id) = linked_face_app();
        app.setup_draft = None;
        let reference = app.controller.geometry_state().bindings[&old_id].selected[0].clone();
        let _ = app.update(Message::BeginOp(OpKind::Adaptive));
        let _ = app.update(Message::CadSelect(reference));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        // Build the same inspector shown before the operation is committed.
        let _ = app.operation_inspector(0);
        let _ = app.update(Message::AdaptiveContainmentChanged(
            cam_model::AdaptiveContainment::Outside,
        ));
        let _ = app.update(Message::FieldChanged(Field::AdaptiveOffset, "1.25".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Entry,
        ));
        let _ = app.update(Message::OperationPositionPicked([4.0, 4.0, 0.]));
        let _ = app.update(Message::OperationPositionPick(
            operation_positions::PositionKind::Exit,
        ));
        let _ = app.update(Message::OperationPositionPicked([8.0, 8.0, 0.]));
        let (preview, _) = app.creation_operation().unwrap();
        assert!(matches!(preview, Operation::Adaptive(op)
            if op.params.containment == cam_model::AdaptiveContainment::Outside
                && op.params.additional_offset == 1.25
                && op.params.start == Some([4.0, 4.0])
                && op.params.end == Some([8.0, 8.0])));
        let _ = app.update(Message::ConfirmOp);
        assert!(!app.creating_operation(), "{}", app.status);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("new adaptive operation selected: {}", app.status);
        };
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Adaptive(op))
            if op.params.containment == cam_model::AdaptiveContainment::Outside
                && op.params.additional_offset == 1.25
                && op.params.start == Some([4.0, 4.0])
                && op.params.end == Some([8.0, 8.0]))
        );
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
            app.inspector_fields()
                .into_iter()
                .filter(|field| !matches!(field, Field::CuttingSpeed | Field::FeedPerTooth))
                .collect::<Vec<_>>(),
            operation_fields(app.operation_draft.as_ref().unwrap())
        );
        assert!(app.inspector_fields().contains(&Field::CuttingSpeed));
        assert!(app.inspector_fields().contains(&Field::FeedPerTooth));
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
            app.controller.inspection_setup().height_settings[&id]
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
            app.controller.inspection_setup().height_settings[&id]
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
    fn pocket_geometry_switches_between_floor_and_chain_height_defaults() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R};
        let (mut app, _) = linked_face_app();
        let _ = app.update(Message::BeginOp(OpKind::Pocket));
        let source = EntityRef {
            asset_id: 1,
            revision: 1,
            instance_id: 1,
            body_id: 1,
            kind: EntityKind::Face,
            id: 2,
        };
        let _ = app.update(Message::CadSelect(source.clone()));
        assert_eq!(
            app.height_draft
                .as_ref()
                .unwrap()
                .definition(P::Top)
                .reference,
            R::StockTop
        );
        assert_eq!(
            app.height_draft
                .as_ref()
                .unwrap()
                .definition(P::Bottom)
                .reference,
            R::SelectedContours
        );
        let _ = app.update(Message::ConfirmOp);
        assert!(!app.creating_operation(), "{}", app.status);
        let before = app.controller.document().clone();
        for (filter, entity, top, bottom) in [
            (
                CadFilter::Loop,
                EntityRef {
                    kind: EntityKind::Loop,
                    id: 5,
                    ..source.clone()
                },
                R::SelectedContours,
                R::Plane(P::Top),
            ),
            (CadFilter::Face, source, R::StockTop, R::SelectedContours),
        ] {
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Add(filter)));
            let _ = app.update(Message::CadSelect(entity));
            let _ = app.update(Message::OperationGeometry(GeometryMessage::Confirm));
            assert!(app.geometry_editor.is_none(), "{}", app.status);
            assert_eq!(
                app.height_draft
                    .as_ref()
                    .unwrap()
                    .definition(P::Top)
                    .reference,
                top
            );
            assert_eq!(
                app.height_draft
                    .as_ref()
                    .unwrap()
                    .definition(P::Bottom)
                    .reference,
                bottom
            );
            assert_eq!(app.controller.document(), &before);
        }
    }

    #[test]
    fn pocket_chain_settings_accumulate_edges_and_preserve_height_edits() {
        let (mut app, _) = linked_face_app();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Pocket));
        let _ = app.update(Message::OperationGeometry(GeometryMessage::Add(
            CadFilter::Loop,
        )));
        let _ = app.inspector();
        let _ = app.update(Message::GeometryFilterChanged(CadFilter::Edge));
        let _ = app.update(Message::HeightOffsetChanged(
            cam_model::HeightPlaneKind::Bottom,
            "-1.5".into(),
        ));
        for id in 20..24 {
            let _ = app.update(Message::CadSelect(EntityRef {
                asset_id: 1,
                revision: 1,
                instance_id: 1,
                body_id: 1,
                kind: EntityKind::Edge,
                id,
            }));
            assert_eq!(app.controller.cad_selection().len(), (id - 19) as usize);
            assert_eq!(
                app.controller
                    .preview_cad_operation(OpKind::Pocket, app.controller.cad_selection())
                    .is_ok(),
                id == 23
            );
        }
        assert_eq!(
            app.height_offsets[&cam_model::HeightPlaneKind::Bottom],
            "-1.5"
        );
        let _ = app.update(Message::OperationGeometry(GeometryMessage::Confirm));
        assert!(app.geometry_editor.is_none(), "{}", app.status);
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::ConfirmOp);
        assert!(!app.creating_operation(), "{}", app.status);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("pocket selected")
        };
        let heights = app.controller.inspection_setup().height_settings[&id]
            .resolve(
                app.controller.document(),
                app.controller.operation(id).unwrap(),
            )
            .unwrap();
        assert_eq!(heights.top, -1.);
        assert_eq!(heights.bottom, -2.5);
    }

    #[test]
    fn cad_surface_geometry_creates_clears_and_switches_to_an_open_engraving() {
        for kind in [
            OpKind::Pocket,
            OpKind::Chamfer,
            OpKind::Engrave,
            OpKind::Carve,
        ] {
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
        for kind in [
            OpKind::Pocket,
            OpKind::Chamfer,
            OpKind::Engrave,
            OpKind::Carve,
        ] {
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
            assert_eq!(
                app.controller.cad_selection(),
                std::slice::from_ref(&source)
            );
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
