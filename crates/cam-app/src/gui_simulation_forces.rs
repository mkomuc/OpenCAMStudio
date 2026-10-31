//! Actual-removal force analysis has its own cancellable lane. It never replaces
//! the verified job or playback stock, and seeking cannot alter the load trace.
use super::options::{endpoint, motion_tag};
use super::*;
use cam_cldata::{MoveKind, Tag};
use cam_model::CuttingLoad;
use cam_sim::RemovalSample;
use iced::widget::column;
use std::ops::Range;
#[path = "gui_force_legend.rs"]
mod legend;

const UNKNOWN: [f32; 4] = [0.55, 0.57, 0.62, 1.];

#[derive(Default)]
struct OperationLoad {
    volume: f64,
    seconds: f64,
    force_integral: f64,
    max_force: f64,
    unknown: usize,
}

pub(super) struct ForceTrace {
    samples: Arc<Vec<RemovalSample>>,
    loads: Vec<Option<CuttingLoad>>,
    ranges: Vec<Range<usize>>,
    operations: BTreeMap<u32, OperationLoad>,
    max_force: f64,
    has_collision: bool,
}

fn cutting_tag(tag: Tag) -> bool {
    !matches!(
        tag.kind,
        MoveKind::Link | MoveKind::Retract | MoveKind::Traverse
    )
}

impl ForceTrace {
    fn load_at(&self, source: cam_sim::SurfaceLocation) -> Option<CuttingLoad> {
        let range = self.ranges.get(source.step_index)?.clone();
        let local = self.samples[range.clone()]
            .partition_point(|s| s.end_fraction < source.fraction - 1e-9);
        (range.start + local < range.end)
            .then(|| self.loads[range.start + local])
            .flatten()
    }
    fn new(
        samples: Arc<Vec<RemovalSample>>,
        timeline: &Timeline,
        inputs: &JobInputs,
        kc: Option<f64>,
    ) -> Self {
        let mut trace = Self {
            ranges: vec![0..0; timeline.steps.len()],
            loads: Vec::with_capacity(samples.len()),
            samples,
            operations: BTreeMap::new(),
            max_force: 0.,
            has_collision: false,
        };
        let milling: BTreeSet<_> = inputs
            .document
            .setup()
            .operations
            .iter()
            .filter(|op| !matches!(op, Operation::Drill(_)))
            .map(Operation::id)
            .collect();
        let tools: BTreeMap<_, _> = inputs
            .document
            .setup()
            .tools
            .iter()
            .map(|tool| (tool.number, tool))
            .collect();
        for (index, sample) in trace.samples.iter().enumerate() {
            trace.has_collision |= sample.collision;
            let range = &mut trace.ranges[sample.step_index];
            if range.start >= range.end {
                range.start = index;
            }
            range.end = index + 1;
            let tag =
                motion_tag(&timeline.steps[sample.step_index].step).filter(|tag| cutting_tag(*tag));
            let tool = sample.tool.and_then(|number| tools.get(&number).copied());
            let load = kc.zip(tool).and_then(|(kc, tool)| {
                if sample.collision
                    || sample.seconds <= 0.
                    || tag.is_none_or(|tag| !milling.contains(&tag.op_id))
                    || matches!(tool.kind, ToolKind::Drill { .. })
                {
                    return None;
                }
                CuttingLoad::from_removal_rate(
                    sample.removed_volume_mm3 / sample.seconds * 60.,
                    kc,
                    sample.spindle_rpm,
                    tool.diameter,
                )
                .ok()
            });
            if let Some(tag) = tag {
                let stats = trace.operations.entry(tag.op_id).or_default();
                stats.volume += sample.removed_volume_mm3;
                if let Some(load) = load {
                    stats.seconds += sample.seconds;
                    stats.force_integral += load.tangential_force_n * sample.seconds;
                    stats.max_force = stats.max_force.max(load.tangential_force_n);
                    trace.max_force = trace.max_force.max(load.tangential_force_n);
                } else {
                    stats.unknown += 1;
                }
            }
            trace.loads.push(load);
        }
        trace
    }
}

