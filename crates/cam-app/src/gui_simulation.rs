//! Video-style playback uses a separate, cancellable preview lane. The verified
//! machining outcome remains intact for diagnostics and post processing.
use super::*;
use crate::jobs::{JobInputs, JobService};
use cam_cldata::{ArcDir, Point3, Program, Step};
use iced::widget::column;
use std::time::Instant;

struct TimedStep {
    step: Step,
    from: Option<Point3>,
    start: f64,
    end: f64,
}

struct Timeline {
    steps: Vec<TimedStep>,
    duration: f64,
}

fn distance(a: Point3, b: Point3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z)
}

fn arc_sweep(a: Point3, end: Point3, center: Point3, dir: ArcDir) -> f64 {
    let start = (a.y - center.y).atan2(a.x - center.x);
    let finish = (end.y - center.y).atan2(end.x - center.x);
    let sweep = match dir {
        ArcDir::Ccw => (finish - start).rem_euclid(std::f64::consts::TAU),
        ArcDir::Cw => -(start - finish).rem_euclid(std::f64::consts::TAU),
    };
    if sweep.abs() < 1e-10 {
        if dir == ArcDir::Ccw {
            std::f64::consts::TAU
        } else {
            -std::f64::consts::TAU
        }
    } else {
        sweep
    }
}

impl Timeline {
    fn new(program: &Program, rapid_rate: f64) -> Result<Self, String> {
        let mut expanded = Vec::new();
        // Expand cycles so the timeline can stop inside a hole, including pecks
        // and dwells, instead of treating an entire drill cycle as one frame.
        for step in &program.steps {
            if let Step::Drill(c) = step {
                let pecks = c
                    .peck
                    .map_or(1., |peck| ((c.z_top - c.depth) / peck).ceil());
                if !pecks.is_finite() || pecks * c.points.len() as f64 > 100_000. {
                    return Err("Simulation playback exceeds 100000 drilling pecks.".into());
                }
                for &[x, y] in &c.points {
                    let rapid = |z| Step::Rapid {
                        to: Point3::new(x, y, z),
                        tag: c.tag,
                    };
                    expanded.push(rapid(c.retract));
                    expanded.push(rapid(c.z_top));
                    let mut top = c.z_top;
                    loop {
                        let target = c.peck.map_or(c.depth, |peck| (top - peck).max(c.depth));
                        if target >= top {
                            return Err("Drilling peck cannot advance at this depth.".into());
                        }
                        if top < c.z_top {
                            expanded.push(rapid((top + 0.5).min(c.z_top)));
                        }
                        expanded.push(Step::Linear {
                            to: Point3::new(x, y, target),
                            feed: c.feed,
                            tag: c.tag,
                        });
                        if target <= c.depth {
                            if let Some(seconds) = c.dwell {
                                expanded.push(Step::Dwell { seconds });
                            }
                        }
                        expanded.push(rapid(c.retract));
                        top = target;
                        if target <= c.depth {
                            break;
                        }
                    }
                }
            } else {
                expanded.push(step.clone());
            }
        }
        let mut steps = Vec::with_capacity(expanded.len());
        let mut position = None;
        let mut duration = 0.;
        for step in expanded {
            let from = position;
            let seconds = match &step {
                Step::Rapid { to, .. } => {
                    position = Some(*to);
                    from.map_or(0., |a| distance(a, *to) / rapid_rate.max(1.) * 60.)
                }
                Step::Linear { to, feed, .. } => {
                    position = Some(*to);
                    from.map_or(0., |a| distance(a, *to) / feed.max(1e-6) * 60.)
                }
                Step::Arc {
                    end,
                    center,
                    dir,
                    feed,
                    ..
                } => {
                    position = Some(*end);
                    from.map_or(0., |a| {
                        let radius = (a.x - center.x).hypot(a.y - center.y);
                        let length =
                            (radius * arc_sweep(a, *end, *center, *dir)).hypot(end.z - a.z);
                        length / feed.max(1e-6) * 60.
                    })
                }
                Step::Dwell { seconds } => *seconds,
                _ => 0.,
            };
            let start = duration;
            duration += seconds.max(0.);
            steps.push(TimedStep {
                step,
                from,
                start,
                end: duration,
            });
        }
        Ok(Self { steps, duration })
    }

