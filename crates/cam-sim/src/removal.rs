//! Spatially sampled removal against the evolving stock. A separate traversal
//! leaves verification checkpoints and ordinary playback geometry unchanged.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct RemovalSample {
    /// Index and fractions in the supplied program (including non-motion steps).
    pub step_index: usize,
    pub start_fraction: f64,
    pub end_fraction: f64,
    pub seconds: f64,
    pub removed_volume_mm3: f64,
    pub tool: Option<u32>,
    pub spindle_rpm: f64,
    /// This or an earlier motion collided; subsequent load is not trustworthy.
    pub collision: bool,
}

const MAX_SAMPLES: usize = 500_000;

/// Sample actual stock removal using the same cutter profiles and collision
/// rules as verification. Initial stock can contain predecessor cuts/voids.
/// Arcs are sampled in equal angular/time intervals, including helical Z travel.
#[allow(clippy::too_many_arguments)]
pub fn analyze_removal_controlled(
    program: &Program,
    min: [f64; 3],
    max: [f64; 3],
    options: &SimOptions,
    tools: &[SimTool],
    initial: Option<&Heightfield>,
    cancelled: &dyn Fn() -> bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<RemovalSample>, String> {
    if !options.resolution.is_finite()
        || options.resolution <= 0.
        || !min.into_iter().chain(max).all(f64::is_finite)
        || min.iter().zip(max).any(|(&a, b)| a >= b)
    {
        return Err(
            "Force analysis requires finite stock bounds and positive grid spacing.".into(),
        );
    }
    program
        .validate()
        .map_err(|e| format!("Invalid force-analysis program: {e:?}"))?;
    let config = Config {
        min,
        max,
        options: *options,
        tools: tools.to_vec(),
        initial_key: None,
        surface_sources: false,
    };
    let mut state = State::new(&config);
    if let Some(field) = initial {
        state.field = field.clone();
    }
    state.field.start_removal_tracking(min[2]);
    let mut samples = Vec::new();
    let mut tool = None;
    let mut rpm = 0.;
    let default_tool = ToolProfile::flat(options.tool_radius);
    for (step_index, step) in program.steps().iter().enumerate() {
        if cancelled() {
            return Err("Force analysis cancelled.".into());
        }
        progress(step_index, program.steps().len());
        match step {
            Step::ToolChange { tool: number } => tool = Some(*number),
            Step::Spindle { rpm: speed, .. } => rpm = *speed,
            Step::SpindleOff => rpm = 0.,
            _ => {}
        }
        let Some(from) = state.cur else {
            if !state.step(step, step_index, tools, default_tool, cancelled) {
                return Err("Force analysis cancelled.".into());
            }
            continue;
        };
        let motion = match step {
            Step::Linear { to, feed, .. } => Some((*to, *feed, distance(from, *to))),
            Step::Arc {
                end,
                center,
                dir,
                feed,
                ..
            } => {
                let angle = cam_cldata::arc_sweep(from, *end, *center, *dir);
                let radius = (from.x - center.x).hypot(from.y - center.y);
                Some((*end, *feed, (radius * angle).hypot(end.z - from.z)))
            }
            _ => None,
        };
        let Some((end, feed, length)) = motion else {
            if !state.step(step, step_index, tools, default_tool, cancelled) {
                return Err("Force analysis cancelled.".into());
            }
            continue;
        };
        if length <= 1e-12 {
            state.step(step, step_index, tools, default_tool, cancelled);
            continue;
        }
        // Several cells per interval smooth lattice quantization without hiding
        // engagement changes over distances comparable to the cutter radius.
        let spacing = (options.resolution * 4.)
            .max(state.tool.radius * 0.5)
            .max(0.05);
        let count = (length / spacing).ceil().max(1.);
        if !count.is_finite() || count > MAX_SAMPLES.saturating_sub(samples.len()) as f64 {
            return Err(format!(
                "Force analysis exceeds {MAX_SAMPLES} samples; use a smaller simulation scope or lower accuracy."
            ));
        }
        let count = count as usize;
        let seconds = length / feed * 60. / count as f64;
        if !seconds.is_finite() || seconds <= 0. {
            return Err(
                "Force-analysis interval duration is not representable; check the programmed feed."
                    .into(),
            );
        }
        for index in 0..count {
            if cancelled() {
                return Err("Force analysis cancelled.".into());
            }
            let lo = index as f64 / count as f64;
            let hi = (index + 1) as f64 / count as f64;
            let mut part = step.clone();
            match &mut part {
                Step::Linear { to, .. } => *to = interpolate(from, end, hi),
                Step::Arc {
                    end: to,
                    center,
                    dir,
                    ..
                } => {
                    let angle = (from.y - center.y).atan2(from.x - center.x)
                        + cam_cldata::arc_sweep(from, end, *center, *dir)
                            * hi
                            * if *dir == ArcDir::Cw { -1. } else { 1. };
                    let radius = (from.x - center.x).hypot(from.y - center.y);
                    *to = Point3::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        from.z + (end.z - from.z) * hi,
                    );
                    if index + 1 == count {
                        *to = end;
                    }
                }
                _ => unreachable!(),
            }
            let before = state.field.tracked_removed_volume();
            if !state.step(&part, step_index, tools, default_tool, cancelled) {
                return Err("Force analysis cancelled.".into());
            }
            let after = state.field.tracked_removed_volume();
            if !before.is_finite() || !after.is_finite() {
                return Err("Simulated removal volume overflowed.".into());
            }
            samples.push(RemovalSample {
                step_index,
                start_fraction: lo,
                end_fraction: hi,
                seconds,
                removed_volume_mm3: (after - before).max(0.),
                tool,
                spindle_rpm: rpm,
                collision: !state.collisions.is_empty(),
            });
        }
    }
    if cancelled() {
        return Err("Force analysis cancelled.".into());
    }
    progress(program.steps().len(), program.steps().len());
    Ok(samples)
}