/// One fixed scale for the whole analyzed program, including unseen leading moves.
fn force_color(force: f64, max: f64) -> [f32; 4] {
    let stops = [
        [0.15, 0.4, 1.],
        [0., 0.8, 0.85],
        [1., 0.85, 0.15],
        [0.95, 0.15, 0.12],
    ];
    let t = if max > 0. {
        (force / max).clamp(0., 1.) * 3.
    } else {
        0.
    };
    let index = (t.floor() as usize).min(2);
    let fraction = (t - index as f64) as f32;
    let rgb: [f32; 3] = std::array::from_fn(|axis| {
        stops[index][axis] * (1. - fraction) + stops[index + 1][axis] * fraction
    });
    [rgb[0], rgb[1], rgb[2], 1.]
}

pub(super) fn stock_surface_color(
    trace: Option<&ForceTrace>,
    source: Option<cam_sim::SurfaceLocation>,
) -> [f32; 4] {
    let Some(trace) = trace else { return UNKNOWN };
    source
        .and_then(|source| trace.load_at(source))
        .map_or(UNKNOWN, |load| {
            force_color(load.tangential_force_n, trace.max_force)
        })
}

impl SimulationPlayer {
    pub(super) fn forces_enabled(&self) -> bool {
        self.options.toolpath_coloring == SimulationToolpathColoring::CuttingForce
            || self.options.stock_coloring == SimulationStockColoring::CuttingForce
    }

