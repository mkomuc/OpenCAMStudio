//! Simulation display preferences and the simulation inspector.
use super::*;
use crate::settings::SimulationMaterial;
use cam_cldata::{MoveKind, Tag};
use iced::widget::column;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::gui) enum SimulationTab {
    #[default]
    Display,
    Information,
    Statistics,
}

#[derive(Clone, Debug)]
pub(in crate::gui) enum SimulationOption {
    Tool(bool),
    ToolDisplay(SimulationToolDisplay),
    TransparentTool(bool),
    ProgrammedPoint(bool),
    Toolpath(bool),
    Links(bool),
    Leads(bool),
    Cutting(bool),
    Points(bool),
    Mode(SimulationToolpathMode),
    Stock(bool),
    Coloring(SimulationStockColoring),
    Material(SimulationMaterial),
    TransparentStock(bool),
    Regenerate(bool),
    StopOnCollision(bool),
    Model(bool),
    Tolerance(f32),
}
impl SimulationOption {
    pub(in crate::gui) fn apply(self, prefs: &mut SimulationPrefs) {
        match self {
            Self::Tool(v) => prefs.show_tool = v,
            Self::ToolDisplay(v) => prefs.tool_display = v,
            Self::TransparentTool(v) => prefs.transparent_tool = v,
            Self::ProgrammedPoint(v) => prefs.programmed_point = v,
            Self::Toolpath(v) => prefs.show_toolpath = v,
            Self::Links(v) => prefs.show_links = v,
            Self::Leads(v) => prefs.show_leads = v,
            Self::Cutting(v) => prefs.show_cutting = v,
            Self::Points(v) => prefs.show_points = v,
            Self::Mode(v) => prefs.toolpath_mode = v,
            Self::Stock(v) => prefs.show_stock = v,
            Self::Coloring(v) => prefs.stock_coloring = v,
            Self::Material(v) => prefs.material = v,
            Self::TransparentStock(v) => prefs.transparent_stock = v,
            Self::Regenerate(v) => prefs.regenerate_on_rewind = v,
            Self::StopOnCollision(v) => prefs.stop_on_collision = v,
            Self::Model(v) => prefs.show_model = v,
            Self::Tolerance(v) => prefs.comparison_tolerance = v.clamp(0.001, 2.),
        }
    }
}

fn motion_tag(step: &Step) -> Option<Tag> {
    match step {
        Step::Rapid { tag, .. } | Step::Linear { tag, .. } | Step::Arc { tag, .. } => Some(*tag),
        _ => None,
    }
}
fn endpoint(step: &Step) -> Option<Point3> {
    match step {
        Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
        Step::Arc { end, .. } => Some(*end),
        _ => None,
    }
}
impl TimedStep {
    fn at(&self, fraction: f64) -> Step {
        let mut step = self.step.clone();
        if let Some(a) = self.from {
            match &mut step {
                Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                    *to = Point3::new(
                        a.x + (to.x - a.x) * fraction,
                        a.y + (to.y - a.y) * fraction,
                        a.z + (to.z - a.z) * fraction,
                    );
                }
                Step::Arc {
                    end, center, dir, ..
                } => {
                    let angle = (a.y - center.y).atan2(a.x - center.x)
                        + arc_sweep(a, *end, *center, *dir) * fraction;
                    let radius = (a.x - center.x).hypot(a.y - center.y);
                    *end = Point3::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        a.z + (end.z - a.z) * fraction,
                    );
                }
                _ => {}
            }
        }
        step
    }
    fn length(&self) -> f64 {
        match (&self.step, self.from) {
            (
                Step::Arc {
                    end, center, dir, ..
                },
                Some(a),
            ) => {
                let radius = (a.x - center.x).hypot(a.y - center.y);
                (radius * arc_sweep(a, *end, *center, *dir)).hypot(end.z - a.z)
            }
            (_, Some(a)) => endpoint(&self.step).map_or(0., |b| distance(a, b)),
            _ => 0.,
        }
    }
}