    fn prefix(&self, seconds: f64) -> Program {
        let seconds = seconds.clamp(0., self.duration);
        let count = self.steps.partition_point(|step| step.end <= seconds);
        let mut steps: Vec<_> = self.steps[..count].iter().map(|s| s.step.clone()).collect();
        if let Some(timed) = self.steps.get(count) {
            let t = ((seconds - timed.start) / (timed.end - timed.start)).clamp(0., 1.);
            if t > 0. {
                let mut step = timed.step.clone();
                if let Some(a) = timed.from {
                    match &mut step {
                        Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                            *to = Point3::new(
                                a.x + (to.x - a.x) * t,
                                a.y + (to.y - a.y) * t,
                                a.z + (to.z - a.z) * t,
                            );
                        }
                        Step::Arc {
                            end, center, dir, ..
                        } => {
                            let angle = (a.y - center.y).atan2(a.x - center.x)
                                + arc_sweep(a, *end, *center, *dir) * t;
                            let radius = (a.x - center.x).hypot(a.y - center.y);
                            *end = Point3::new(
                                center.x + radius * angle.cos(),
                                center.y + radius * angle.sin(),
                                a.z + (end.z - a.z) * t,
                            );
                        }
                        Step::Dwell { seconds } => *seconds *= t,
                        _ => {}
                    }
                }
                steps.push(step);
            }
        }
        Program { steps }
    }
}

pub(super) struct SimulationPlayer {
    source: crate::JobStamp,
    inputs: JobInputs,
    timeline: Timeline,
    jobs: JobService,
    pub preview: crate::RunOutcome,
    pub position: f64,
    requested: f64,
    rendered: f64,
    pub playing: bool,
    pub speed: f64,
    error: Option<String>,
    last_tick: Instant,
}