    pub(in crate::gui) fn force_viewport_legend(
        &self,
        reserved_left: f32,
        gizmo_size: Option<f32>,
    ) -> Option<Element<'_, Message>> {
        let visible = (self.options.show_stock
            && self.options.stock_coloring == SimulationStockColoring::CuttingForce)
            || (self.options.show_toolpath
                && self.options.toolpath_coloring == SimulationToolpathColoring::CuttingForce);
        if !visible {
            return None;
        }
        let maximum = self
            .force_trace
            .as_ref()
            .filter(|trace| trace.operations.values().any(|s| s.seconds > 0.))
            .map(|trace| trace.max_force);
        Some(
            canvas::Canvas::new(legend::ForceLegend {
                maximum,
                reserved_left,
                gizmo_size,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }
    pub(in crate::gui) fn sync_force_material(&mut self, library: &ToolLibrary) {
        let material = self
            .inputs
            .document
            .setup()
            .material_id
            .and_then(|id| library.materials.iter().find(|m| m.id == id))
            .cloned();
        if self.force_material == material {
            return;
        }
        self.force_material = material;
        if let Some(trace) = self.force_trace.take() {
            self.force_trace = Some(ForceTrace::new(
                trace.samples,
                &self.timeline,
                &self.inputs,
                self.force_material
                    .as_ref()
                    .and_then(|m| m.specific_cutting_force),
            ));
        }
        if self.forces_enabled() {
            self.refresh_display();
        }
    }

    pub(super) fn configure_force_analysis(&mut self, rebuild: bool) {
        if rebuild {
            self.force_jobs.cancel();
            self.force_trace = None;
            self.force_error = None;
        }
        if !self.forces_enabled() {
            self.force_jobs.cancel();
            self.force_error = None;
            return;
        }
        if self.force_trace.is_none() && self.force_error.is_none() && !self.force_jobs.pending() {
            let mut inputs = self.inputs.clone();
            inputs.preview_program = Some(self.timeline.prefix(self.timeline.duration));
            inputs.removal_analysis = true;
            self.force_jobs.start(
                self.source.revision,
                format!("removal:{:?}", inputs.preview_resolution),
                inputs,
                Default::default(),
            );
        }
    }

    pub(super) fn poll_forces(&mut self) {
        if let Some(result) = self.force_jobs.poll(self.source.revision) {
            if result.outcome.completion == JobCompletion::Complete {
                self.force_trace = Some(ForceTrace::new(
                    result.outcome.removal_samples,
                    &self.timeline,
                    &self.inputs,
                    self.force_material
                        .as_ref()
                        .and_then(|m| m.specific_cutting_force),
                ));
            } else {
                self.force_error = Some(
                    result
                        .outcome
                        .diagnostics
                        .first()
                        .map(|d| d.message.clone())
                        .unwrap_or_else(|| {
                            "Force analysis failed. Switch coloring off and on to retry.".into()
                        }),
                );
            }
            self.refresh_display();
        }
    }

    pub(super) fn add_force_motion(
        &self,
        scene: &mut Scene,
        index: usize,
        step: &TimedStep,
        lo: f64,
        hi: f64,
        tag: Tag,
    ) {
        let duration = step.end - step.start;
        let lo = (lo - step.start) / duration;
        let hi = (hi - step.start) / duration;
        let range = self
            .force_trace
            .as_ref()
            .and_then(|trace| trace.ranges.get(index))
            .cloned()
            .unwrap_or(0..0);
        let draw = |scene: &mut Scene, from: f64, to: f64, color| {
            let a = endpoint(&step.at(from)).unwrap();
            let clipped = step.at(to);
            match clipped {
                Step::Linear { to, .. } => scene.add_strip_with_owner(
                    vec![
                        [a.x as f32, a.y as f32, a.z as f32],
                        [to.x as f32, to.y as f32, to.z as f32],
                    ],
                    color,
                    Some(tag.op_id),
                    false,
                    tag.pattern_id,
                    tag.instance_index,
                ),
                _ => {
                    let program = Program {
                        steps: vec![Step::Rapid { to: a, tag }, clipped],
                    };
                    let mut part = Scene::from_program(&program);
                    for strip in &mut part.strips {
                        strip.color = color;
                    }
                    scene.strips.append(&mut part.strips);
                }
            }
        };
        if range.start >= range.end {
            draw(scene, lo, hi, UNKNOWN);
            return;
        }
        let trace = self.force_trace.as_ref().unwrap();
        for index in range {
            let sample = &trace.samples[index];
            let a = lo.max(sample.start_fraction);
            let b = hi.min(sample.end_fraction);
            if b > a {
                let color = trace.loads[index].map_or(UNKNOWN, |load| {
                    force_color(load.tangential_force_n, trace.max_force)
                });
                draw(scene, a, b, color);
            }
        }
    }

    pub(super) fn force_legend(&self) -> Element<'_, Message> {
        let mut panel = column![text("Force from simulated material removal").size(12)].spacing(6);
        if let Some(material) = &self.force_material {
            panel = panel.push(
                text(format!(
                    "Material: {} · kc {} N/mm²",
                    material.name,
                    material
                        .specific_cutting_force
                        .map_or("unset".into(), |v| v.to_string())
                ))
                .size(11),
            );
        }
        if self
            .force_material
            .as_ref()
            .and_then(|m| m.specific_cutting_force)
            .is_none()
        {
            panel = panel.push(
                text("Choose a managed stock material and set its kc in Tools → Materials.")
                    .size(11),
            );
        }
        if self.force_jobs.pending() {
            let progress = self
                .force_jobs
                .progress()
                .map_or(String::new(), |p| format!(" {}/{}", p.completed, p.total));
            panel = panel.push(text(format!("Calculating removal…{progress}")).size(11));
        } else if let Some(error) = &self.force_error {
            panel = panel.push(text(error.clone()).size(11).color(palette::ERROR));
        } else if let Some(trace) = &self.force_trace {
            let swatch = |value| {
                row![
                    text("■").color(Color::from(force_color(value, trace.max_force))),
                    text(format!("{value:.1} N")).size(11),
                ]
                .spacing(3)
            };
            if trace.operations.values().any(|stats| stats.seconds > 0.) {
                panel = panel.push(
                    row![
                        swatch(0.),
                        swatch(trace.max_force / 2.),
                        swatch(trace.max_force)
                    ]
                    .spacing(8),
                );
            } else {
                panel =
                    panel.push(text("No valid force samples; cutting moves are gray.").size(11));
            }
            if trace.has_collision {
                panel = panel.push(
                    text("Collision encountered; loads from contact onward are unavailable.")
                        .size(11)
                        .color(palette::ERROR),
                );
            }
        }
        panel.push(text("Blue: air/low load · red: highest sampled load · gray: unavailable. Rapids retain move colors. Equivalent mean force at the nominal tool diameter; not peak tooth force. Accuracy follows the stock grid.").size(11).color(INSPECTOR_MUTED)).into()
    }

    pub(super) fn force_information(&self) -> Element<'_, Message> {
        let Some(trace) = &self.force_trace else {
            return self.force_legend();
        };
        let current = self
            .timeline
            .steps
            .iter()
            .enumerate()
            .find(|(_, s)| s.start <= self.rendered && s.end > self.rendered);
        let load = current.and_then(|(index, step)| {
            let fraction = (self.rendered - step.start) / (step.end - step.start);
            trace.ranges[index]
                .clone()
                .find(|&i| {
                    trace.samples[i].start_fraction <= fraction
                        && trace.samples[i].end_fraction > fraction
                })
                .and_then(|i| trace.loads[i])
        });
        match load {
            Some(load) => text(format!("Equivalent tangential force: {:.3} N\nNet cutting power: {:.3} W\nSpindle torque: {:.3} N·m\nRemoval rate: {:.3} cm³/min",
                load.tangential_force_n, load.cutting_power_w, load.spindle_torque_nm, load.removal_rate_mm3_min / 1000.)).size(12).into(),
            None => text("Force unavailable at this frame (non-cutting motion, missing data, drilling or collision).").size(11).into(),
        }
    }