impl SimulationPlayer {
    pub(in crate::gui) fn configure(&mut self, prefs: &SimulationPrefs, custom: [f32; 3]) {
        let rebuild = prefs.accuracy != self.inputs.simulation_accuracy
            || (prefs.regenerate_on_rewind && !self.options.regenerate_on_rewind);
        let comparison_changed = prefs.comparison_tolerance != self.options.comparison_tolerance
            || prefs.stock_coloring != self.options.stock_coloring;
        if prefs.stop_on_collision != self.options.stop_on_collision {
            self.collision = None;
            self.acknowledged_step = None;
            self.collision_floor = self.rendered;
        }
        if prefs.comparison_tolerance != self.options.comparison_tolerance {
            self.comparison.invalidate();
            self.comparison_classes.clear();
        }
        if prefs.stock_coloring != SimulationStockColoring::Comparison {
            self.comparison.invalidate();
        }
        self.options = prefs.clone();
        self.custom_stock_color = custom;
        if rebuild {
            self.inputs.simulation_accuracy = prefs.accuracy;
            let full = self.timeline.prefix(self.timeline.duration);
            self.inputs.preview_resolution = Some(self.inputs.stock_resolution(&full));
            self.stock_position = self.position;
            self.request_frame();
        } else if comparison_changed {
            if let Some(frame) = self.pending_frame.take() {
                self.prepare_frame(frame);
            }
        }
        self.refresh_display();
    }

    fn active_operation(&self) -> Option<u32> {
        self.timeline
            .steps
            .iter()
            .find(|s| {
                s.start <= self.rendered && s.end > self.rendered && motion_tag(&s.step).is_some()
            })
            .and_then(|s| motion_tag(&s.step))
            .map(|t| t.op_id)
            .or_else(|| {
                self.preview
                    .program
                    .steps
                    .iter()
                    .rev()
                    .find_map(motion_tag)
                    .map(|t| t.op_id)
            })
    }

    fn path_scene(&self) -> Scene {
        let prefs = &self.options;
        let mut scene = Scene::new();
        if !prefs.show_toolpath {
            return scene;
        }
        let (mut start, end) = match prefs.toolpath_mode {
            SimulationToolpathMode::Trailing | SimulationToolpathMode::Tail => (0., self.rendered),
            SimulationToolpathMode::Leading => (self.rendered, self.timeline.duration),
            _ => (0., self.timeline.duration),
        };
        if prefs.toolpath_mode == SimulationToolpathMode::Tail {
            let mut remaining = 20.; // A short, fixed 20 mm trail behind the tip.
            for step in self.timeline.steps.iter().rev().filter(|s| s.start < end) {
                let length = step.length();
                if length <= 0. || step.end <= step.start {
                    continue;
                }
                let fraction =
                    ((end.min(step.end) - step.start) / (step.end - step.start)).clamp(0., 1.);
                let available = length * fraction;
                if length > 0. && available >= remaining {
                    start = step.start + (fraction - remaining / length) * (step.end - step.start);
                    break;
                }
                remaining -= available;
            }
        }
        let active = self.active_operation();
        let frame = &self.inputs.document.setup().frame;
        let mut run = Program::new();
        let flush = |run: &mut Program, scene: &mut Scene| {
            let mut part = Scene::from_program(run);
            scene.strips.append(&mut part.strips);
            scene.path_cones.append(&mut part.path_cones);
            run.steps.clear();
        };
        let mut previous_end = None;
        for step in &self.timeline.steps {
            let Some(tag) = motion_tag(&step.step) else {
                continue;
            };
            let visible = match tag.kind {
                MoveKind::Link | MoveKind::Traverse => prefs.show_links,
                MoveKind::Cutting => prefs.show_cutting,
                _ => prefs.show_leads,
            } && (prefs.toolpath_mode != SimulationToolpathMode::Operation
                || active == Some(tag.op_id));
            let lo = start.max(step.start);
            let hi = end.min(step.end);
            if !visible || hi <= lo || step.from.is_none() {
                flush(&mut run, &mut scene);
                previous_end = None;
                continue;
            }
            let duration = step.end - step.start;
            let a = endpoint(&step.at((lo - step.start) / duration)).unwrap();
            let clipped = step.at((hi - step.start) / duration);
            if previous_end != Some(a) {
                flush(&mut run, &mut scene);
                run.steps.push(Step::Rapid {
                    to: a,
                    tag: Tag {
                        kind: MoveKind::Link,
                        ..tag
                    },
                });
            }
            previous_end = endpoint(&clipped);
            run.steps.push(clipped);
        }
        flush(&mut run, &mut scene);
        // Points describe the actual displayed motion vertices, including clipped endpoints.
        if prefs.show_points {
            let points: Vec<_> = scene
                .strips
                .iter()
                .flat_map(|s| s.points.iter().map(move |p| (*p, s.op)))
                .collect();
            for (point, op) in points {
                add_point(&mut scene, point, op, [0.9, 0.9, 1., 1.], 0.12);
            }
        }
        for strip in &mut scene.strips {
            for point in &mut strip.points {
                *point = frame
                    .to_world_vector(point.map(f64::from))
                    .map(|n| n as f32);
            }
        }
        for cone in &mut scene.path_cones {
            cone.at = frame
                .to_world_vector(cone.at.map(f64::from))
                .map(|n| n as f32);
            cone.direction = frame
                .to_world_vector(cone.direction.map(f64::from))
                .map(|n| n as f32);
        }
        scene
    }