impl SimulationPlayer {
    pub fn new(controller: &AppController) -> Result<Self, String> {
        let outcome = controller.outcome().ok_or("No toolpath to simulate.")?;
        let source = outcome
            .stamp
            .clone()
            .ok_or("Run the toolpath before playback.")?;
        let timeline = Timeline::new(&outcome.program, controller.machine().rapid_rate)?;
        let preview = crate::RunOutcome {
            program: timeline.prefix(0.),
            ..Default::default()
        };
        let mut inputs = controller.job_inputs();
        inputs.preview_resolution = Some(inputs.stock_resolution(&outcome.program));
        let mut player = Self {
            source,
            inputs,
            timeline,
            jobs: JobService::default(),
            preview,
            position: 0.,
            requested: 0.,
            rendered: 0.,
            playing: false,
            speed: 1.,
            error: None,
            last_tick: Instant::now(),
        };
        player.request_frame();
        Ok(player)
    }
    pub fn current(&self, controller: &AppController) -> bool {
        controller.revision() == self.source.revision
            && controller.outcome().and_then(|o| o.stamp.as_ref()) == Some(&self.source)
    }
    fn request_frame(&mut self) {
        let mut inputs = self.inputs.clone();
        inputs.preview_program = Some(self.timeline.prefix(self.position));
        self.requested = self.position;
        self.jobs.start(
            self.source.revision,
            format!("playback:{}", self.position),
            inputs,
            Default::default(),
        );
    }
    pub fn seek(&mut self, seconds: f64) {
        self.error = None;
        self.playing = false;
        self.position = seconds.clamp(0., self.timeline.duration);
        self.last_tick = Instant::now();
        self.request_frame();
    }
    pub fn toggle(&mut self) {
        if self.position >= self.timeline.duration {
            self.seek(0.);
        }
        self.playing = !self.playing;
        self.last_tick = Instant::now();
    }
    pub fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if let Some(result) = self.jobs.poll(self.source.revision) {
            if result.outcome.completion == JobCompletion::Complete {
                self.preview = result.outcome;
                self.rendered = self.requested;
            } else {
                self.playing = false;
                self.rendered = self.requested;
                self.error = Some("Simulation preview failed. Seek again to retry.".into());
            }
        }
        if self.playing {
            self.position = (self.position + elapsed * self.speed).min(self.timeline.duration);
            if self.position >= self.timeline.duration {
                self.playing = false;
            }
        }
        if !self.jobs.pending() && (self.position - self.rendered).abs() > 1e-6 {
            self.request_frame();
        }
    }
    pub fn needs_tick(&self) -> bool {
        self.playing || self.jobs.needs_poll()
    }
    /// Follow the rendered frame, including its active tool, when seeking in
    /// either direction. The tool and stock must describe the same instant.
    pub fn tool_mesh(&self) -> (Vec<MeshVertex>, Vec<u32>) {
        let program = &self.preview.program;
        let tip = program.steps.iter().rev().find_map(|step| match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
            Step::Arc { end, .. } => Some(*end),
            _ => None,
        });
        let number = program.steps.iter().rev().find_map(|step| match step {
            Step::ToolChange { tool } => Some(*tool),
            _ => None,
        });
        let setup = self.inputs.document.setup();
        let tool = setup.tools.iter().find(|tool| Some(tool.number) == number);
        let (Some(tip), Some(tool)) = (tip, tool) else {
            return (vec![], vec![]);
        };
        let (mut vertices, indices) = cam_render::tool_mesh(&tool.profile());
        for vertex in &mut vertices {
            vertex.position = setup
                .frame
                .to_world_vector([
                    tip.x + vertex.position[0] as f64,
                    tip.y + vertex.position[1] as f64,
                    tip.z + vertex.position[2] as f64,
                ])
                .map(|v| v as f32);
            vertex.normal = setup
                .frame
                .to_world_vector(vertex.normal.map(f64::from))
                .map(|v| v as f32);
        }
        (vertices, indices)
    }
    pub fn controls(&self) -> Element<'_, Message> {
        let duration = self.timeline.duration.max(0.001);
        let time = |s: f64| format!("{}:{:02}", s as u64 / 60, s as u64 % 60);
        container(
            column![
                row![
                    inspector_button(if self.playing { "Pause" } else { "Play" })
                        .on_press(Message::SimulationPlayPause),
                    text(format!(
                        "{} / {}",
                        time(self.position),
                        time(self.timeline.duration)
                    ))
                    .size(12),
                    text(format!("{:.0}%", self.position / duration * 100.)).size(12),
                    text(self.error.as_deref().unwrap_or(if self.jobs.pending() {
                        "Updating…"
                    } else {
                        ""
                    }))
                    .size(12),
                    Space::new().width(Length::Fill),
                    text(format!("Speed {:.2}×", self.speed)).size(12),
                    slider(0.25..=10., self.speed, Message::SimulationSpeed)
                        .step(0.25)
                        .width(150),
                    inspector_button("Close simulation").on_press(Message::SimulationClose),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                slider(0.0..=duration, self.position, Message::SimulationSeek)
                    .step((duration / 10000.).max(0.001)),
            ]
            .spacing(6),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .into()
    }
}