    pub(super) fn force_statistics(&self) -> Element<'_, Message> {
        if !self.forces_enabled() {
            return text("Enable Toolpath → Color by → Cutting force to analyze actual removal.")
                .size(11)
                .into();
        }
        let Some(trace) = &self.force_trace else {
            return self.force_legend();
        };
        let mut panel = column![inspector_section("Loads per operation"),
            text("Full simulated program; mean includes air cuts during sampled feed motions. Max is a sampled interval mean.").size(11),
        ].spacing(8);
        for (id, stats) in &trace.operations {
            let force = if stats.seconds > 0. {
                format!(
                    "Mean {:.3} N · max {:.3} N",
                    stats.force_integral / stats.seconds,
                    stats.max_force
                )
            } else {
                "Force unavailable".into()
            };
            panel = panel.push(
                text(format!(
                    "Operation #{id}\n{force}\nRemoved {:.3} mm³{}",
                    stats.volume,
                    if stats.unknown > 0 {
                        " · incomplete force coverage"
                    } else {
                        ""
                    }
                ))
                .size(12),
            );
        }
        panel.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{ProgramBuilder, SpindleDir};
    use std::time::Duration;

    fn library(kc: Option<f64>) -> ToolLibrary {
        ToolLibrary {
            materials: vec![cam_model::Material {
                id: 7,
                name: "Test stock".into(),
                specific_cutting_force: kc,
            }],
            ..Default::default()
        }
    }
    fn player() -> SimulationPlayer {
        let mut player = super::super::tests::collision_player();
        let mut controller = AppController::new(crate::default_machine());
        controller.edit_setup(|s| {
            s.tools = vec![cam_model::Tool {
                number: 1,
                diameter: 4.,
                ..Default::default()
            }];
            s.material_id = Some(7);
        });
        let op = controller.operation_template(OpKind::Face, 1);
        controller.add_operation(op);
        let id = controller.selected_operation().unwrap().id();
        let mut program = ProgramBuilder::new()
            .tool_change(1)
            .spindle_on(10000., SpindleDir::Cw)
            .op(id)
            .feed(600.)
            .rapid(Point3::new(-4., 8., 5.), MoveKind::Link)
            .linear(Point3::new(-4., 8., -2.), MoveKind::Plunge)
            .linear(Point3::new(25., 8., -2.), MoveKind::Cutting)
            .linear(Point3::new(-4., 8., -2.), MoveKind::Cutting)
            .build();
        for step in &mut program.steps {
            if let Step::Linear { tag, .. } = step {
                tag.pattern_id = Some(17);
                tag.instance_index = Some(3);
            }
        }
        player.inputs = controller.job_inputs();
        player.inputs.min = [0., 0., -5.];
        player.inputs.max = [20., 20., 0.];
        player.inputs.preview_resolution = Some(0.1);
        player.timeline = Timeline::new(&program, 2000.).unwrap();
        player.options.show_model = false;
        player.options.toolpath_mode = SimulationToolpathMode::All;
        player.sync_force_material(&library(Some(1500.)));
        player
    }
    fn drain(player: &mut SimulationPlayer) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while player.force_jobs.needs_poll() {
            player.poll_forces();
            assert!(Instant::now() < deadline, "force worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(player.force_error.is_none(), "{:?}", player.force_error);
    }
    fn drain_frames(player: &mut SimulationPlayer) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while player.needs_tick() {
            player.tick();
            assert!(Instant::now() < deadline, "stock/force frame timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(player.force_error.is_none(), "{:?}", player.force_error);
    }
    fn enable(player: &mut SimulationPlayer) {
        let mut prefs = player.options.clone();
        prefs.toolpath_coloring = SimulationToolpathColoring::CuttingForce;
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        drain(player);
    }

    #[test]
    fn analysis_colors_actual_removal_and_repeat_passes_without_changing_playback_stock() {
        let mut player = player();
        let document = player.inputs.document.clone();
        let stock = player.preview.stock_vertices.clone();
        enable(&mut player);
        let trace = player.force_trace.as_ref().unwrap();
        assert!(trace
            .loads
            .iter()
            .flatten()
            .any(|l| l.tangential_force_n > 0.));
        assert!(trace
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.step_index == 5)
            .all(|(i, _)| trace.loads[i].unwrap().tangential_force_n == 0.));
        assert!((trace.operations.values().next().unwrap().volume - 160.).abs() < 1.);
        let colors: Vec<_> = player
            .preview
            .scene
            .strips
            .iter()
            .filter(|s| s.pattern_id == Some(17))
            .map(|s| s.color)
            .collect();
        assert!(colors
            .iter()
            .any(|&c| c == force_color(0., trace.max_force)));
        assert!(colors
            .iter()
            .any(|&c| c != force_color(0., trace.max_force) && c != UNKNOWN));
        assert_eq!(player.inputs.document, document);
        assert_eq!(player.preview.stock_vertices, stock);
        assert!(player
            .preview
            .scene
            .strips
            .iter()
            .filter(|s| s.pattern_id == Some(17))
            .all(|s| s.instance_index == Some(3)));
        let max = trace.max_force;
        let raw = trace.samples.clone();
        player.rendered = player.timeline.duration / 2.;
        player.refresh_display();
        assert_eq!(
            player.force_trace.as_ref().unwrap().max_force,
            max,
            "rewind must keep a fixed color scale"
        );
        assert!(Arc::ptr_eq(
            &raw,
            &player.force_trace.as_ref().unwrap().samples
        ));
        let _info = player.force_information();
        let _stats = player.force_statistics();
        let _legend = player.force_legend();
    }

    #[test]
    fn material_edits_rescale_loads_without_resimulating_and_missing_data_is_gray() {
        let mut player = player();
        enable(&mut player);
        let original = player.force_trace.as_ref().unwrap();
        let max = original.max_force;
        let raw = original.samples.clone();
        player.sync_force_material(&library(Some(3000.)));
        let trace = player.force_trace.as_ref().unwrap();
        assert_eq!(trace.max_force, max * 2.);
        assert!(Arc::ptr_eq(&raw, &trace.samples));
        assert!(!player.force_jobs.pending());
        player.sync_force_material(&library(None));
        let trace = player.force_trace.as_ref().unwrap();
        assert!(trace.loads.iter().all(Option::is_none));
        assert!(trace.operations.values().all(|s| s.unknown > 0));
        assert!(player
            .preview
            .scene
            .strips
            .iter()
            .filter(|s| s.pattern_id == Some(17))
            .all(|s| s.color == UNKNOWN));
    }

    #[test]
    fn stock_coloring_colors_cut_faces_independently_of_toolpath_visibility() {
        let mut player = player();
        let mut prefs = player.options.clone();
        prefs.show_toolpath = false;
        prefs.show_cutting = false;
        assert!(SimulationStockColoring::ALL.contains(&SimulationStockColoring::CuttingForce));
        assert_eq!(
            SimulationStockColoring::CuttingForce.to_string(),
            "Cutting force"
        );
        SimulationOption::Coloring(SimulationStockColoring::CuttingForce).apply(&mut prefs);
        assert!(!prefs.show_toolpath && !prefs.show_cutting);
        assert_eq!(
            prefs.toolpath_coloring,
            SimulationToolpathColoring::MoveType
        );
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        player.seek_end(true);
        drain_frames(&mut player);
        assert!(player.force_trace.as_ref().unwrap().max_force > 0.);
        assert!(player.force_viewport_legend(0., Some(110.)).is_some());
        assert!(!player.preview.stock_cut_sources.is_empty());
        let material = player.options.material.color(player.custom_stock_color);
        let mut colored = 0;
        let mut original = 0;
        for (i, vertex) in player.preview.stock_vertices.iter().enumerate() {
            if player.preview.stock_machined[i] {
                if player.preview.stock_cut_sources[i].is_some() {
                    assert_ne!(vertex.color, UNKNOWN);
                    assert_ne!(vertex.color, material);
                    colored += 1;
                }
            } else {
                assert_eq!(vertex.color, material);
                original += 1;
            }
        }
        assert!(colored > 0 && original > 0);
        assert!(
            player
                .preview
                .stock_cut_sources
                .iter()
                .flatten()
                .all(|source| source.step_index == 4),
            "the air pass must not overwrite cut-face attribution"
        );
        player.sync_force_material(&library(None));
        assert!(player
            .preview
            .stock_vertices
            .iter()
            .enumerate()
            .filter(|(i, _)| player.preview.stock_machined[*i])
            .all(|(_, vertex)| vertex.color == UNKNOWN));
        SimulationOption::Coloring(SimulationStockColoring::Operation).apply(&mut prefs);
        assert_eq!(
            prefs.toolpath_coloring,
            SimulationToolpathColoring::MoveType
        );
        SimulationOption::Coloring(SimulationStockColoring::CuttingForce).apply(&mut prefs);
        SimulationOption::PathColoring(SimulationToolpathColoring::MoveType).apply(&mut prefs);
        assert_eq!(prefs.stock_coloring, SimulationStockColoring::CuttingForce);
    }

    #[test]
    fn partial_and_rewound_stock_frames_keep_matching_force_attribution() {
        let mut player = player();
        let mut prefs = player.options.clone();
        prefs.stock_coloring = SimulationStockColoring::CuttingForce;
        prefs.show_toolpath = false;
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        let step = &player.timeline.steps[4];
        let halfway = step.start + (step.end - step.start) * 0.5;
        player.seek(halfway);
        drain_frames(&mut player);
        assert!(player
            .preview
            .stock_cut_sources
            .iter()
            .flatten()
            .filter(|s| s.step_index == 4)
            .all(|s| s.fraction <= 0.5 + 1e-8));
        player.seek_end(true);
        drain_frames(&mut player);
        let sources = player.preview.stock_cut_sources.clone();
        let colors: Vec<_> = player
            .preview
            .stock_vertices
            .iter()
            .map(|v| v.color)
            .collect();
        prefs.regenerate_on_rewind = false;
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        player.seek(halfway);
        drain_frames(&mut player);
        assert_eq!(player.preview.stock_cut_sources, sources);
        assert_eq!(
            player
                .preview
                .stock_vertices
                .iter()
                .map(|v| v.color)
                .collect::<Vec<_>>(),
            colors
        );
    }

    #[test]
    fn colored_intervals_clip_to_the_requested_motion_fraction() {
        let mut player = player();
        enable(&mut player);
        let step = &player.timeline.steps[4];
        let tag = motion_tag(&step.step).unwrap();
        let duration = step.end - step.start;
        let mut scene = Scene::new();
        player.add_force_motion(
            &mut scene,
            4,
            step,
            step.start + duration * 0.25,
            step.start + duration * 0.75,
            tag,
        );
        assert!(!scene.strips.is_empty());
        let points: Vec<_> = scene.strips.iter().flat_map(|s| &s.points).collect();
        assert!(points
            .iter()
            .all(|p| p[0] >= 3.25 - 1e-5 && p[0] <= 17.75 + 1e-5));
        assert!((points.first().unwrap()[0] - 3.25).abs() < 1e-5);
        assert!((points.last().unwrap()[0] - 17.75).abs() < 1e-5);
        assert!(scene
            .strips
            .iter()
            .all(|s| s.pattern_id == Some(17) && s.instance_index == Some(3)));
    }

    #[test]
    fn accuracy_changes_rebuild_the_trace_and_disabling_cancels_pending_work() {
        let mut player = player();
        enable(&mut player);
        let raw = player.force_trace.as_ref().unwrap().samples.clone();
        let mut prefs = player.options.clone();
        prefs.accuracy = 1.5;
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        assert!(player.force_trace.is_none());
        assert!(player.force_jobs.pending());
        prefs.toolpath_coloring = SimulationToolpathColoring::MoveType;
        player.configure(&prefs, [0.2, 0.6, 0.3]);
        assert!(!player.force_jobs.pending());
        drain(&mut player);
        assert!(
            player.force_trace.is_none(),
            "cancelled work must not publish a stale trace"
        );
        enable(&mut player);
        assert!(!Arc::ptr_eq(
            &raw,
            &player.force_trace.as_ref().unwrap().samples
        ));
    }

    #[test]
    fn drilling_and_collisions_are_unavailable_instead_of_reported_as_zero() {
        let mut player = player();
        enable(&mut player);
        let mut samples = (*player.force_trace.as_ref().unwrap().samples).clone();
        for sample in &mut samples {
            sample.collision = true;
        }
        let trace = ForceTrace::new(
            Arc::new(samples),
            &player.timeline,
            &player.inputs,
            Some(1500.),
        );
        assert!(trace.loads.iter().all(Option::is_none));
        let controller = AppController::new(crate::default_machine());
        let mut drill = controller.operation_template(OpKind::Drill, 1);
        drill.set_id(player.inputs.document.setup().operations[0].id());
        Arc::make_mut(&mut player.inputs.document)
            .setup_mut()
            .operations[0] = drill;
        let trace = ForceTrace::new(
            player.force_trace.as_ref().unwrap().samples.clone(),
            &player.timeline,
            &player.inputs,
            Some(1500.),
        );
        assert!(trace.loads.iter().all(Option::is_none));
    }
}
