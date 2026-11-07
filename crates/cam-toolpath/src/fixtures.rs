//! Continuous checks against conservative fixture and whole-tool envelopes.
//! Fixtures are static solids: cutting moves can never remove them.
use crate::CancelToken;
use cam_cldata::{CutterComp, Point3, Program, Step, Tag};
use cam_model::{FixtureEnvelope, Tool};

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureContact {
    pub fixture_id: u64,
    pub fixture_name: String,
    pub at: [f64; 3],
    pub tag: Tag,
    pub step_index: usize,
    pub fraction: f64,
}

// Segment/box slab intersection, including the complete swept tool cylinder's
// bounding box. No distance sampling, so even thin fixtures cannot be skipped.
fn contact(
    a: [f64; 3],
    b: [f64; 3],
    fixture: &FixtureEnvelope,
    radius: f64,
    length: f64,
    curve_error: f64,
) -> Option<f64> {
    let min = [
        fixture.min[0] - radius - curve_error,
        fixture.min[1] - radius - curve_error,
        fixture.min[2] - length,
    ];
    let max = [
        fixture.max[0] + radius + curve_error,
        fixture.max[1] + radius + curve_error,
        fixture.max[2],
    ];
    slab_interval(a, b, min, max).map(|(enter, _)| enter)
}