impl App {
    pub(super) fn simulation_controls(&self) -> Element<'_, Message> {
        self.simulation
            .as_ref()
            .map_or_else(|| Space::new().height(0).into(), SimulationPlayer::controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{MoveKind, Tag};
    use std::time::Duration;

    fn tag() -> Tag {
        Tag::new(7, MoveKind::Cutting)
    }
    fn last_position(program: &Program) -> Point3 {
        program
            .steps
            .iter()
            .rev()
            .find_map(|step| match step {
                Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
                Step::Arc { end, .. } => Some(*end),
                _ => None,
            })
            .unwrap()
    }
    #[test]
    fn timeline_interpolates_feed_moves_and_rewinds_without_future_steps() {
        let timeline = Timeline::new(
            &Program {
                steps: vec![
                    Step::Rapid {
                        to: Point3::new(0., 0., 0.),
                        tag: tag(),
                    },
                    Step::Linear {
                        to: Point3::new(10., 0., 0.),
                        feed: 60.,
                        tag: tag(),
                    },
                    Step::Dwell { seconds: 2. },
                    Step::SpindleOff,
                ],
            },
            2000.,
        )
        .unwrap();
        assert_eq!(timeline.duration, 12.);
        assert_eq!(last_position(&timeline.prefix(5.)).x, 5.);
        assert!(!timeline.prefix(11.).steps.contains(&Step::SpindleOff));
        assert!(timeline.prefix(12.).steps.contains(&Step::SpindleOff));
        assert_eq!(last_position(&timeline.prefix(2.)).x, 2.);
        assert_eq!(last_position(&timeline.prefix(0.)).x, 0.);
    }
    #[test]
    fn timeline_interpolates_full_circle_helices_in_both_directions() {
        for dir in [ArcDir::Cw, ArcDir::Ccw] {
            let timeline = Timeline::new(
                &Program {
                    steps: vec![
                        Step::Rapid {
                            to: Point3::new(1., 0., 0.),
                            tag: tag(),
                        },
                        Step::Arc {
                            end: Point3::new(1., 0., -2.),
                            center: Point3::new(0., 0., 0.),
                            dir,
                            feed: 60.,
                            tag: tag(),
                        },
                    ],
                },
                2000.,
            )
            .unwrap();
            let half = timeline.prefix(timeline.duration / 2.);
            assert!(half.validate().is_ok());
            let at = last_position(&half);
            assert!((at.x + 1.).abs() < 1e-9);
            assert!((at.z + 1.).abs() < 1e-9);
        }
    }
    #[test]
    fn drilling_playback_has_partial_pecks_and_dwell_time() {
        let timeline = Timeline::new(
            &Program {
                steps: vec![Step::Drill(cam_cldata::DrillCycle {
                    points: vec![[2., 3.]],
                    z_top: 0.,
                    depth: -2.,
                    retract: 1.,
                    peck: Some(1.),
                    dwell: Some(2.),
                    feed: 60.,
                    tag: tag(),
                })],
            },
            60.,
        )
        .unwrap();
        assert!(timeline
            .steps
            .iter()
            .all(|s| !matches!(s.step, Step::Drill(_))));
        assert!((last_position(&timeline.prefix(1.5)).z + 0.5).abs() < 1e-9);
        let full = timeline.prefix(timeline.duration);
        assert!(full.steps.contains(&Step::Dwell { seconds: 2. }));
        assert_eq!(last_position(&full).z, 1.);
    }
    fn drain_player(player: &mut SimulationPlayer) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while player.needs_tick() {
            player.tick();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn seeking_rebuilds_stock_and_preserves_verified_export() {
        let mut controller = AppController::new(crate::default_machine());
        controller.edit_stock(|stock| {
            *stock = cam_model::Stock::FixedBox {
                size: [8., 8., 4.],
                model_position: [0.; 3],
            }
        });
        controller.add_operation(Operation::Drill(cam_model::DrillOp {
            id: 0,
            tool: 1,
            points: vec![[4., 4.]],
            depth: 2.,
            start_offset: 0.,
            peck: None,
            dwell: None,
            spindle_rpm: 1000.,
            tool_preset: None,
            work_offset: 1,
            feed: 100.,
        }));
        controller.start_run();
        let deadline = Instant::now() + Duration::from_secs(15);
        while controller.run_needs_poll() {
            controller.poll_run();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let nc = controller.export_nc().unwrap().to_owned();
        let mut player = SimulationPlayer::new(&controller).unwrap();
        drain_player(&mut player);
        assert!(
            !player.preview.stock_vertices.is_empty(),
            "opening playback shows the initial stock"
        );
        assert!(player.preview.stock_machined.iter().all(|&cut| !cut));
        assert_eq!(
            player.inputs.preview_resolution,
            Some(
                player
                    .inputs
                    .stock_resolution(&controller.outcome().unwrap().program)
            )
        );
        player.seek(player.timeline.duration);
        drain_player(&mut player);
        let cut = player.preview.stock_vertices.clone();
        let cut_faces = player.preview.stock_machined.clone();
        assert!(!cut.is_empty());
        assert!(cut_faces.iter().any(|&cut| cut));
        player.seek(0.);
        drain_player(&mut player);
        assert!(
            cut != player.preview.stock_vertices,
            "rewinding must restore uncut stock"
        );
        assert!(player.preview.stock_machined.iter().all(|&cut| !cut));
        assert!(player.current(&controller));
        assert_eq!(controller.export_nc().unwrap(), nc);
        player.seek(player.timeline.duration);
        drain_player(&mut player);
        assert!(
            cut == player.preview.stock_vertices,
            "seeking forward must reproduce the same stock"
        );
        assert_eq!(cut_faces, player.preview.stock_machined);
        player.toggle();
        assert!(player.playing);
        assert_eq!(player.position, 0.);
        player.toggle();
        assert!(!player.playing);
        controller.edit_operation(0, |op| {
            if let Operation::Drill(op) = op {
                op.depth = 1.;
            }
        });
        assert!(!player.current(&controller));
    }

    #[test]
    fn solid_tool_follows_tool_changes_and_rewind_in_rotated_setup() {
        let mut controller = AppController::new(crate::default_machine());
        controller.edit_setup(|setup| {
            setup.frame = cam_model::SetupFrame::from_xy([0., 0., 1.], [1., 0., 0.]).unwrap();
            setup.tools = vec![
                cam_model::Tool {
                    number: 1,
                    diameter: 6.,
                    length: 30.,
                    ..Default::default()
                },
                cam_model::Tool {
                    number: 2,
                    diameter: 12.,
                    length: 40.,
                    ..Default::default()
                },
            ];
        });
        let timeline = Timeline::new(
            &Program {
                steps: vec![
                    Step::ToolChange { tool: 1 },
                    Step::Rapid {
                        to: Point3::new(0., 0., 0.),
                        tag: tag(),
                    },
                    Step::Linear {
                        to: Point3::new(10., 0., 0.),
                        feed: 60.,
                        tag: tag(),
                    },
                    Step::ToolChange { tool: 2 },
                    Step::Linear {
                        to: Point3::new(10., 10., 0.),
                        feed: 60.,
                        tag: tag(),
                    },
                ],
            },
            2000.,
        )
        .unwrap();
        let mut player = SimulationPlayer {
            source: crate::JobStamp {
                request_id: 1,
                revision: 0,
                input_fingerprint: String::new(),
            },
            inputs: controller.job_inputs(),
            timeline,
            jobs: JobService::default(),
            preview: Default::default(),
            position: 0.,
            requested: 0.,
            rendered: 0.,
            playing: false,
            speed: 1.,
            error: None,
            last_tick: Instant::now(),
        };
        let frame = &controller.document().setup().frame;
        for (time, tip, radius, length) in [
            (5., [5., 0., 0.], 3., 30.),
            (15., [10., 5., 0.], 6., 40.),
            (5., [5., 0., 0.], 3., 30.),
        ] {
            player.preview.program = player.timeline.prefix(time);
            let (vertices, indices) = player.tool_mesh();
            assert!(!indices.is_empty());
            let points: Vec<_> = vertices
                .iter()
                .map(|v| frame.to_setup_vector(v.position.map(f64::from)))
                .collect();
            let min: [f64; 3] =
                std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min));
            let max: [f64; 3] = std::array::from_fn(|i| {
                points
                    .iter()
                    .map(|p| p[i])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
            for axis in 0..3 {
                let expected_min = tip[axis] - if axis < 2 { radius } else { 0. };
                let expected_max = tip[axis] + if axis < 2 { radius } else { length };
                assert!((min[axis] - expected_min).abs() < 1e-5);
                assert!((max[axis] - expected_max).abs() < 1e-5);
            }
        }
    }
}