    pub(super) fn refresh_display(&mut self) {
        let mut scene = self.path_scene();
        // Keep the imported drawing independently of the toolpath's display mode.
        if self.options.show_model {
            for region in self.inputs.regions.iter() {
                scene.add_region(region, PART);
            }
            for path in self.inputs.open_paths.iter() {
                scene.add_open_path(path.points(), PART);
            }
        }
        if self.options.show_tool && self.options.programmed_point {
            if let Some(tip) = self.preview.program.steps.iter().rev().find_map(endpoint) {
                let p = self
                    .inputs
                    .document
                    .setup()
                    .frame
                    .to_world_vector([tip.x, tip.y, tip.z]);
                add_point(
                    &mut scene,
                    p.map(|v| v as f32),
                    None,
                    [1., 0.25, 0.85, 1.],
                    0.5,
                );
            }
        }
        self.preview.scene = scene;
        if self.options.stock_coloring == SimulationStockColoring::Comparison
            && self.pending_frame.is_none()
            && self.comparison_classes.len() != self.preview.stock_vertices.len()
        {
            if let Some(classes) = self.comparison.request(
                &self.target,
                &self.preview.stock_vertices,
                f64::from(self.options.comparison_tolerance),
            ) {
                self.comparison_classes = classes;
            }
        }
        let material = self.options.material.color(self.custom_stock_color);
        let mut tools = BTreeMap::new();
        let mut tool = 0;
        for step in &self.timeline.steps {
            if let Step::ToolChange { tool: number } = step.step {
                tool = number;
            }
            if let Some(tag) = motion_tag(&step.step) {
                tools.insert(tag.op_id, tool);
            }
        }
        // A pending comparison must not replace a complete frame with gray
        // placeholders, including while changing the tolerance or display.
        let retain_colors = self.options.stock_coloring == SimulationStockColoring::Comparison
            && (self.pending_frame.is_some()
                || (self.comparison.pending()
                    && self.comparison_classes.len() != self.preview.stock_vertices.len()));
        for (index, vertex) in self.preview.stock_vertices.iter_mut().enumerate() {
            if retain_colors {
                continue;
            }
            let owner = self.preview.stock_operations.get(index).copied().flatten();
            vertex.color = match self.options.stock_coloring {
                SimulationStockColoring::Material => material,
                SimulationStockColoring::Operation => owner.map_or(material, identity_color),
                SimulationStockColoring::Tool => owner
                    .and_then(|id| tools.get(&id))
                    .copied()
                    .map_or(material, identity_color),
                SimulationStockColoring::Comparison => {
                    match self.comparison_classes.get(index).copied().flatten() {
                        Some(cam_render::SurfaceClass::Excess) => [0.2, 0.4, 1., 1.],
                        Some(cam_render::SurfaceClass::WithinTolerance) => [0.15, 0.85, 0.2, 1.],
                        Some(cam_render::SurfaceClass::Gouge) => [1., 0.1, 0.1, 1.],
                        None => [0.5, 0.5, 0.5, 1.],
                    }
                }
            };
        }
    }