fn distance(a: Point3, b: Point3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z)
}
fn interpolate(a: Point3, b: Point3, t: f64) -> Point3 {
    Point3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{MoveKind, ProgramBuilder, SpindleDir};
    use std::cell::Cell;

    fn options() -> SimOptions {
        SimOptions {
            resolution: 0.1,
            tool_radius: 2.,
        }
    }
    fn tools() -> Vec<SimTool> {
        vec![SimTool {
            number: 1,
            profile: ToolProfile::flat(2.),
            noncutting_tip_radius: None,
        }]
    }
    fn slot() -> ProgramBuilder {
        ProgramBuilder::new()
            .tool_change(1)
            .spindle_on(10000., SpindleDir::Cw)
            .op(0)
            .feed(600.)
            .rapid(Point3::new(-4., 8., 5.), MoveKind::Link)
            .linear(Point3::new(-4., 8., -2.), MoveKind::Plunge)
            .linear(Point3::new(25., 8., -2.), MoveKind::Cutting)
    }
    fn analyze(program: &Program, initial: Option<&Heightfield>) -> Vec<RemovalSample> {
        analyze_removal_controlled(
            program,
            [0., 0., -5.],
            [20., 20., 0.],
            &options(),
            &tools(),
            initial,
            &|| false,
            |_, _| {},
        )
        .unwrap()
    }

    #[test]
    fn air_cuts_are_zero_and_repeating_a_pass_removes_nothing() {
        let program = slot()
            .linear(Point3::new(-4., 8., -2.), MoveKind::Cutting)
            .build();
        let samples = analyze(&program, None);
        let first: Vec<_> = samples.iter().filter(|s| s.step_index == 4).collect();
        assert!(!first.is_empty());
        assert!(first.iter().any(|s| s.removed_volume_mm3 == 0.));
        assert!(first.iter().any(|s| s.removed_volume_mm3 > 0.));
        assert!(samples
            .iter()
            .filter(|s| s.step_index == 5)
            .all(|s| s.removed_volume_mm3 == 0.));
        assert!(samples
            .iter()
            .all(|s| !s.collision && s.spindle_rpm == 10000. && s.tool == Some(1)));
        let volume: f64 = samples.iter().map(|s| s.removed_volume_mm3).sum();
        assert!((volume - 160.).abs() < 1., "volume {volume}");
    }

    #[test]
    fn samples_follow_programmed_tool_feed_and_spindle_changes() {
        let program = slot()
            .spindle_on(20000., SpindleDir::Cw)
            .tool_change(2)
            .feed(1200.)
            .linear(Point3::new(-4., 8., -2.), MoveKind::Cutting)
            .build();
        let mut tools = tools();
        tools.push(SimTool {
            number: 2,
            ..tools[0]
        });
        let samples = analyze_removal_controlled(
            &program,
            [0., 0., -5.],
            [20., 20., 0.],
            &options(),
            &tools,
            None,
            &|| false,
            |_, _| {},
        )
        .unwrap();
        let returning: Vec<_> = samples.iter().filter(|s| s.step_index == 7).collect();
        assert!(!returning.is_empty());
        assert!(returning
            .iter()
            .all(|s| s.tool == Some(2) && s.spindle_rpm == 20000.));
        assert!((returning.iter().map(|s| s.seconds).sum::<f64>() - 1.45).abs() < 1e-10);
    }

    #[test]
    fn half_overlap_and_inherited_pockets_reduce_removed_material() {
        let program = slot()
            .linear(Point3::new(25., 10., -2.), MoveKind::Link)
            .op(1)
            .linear(Point3::new(-4., 10., -2.), MoveKind::Cutting)
            .build();
        let samples = analyze(&program, None);
        let first: f64 = samples
            .iter()
            .filter(|s| s.step_index == 4)
            .map(|s| s.removed_volume_mm3)
            .sum();
        let second: f64 = samples
            .iter()
            .filter(|s| s.step_index == 6)
            .map(|s| s.removed_volume_mm3)
            .sum();
        assert!((second / first - 0.5).abs() < 0.02, "{first} / {second}");
        let mut initial = Heightfield::new([0., 0.], [20., 20.], 0.1, 0.);
        initial.lower_rect([0., 0.], [10., 20.], -2.);
        let seeded = analyze(&slot().build(), Some(&initial));
        let seeded_volume: f64 = seeded.iter().map(|s| s.removed_volume_mm3).sum();
        assert!((seeded_volume / first - 0.5).abs() < 0.02);
        assert_eq!(
            initial.sample(15., 8.),
            0.,
            "analysis must not mutate the seed"
        );
    }

    #[test]
    fn through_cuts_and_sparse_voids_are_counted_once_with_partial_edge_cells() {
        let mut field = Heightfield::new([0., 0.], [1.05, 1.05], 0.5, 0.);
        field.start_removal_tracking(-1.);
        let p = [0.5, 0.5, -4.];
        field.cut_segment_profile(p, p, &ToolProfile::flat(3.));
        assert!((field.tracked_removed_volume() - 1.05 * 1.05).abs() < 1e-10);
        let mut field = Heightfield::new([0., 0.], [10., 10.], 0.1, 0.);
        field.start_removal_tracking(-5.);
        let thread = ToolProfile {
            radius: 2.,
            shape: ProfileShape::Thread {
                root_radius: 0.5,
                tooth_height: 2.,
                pitch: 2.,
                tooth_count: 1,
            },
        };
        let p = [5., 5., -4.];
        field.cut_segment_profile(p, p, &thread);
        assert!(field.tracked_removed_volume() > 0.);
        let mut seeded = field.clone();
        seeded.start_removal_tracking(-5.);
        let p = [5., 5., -8.];
        seeded.cut_segment_profile(p, p, &ToolProfile::flat(2.));
        field.cut_segment_profile(p, p, &ToolProfile::flat(2.));
        let mut flat = Heightfield::new([0., 0.], [10., 10.], 0.1, 0.);
        flat.start_removal_tracking(-5.);
        flat.cut_segment_profile(p, p, &ToolProfile::flat(2.));
        assert!((field.tracked_removed_volume() - flat.tracked_removed_volume()).abs() < 1e-7);
        assert!(seeded.tracked_removed_volume() < flat.tracked_removed_volume());
        let before = field.tracked_removed_volume();
        field.cut_segment_profile(p, p, &ToolProfile::flat(2.));
        assert_eq!(field.tracked_removed_volume(), before);
    }

    #[test]
    fn clockwise_and_counterclockwise_helices_preserve_time_and_removal() {
        for dir in [ArcDir::Cw, ArcDir::Ccw] {
            let end = Point3::new(10., if dir == ArcDir::Cw { 12. } else { 8. }, -2.);
            let program = ProgramBuilder::new()
                .tool_change(1)
                .spindle_on(10000., SpindleDir::Cw)
                .feed(600.)
                .rapid(Point3::new(8., 10., 3.), MoveKind::Link)
                .linear(Point3::new(8., 10., -1.), MoveKind::Plunge)
                .arc(end, Point3::new(10., 10., -1.), dir, MoveKind::Cutting)
                .build();
            let samples = analyze(&program, None);
            let arc: Vec<_> = samples.iter().filter(|s| s.step_index == 4).collect();
            let seconds: f64 = arc.iter().map(|s| s.seconds).sum();
            assert!((seconds - std::f64::consts::PI.hypot(1.) / 10.).abs() < 1e-10);
            assert_eq!(arc.last().unwrap().end_fraction, 1.);
            let reference = simulate_controlled(
                &program,
                [0., 0., -5.],
                [20., 20., 0.],
                &options(),
                &tools(),
                &|| false,
                |_, _| {},
            )
            .unwrap();
            let volume: f64 = samples.iter().map(|s| s.removed_volume_mm3).sum();
            assert!(
                (volume / reference.removed_volume - 1.).abs() < 0.08,
                "{dir:?}: {volume} vs {}",
                reference.removed_volume
            );
        }
    }

    #[test]
    fn cancelled_traversals_publish_no_partial_trace_and_collisions_taint_following_samples() {
        let polls = Cell::new(0);
        assert!(analyze_removal_controlled(
            &slot().build(),
            [0., 0., -5.],
            [20., 20., 0.],
            &options(),
            &tools(),
            None,
            &|| {
                polls.set(polls.get() + 1);
                polls.get() > 20
            },
            |_, _| {}
        )
        .is_err());
        let program = ProgramBuilder::new()
            .tool_change(1)
            .feed(600.)
            .rapid(Point3::new(5., 5., -1.), MoveKind::Link)
            .rapid(Point3::new(10., 5., -1.), MoveKind::Link)
            .linear(Point3::new(15., 5., -1.), MoveKind::Cutting)
            .build();
        assert!(analyze(&program, None).iter().all(|s| s.collision));
    }
}