fn slab_interval(a: [f64; 3], b: [f64; 3], min: [f64; 3], max: [f64; 3]) -> Option<(f64, f64)> {
    let (mut enter, mut exit) = (0.0_f64, 1.0_f64);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 1e-14 {
            if a[i] <= min[i] + 1e-9 || a[i] >= max[i] - 1e-9 {
                return None;
            }
        } else {
            let t0 = (min[i] - a[i]) / d;
            let t1 = (max[i] - a[i]) / d;
            enter = enter.max(t0.min(t1));
            exit = exit.min(t0.max(t1));
            if enter >= exit - 1e-12 {
                return None;
            }
        }
    }
    Some((enter, exit))
}
fn xyz(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn tool_envelope(tool: &Tool) -> Result<(f64, f64), String> {
    let mut radius = tool.diameter.max(tool.shank_dia()).max(tool.neck_dia()) / 2.;
    let mut length = tool.length;
    if !radius.is_finite() || radius <= 0. || !length.is_finite() || length <= 0. {
        return Err(format!(
            "Fixture verification requires positive finite dimensions for tool {}",
            tool.number
        ));
    }
    // Thread teeth and pointed tools can extend beyond the nominal stickout.
    // Include every generated profile segment, padding arcs to their full circle.
    let profile = tool.profile();
    let mut start = profile.start;
    for segment in profile.segs {
        radius = radius.max(segment.end.x);
        length = length.max(segment.end.y);
        if let cam_geo::SegShape::Arc { center, .. } = segment.shape {
            let r = (start.x - center.x).hypot(start.y - center.y);
            radius = radius.max(center.x + r);
            length = length.max(center.y + r);
        }
        start = segment.end;
    }
    if !radius.is_finite() || !length.is_finite() {
        return Err("Fixture tool envelope exceeds finite coordinates".into());
    }
    Ok((radius, length))
}

/// All motions, including tool changes, arcs/helices, patterns and drilling links,
/// are checked using the selected tool's cutting, neck and shank dimensions.
/// Boxes deliberately protect fixture voids and rounded tool corners as well.
pub fn verify_fixture_clearance(
    program: &Program,
    tools: &[Tool],
    fixtures: &[FixtureEnvelope],
    cancel: &CancelToken,
) -> Result<Vec<FixtureContact>, String> {
    if fixtures.is_empty() {
        return Ok(Vec::new());
    }
    if program.has_rotary_motion() {return Err("Fixture clearance verification is unavailable for rotary motion".into());}
    program.validate().map_err(|e| e.to_string())?;
    if fixtures.iter().any(|f| {
        (0..3).any(|i| !f.min[i].is_finite() || !f.max[i].is_finite() || f.min[i] > f.max[i])
    }) {
        return Err("Fixture verification requires finite, ordered fixture envelopes".into());
    }
    let mut envelopes = std::collections::BTreeMap::new();
    let mut contacts = Vec::new();
    let mut cur = None;
    let mut tool = tools.first();
    let mut compensation = false;
    let mut last_tag = None;
    for (index, step) in program.steps().iter().enumerate() {
        if cancel.is_cancelled() {
            return Err("Fixture verification cancelled".into());
        }
        match step {
            Step::ToolChange { tool: number } => {
                tool = Some(
                    tools
                        .iter()
                        .find(|t| t.number == *number)
                        .ok_or_else(|| format!("Fixture verification: missing tool {number}"))?,
                );
            }
            Step::Datum { .. } => cur = None,
            Step::CutterComp(value) => compensation = *value != CutterComp::Off,
            _ => {}
        }
        let Some(tag) = step
            .tag()
            .or(last_tag.filter(|_| matches!(step, Step::ToolChange { .. })))
        else {
            continue;
        };
        last_tag = Some(tag);
        let tool = tool.ok_or("Fixture verification requires a defined tool")?;
        if let std::collections::btree_map::Entry::Vacant(entry) = envelopes.entry(tool.number) {
            entry.insert(tool_envelope(tool)?);
        }
        let (radius, length) = envelopes[&tool.number];
        let radius = if compensation {
            radius + tool.diameter / 2.
        } else {
            radius
        };
        let mut hit_fixtures = std::collections::BTreeSet::new();
        let mut check = |a: [f64; 3], b: [f64; 3], range: (f64, f64), error: f64| {
            for fixture in fixtures {
                if hit_fixtures.contains(&fixture.instance_id) {
                    continue;
                }
                if let Some(t) = contact(a, b, fixture, radius, length, error) {
                    hit_fixtures.insert(fixture.instance_id);
                    contacts.push(FixtureContact {
                        fixture_id: fixture.instance_id,
                        fixture_name: fixture.name.clone(),
                        at: std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t),
                        tag,
                        step_index: index,
                        fraction: range.0 + (range.1 - range.0) * t,
                    });
                }
            }
        };
        match step {
            Step::ToolChange { .. } => {
                if let Some(p) = cur {
                    check(p, p, (0., 1.), 0.);
                }
            }
            Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                let b = xyz(*to);
                check(cur.unwrap_or(b), b, (0., 1.), 0.);
                cur = Some(b);
            }
            Step::Arc {
                end, center, dir, ..
            } => {
                if let Some(a) = cur {
                    let start = Point3::new(a[0], a[1], a[2]);
                    let sweep = cam_cldata::arc_sweep(start, *end, *center, *dir)
                        * if *dir == cam_cldata::ArcDir::Cw {
                            -1.
                        } else {
                            1.
                        };
                    let r = (a[0] - center.x).hypot(a[1] - center.y);
                    let angle = (a[1] - center.y).atan2(a[0] - center.x);
                    // Chord sagitta is <= 0.005 mm; pad each chord by that
                    // bound, keeping checks conservative even at arc extrema.
                    let error = 0.005_f64.min(r);
                    let increment = (2. * (1. - error / r).clamp(-1., 1.).acos())
                        .min(std::f64::consts::FRAC_PI_2);
                    let n = (sweep.abs() / increment).ceil().max(1.) as usize;
                    let mut last = a;
                    for i in 1..=n {
                        if cancel.is_cancelled() {
                            return Err("Fixture verification cancelled".into());
                        }
                        let t = i as f64 / n as f64;
                        let p = if i == n {
                            xyz(*end)
                        } else {
                            [
                                center.x + r * (angle + sweep * t).cos(),
                                center.y + r * (angle + sweep * t).sin(),
                                a[2] + (end.z - a[2]) * t,
                            ]
                        };
                        check(last, p, ((i - 1) as f64 / n as f64, t), error);
                        last = p;
                    }
                }
                cur = Some(xyz(*end));
            }
            Step::Drill(cycle) => {
                for &[x, y] in &cycle.points {
                    if cancel.is_cancelled() {
                        return Err("Fixture verification cancelled".into());
                    }
                    let top = [x, y, cycle.retract];
                    let bottom = [x, y, cycle.depth];
                    check(cur.unwrap_or(top), top, (0., 1.), 0.);
                    check(top, bottom, (0., 1.), 0.);
                    check(bottom, top, (0., 1.), 0.);
                    cur = Some(top);
                }
            }
            _ => {}
        }
    }
    Ok(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{ArcDir, DrillCycle, MoveKind};
    fn fixture(min: [f64; 3], max: [f64; 3]) -> FixtureEnvelope {
        FixtureEnvelope {
            instance_id: 12,
            name: "vise jaw".into(),
            min,
            max,
        }
    }
    fn tool() -> Tool {
        Tool {
            number: 1,
            diameter: 2.,
            length: 10.,
            ..Default::default()
        }
    }
    fn tag() -> Tag {
        Tag {
            op_id: 7,
            kind: MoveKind::Cutting,
            pattern_id: Some(9),
            instance_index: Some(2),
        }
    }
    fn rapid(p: [f64; 3]) -> Step {
        Step::Rapid {
            to: Point3::new(p[0], p[1], p[2]),
            tag: tag(),
        }
    }
    fn check(steps: Vec<Step>, fixtures: &[FixtureEnvelope]) -> Vec<FixtureContact> {
        verify_fixture_clearance(&Program { steps }, &[tool()], fixtures, &CancelToken::new())
            .unwrap()
    }
    #[test]
    fn fixture_sweeps_catch_thin_obstacles_and_preserve_provenance() {
        let hits = check(
            vec![rapid([-100., 0., 0.]), rapid([100., 0., 0.])],
            &[fixture([0., -0.01, -1.], [0.001, 0.01, 1.])],
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].step_index, 1);
        assert_eq!(hits[0].tag, tag());
        assert!((hits[0].at[0] + 1.).abs() < 1e-8);
        assert!((hits[0].fraction - 0.495).abs() < 1e-8);
    }
    #[test]
    fn fixture_sweeps_include_shank_above_tip_and_accept_clearance_boundary() {
        let fixtures = [fixture([-1., -1., 5.], [1., 1., 6.])];
        assert!(!check(vec![rapid([-5., 0., 0.]), rapid([5., 0., 0.])], &fixtures).is_empty());
        assert!(check(vec![rapid([-5., 0., 6.]), rapid([5., 0., 6.])], &fixtures).is_empty());
        let mut t = tool();
        t.shank_diameter = 8.;
        let program = Program {
            steps: vec![rapid([4., 0., 0.])],
        };
        assert_eq!(
            verify_fixture_clearance(&program, &[t], &fixtures, &CancelToken::new())
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn fixture_sweeps_follow_clockwise_and_counterclockwise_arcs_and_helices() {
        for (dir, y) in [(ArcDir::Ccw, 10.), (ArcDir::Cw, -10.)] {
            let steps = vec![
                rapid([10., 0., 0.]),
                Step::Arc {
                    end: Point3::new(-10., 0., 2.),
                    center: Point3::new(0., 0., 0.),
                    dir,
                    feed: 100.,
                    tag: tag(),
                },
            ];
            let fixtures = [fixture([-0.01, y - 0.01, 1.], [0.01, y + 0.01, 2.])];
            let hits = check(steps, &fixtures);
            assert_eq!(hits.len(), 1, "{dir:?}");
            assert_eq!(hits[0].step_index, 1);
            assert!(hits[0].fraction > 0.4 && hits[0].fraction < 0.6);
        }
    }
    #[test]
    fn fixture_sweeps_check_drill_links_and_clearance_envelopes_do_not_get_cut() {
        let cycle = DrillCycle {
            points: vec![[0., 0.], [20., 0.]],
            z_top: 0.,
            depth: -2.,
            retract: 5.,
            feed: 100.,
            peck: Some(1.),
            dwell: None,
            tag: tag(),
        };
        let hits = check(
            vec![Step::Drill(cycle)],
            &[fixture([9., -1., 3.], [11., 1., 7.])],
        );
        assert_eq!(hits.len(), 1);
        let steps = vec![
            rapid([-5., 0., 0.]),
            Step::Linear {
                to: Point3::new(5., 0., 0.),
                feed: 100.,
                tag: tag(),
            },
            rapid([-5., 0., 0.]),
        ];
        assert_eq!(
            check(steps, &[fixture([0., -1., -1.], [1., 1., 1.])]).len(),
            2
        );
    }
    #[test]
    fn fixture_envelope_covers_profile_geometry_beyond_nominal_length() {
        let mut t = tool();
        t.kind = cam_model::ToolKind::ThreadMill { pitch: None };
        t.flute_length = 20.;
        t.neck_diameter = 1.;
        let fixtures = [fixture([-1., -1., 15.], [1., 1., 16.])];
        let program = Program {
            steps: vec![rapid([0., 0., 0.])],
        };
        assert_eq!(
            verify_fixture_clearance(&program, &[t], &fixtures, &CancelToken::new())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fixture_sweeps_check_tool_changes_and_cancel_without_partial_results() {
        let mut wide = tool();
        wide.number = 2;
        wide.shank_diameter = 10.;
        let p = Program {
            steps: vec![rapid([4., 0., 0.]), Step::ToolChange { tool: 2 }],
        };
        let fixtures = [fixture([0., -1., 2.], [1., 1., 3.])];
        let hits =
            verify_fixture_clearance(&p, &[tool(), wide], &fixtures, &CancelToken::new()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].step_index, 1);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(verify_fixture_clearance(&p, &[tool()], &fixtures, &cancel).is_err());
    }
}

#[path = "fixture_routing.rs"]
mod routing;
pub(crate) use routing::avoid_profile_fixtures;
/// Invalidates persisted fixture paths when routing behavior changes.
pub const FIXTURE_ROUTING_REVISION: u32 = 1;