    pub(in crate::gui) fn menu(&self) -> Element<'_, Message> {
        let tab = |label: &str, value| {
            let active = self.tab == value;
            inspector_button(label)
                .on_press(Message::SimulationTab(value))
                .style(move |theme, status| inspector_tab(theme, status, active))
        };
        let tabs = row![
            tab("Display", SimulationTab::Display),
            tab("Info", SimulationTab::Information),
            tab("Statistics", SimulationTab::Statistics),
        ]
        .spacing(4)
        .wrap();
        let content: Element<'_, Message> = match self.tab {
            SimulationTab::Display => self.display_menu(),
            SimulationTab::Information => {
                let point = self.preview.program.steps.iter().rev().find_map(endpoint);
                let tool = self.preview.program.steps.iter().rev().find_map(|s| match s { Step::ToolChange { tool } => Some(*tool), _ => None });
                let mut info = column![inspector_section("Current frame"),
                    text(format!("Operation: {}", self.active_operation().map_or("—".into(), |v| format!("#{v}")))).size(12),
                    text(format!("Tool: {}", tool.map_or("—".into(), |v| v.to_string()))).size(12),
                ].spacing(10);
                if let Some(p) = point { info = info.push(text(format!("X {:.3} mm\nY {:.3} mm\nZ {:.3} mm", p.x, p.y, p.z)).size(12)); }
                if let Some(c) = &self.collision { info = info.push(text(c.message.clone()).size(12).color(Color::from_rgb(1., 0.3, 0.2))); }
                info.into()
            }
            SimulationTab::Statistics => column![inspector_section("Playback"),
                text(format!("Duration: {:.2} s\nRendered: {:.2} s\nMotions: {}\nStock triangles: {}\nStock grid: {:.3} mm",
                    self.timeline.duration, self.rendered,
                    self.timeline.steps.iter().filter(|s| motion_tag(&s.step).is_some()).count(),
                    self.preview.stock_indices.len()/3, self.inputs.preview_resolution.unwrap_or(0.))).size(12),
            ].spacing(10).into(),
        };
        column![
            inspector_heading("Simulation"),
            tabs,
            scrollable(container(content).padding(Padding {
                top: 4.0,
                right: 18.0,
                bottom: 4.0,
                left: 0.0,
            }))
            .height(Length::Fill),
            inspector_button("Close simulation")
                .on_press(Message::SimulationClose)
                .width(Length::Fill),
        ]
        .spacing(10)
        .padding(12)
        .height(Length::Fill)
        .into()
    }

    fn display_menu(&self) -> Element<'_, Message> {
        let p = &self.options;
        let section = |i: usize, label: &str| {
            inspector_button(format!(
                "{} {label}",
                if self.sections[i] { "▾" } else { "▸" }
            ))
            .on_press(Message::SimulationSection(i))
            .width(Length::Fill)
        };
        let mut list = column![section(0, "Tool")].spacing(10);
        if self.sections[0] {
            list = list.push(check("Show tool", p.show_tool, SimulationOption::Tool));
            if p.show_tool {
                list = list
                    .push(picker(
                        "Tool display",
                        SimulationToolDisplay::ALL,
                        p.tool_display,
                        SimulationOption::ToolDisplay,
                    ))
                    .push(check(
                        "Transparent",
                        p.transparent_tool,
                        SimulationOption::TransparentTool,
                    ))
                    .push(check(
                        "Programmed point",
                        p.programmed_point,
                        SimulationOption::ProgrammedPoint,
                    ));
            }
        }
        list = list
            .push(popup_separator::<Message>())
            .push(section(1, "Toolpath"));
        if self.sections[1] {
            list = list.push(check(
                "Show toolpath",
                p.show_toolpath,
                SimulationOption::Toolpath,
            ));
            if p.show_toolpath {
                list = list
                    .push(check("Show links", p.show_links, SimulationOption::Links))
                    .push(check(
                        "Show leads and retracts",
                        p.show_leads,
                        SimulationOption::Leads,
                    ))
                    .push(check(
                        "Show cutting moves",
                        p.show_cutting,
                        SimulationOption::Cutting,
                    ))
                    .push(check(
                        "Show points",
                        p.show_points,
                        SimulationOption::Points,
                    ))
                    .push(picker(
                        "Mode",
                        SimulationToolpathMode::ALL,
                        p.toolpath_mode,
                        SimulationOption::Mode,
                    ));
            }
        }
        list = list
            .push(popup_separator::<Message>())
            .push(section(2, "Stock"));
        if self.sections[2] {
            list = list.push(check("Show stock", p.show_stock, SimulationOption::Stock));
            if p.show_stock {
                list = list
                    .push(picker(
                        "Coloring",
                        SimulationStockColoring::ALL,
                        p.stock_coloring,
                        SimulationOption::Coloring,
                    ))
                    .push(picker(
                        "Material",
                        SimulationMaterial::ALL,
                        p.material,
                        SimulationOption::Material,
                    ))
                    .push(text(format!("Accuracy  {:.2}×", p.accuracy)).size(12))
                    .push(
                        slider(
                            crate::SIMULATION_ACCURACY_RANGE.0..=crate::SIMULATION_ACCURACY_RANGE.1,
                            p.accuracy,
                            Message::SetSimulationAccuracy,
                        )
                        .step(0.25_f32)
                        .on_release(Message::SimulationAccuracySettled),
                    )
                    .push(check(
                        "Transparent",
                        p.transparent_stock,
                        SimulationOption::TransparentStock,
                    ));
                if p.stock_coloring == SimulationStockColoring::Comparison {
                    list = list
                        .push(text(format!("Tolerance  {:.3} mm", p.comparison_tolerance)).size(12))
                        .push(
                            slider(0.001..=2., p.comparison_tolerance, |v| {
                                Message::SimulationOption(SimulationOption::Tolerance(v))
                            })
                            .step(0.001_f32)
                            .on_release(Message::SimulationSaveSettings),
                        );
                }
            }
            list = list
                .push(check(
                    "Regenerate stock on rewind",
                    p.regenerate_on_rewind,
                    SimulationOption::Regenerate,
                ))
                .push(check(
                    "Stop on collision",
                    p.stop_on_collision,
                    SimulationOption::StopOnCollision,
                ));
        }
        list = list
            .push(popup_separator::<Message>())
            .push(section(3, "Display"));
        if self.sections[3] {
            list = list.push(check("Show model", p.show_model, SimulationOption::Model));
        }
        list.into()
    }
}

fn identity_color(id: u32) -> [f32; 4] {
    // Golden-angle hues keep adjacent identities apart without repeating a palette.
    let hue = ((f64::from(id) * 0.618_033_988_75 + 0.55).fract() * 6.) as f32;
    let x = 0.95 * (1. - (hue % 2. - 1.).abs());
    let [r, g, b] = match hue as u32 {
        0 => [0.95, x, 0.],
        1 => [x, 0.95, 0.],
        2 => [0., 0.95, x],
        3 => [0., x, 0.95],
        4 => [x, 0., 0.95],
        _ => [0.95, 0., x],
    };
    [0.2 + r * 0.8, 0.2 + g * 0.8, 0.2 + b * 0.8, 1.]
}
fn add_point(scene: &mut Scene, point: [f32; 3], op: Option<u32>, color: [f32; 4], size: f32) {
    for axis in 0..3 {
        let mut a = point;
        let mut b = point;
        a[axis] -= size;
        b[axis] += size;
        scene.add_strip_op(vec![a, b], color, op);
    }
}
fn check(
    label: &str,
    value: bool,
    option: fn(bool) -> SimulationOption,
) -> Element<'static, Message> {
    row![
        text(label.to_owned()).size(12).width(Length::Fill),
        checkbox(value)
            .size(14)
            .on_toggle(move |v| Message::SimulationOption(option(v))),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
fn picker<T: Clone + Copy + PartialEq + std::fmt::Display + 'static>(
    label: &str,
    values: &'static [T],
    selected: T,
    option: fn(T) -> SimulationOption,
) -> Element<'static, Message> {
    column![
        text(label.to_owned()).size(12),
        pick_list(values, Some(selected), move |v| Message::SimulationOption(
            option(v)
        ))
        .text_size(12)
        .padding([7, 9])
        .style(inspector_picker)
        .width(Length::Fill),
    ]
    .spacing(4)
    .into()
}
