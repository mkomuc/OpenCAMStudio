//! Trim protected portions of supported profile paths after pattern expansion.
//! Reconnect the surviving paths at fixture clearance, preserving exact arcs.
use super::{slab_interval, tool_envelope};
use crate::{CancelToken, Diagnostic, StrategyResult};
use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_model::{FixtureEnvelope, Tool};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

// Larger than verification's conservative 0.005 mm arc-chord padding.
const MARGIN: f64 = 0.02;
type Owner = (u32, Option<u64>, Option<u32>);
fn owner(tag: Tag) -> Owner {
    (tag.op_id, tag.pattern_id, tag.instance_index)
}
fn array(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn near(a: Point3, b: Point3) -> bool {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.z - b.z).abs())
        < 1e-8
}

#[derive(Clone, Copy)]
struct Motion<'a> {
    from: Point3,
    step: &'a Step,
    end: Point3,
}
impl Motion<'_> {
    fn at(&self, t: f64) -> Point3 {
        if t == 0. {
            return self.from;
        }
        if t == 1. {
            return self.end;
        }
        if let Step::Arc { center, .. } = self.step {
            let angle = (self.from.y - center.y).atan2(self.from.x - center.x) + self.sweep() * t;
            let radius = (self.from.x - center.x).hypot(self.from.y - center.y);
            Point3::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
                self.from.z + (self.end.z - self.from.z) * t,
            )
        } else {
            Point3::new(
                self.from.x + (self.end.x - self.from.x) * t,
                self.from.y + (self.end.y - self.from.y) * t,
                self.from.z + (self.end.z - self.from.z) * t,
            )
        }
    }
    fn sweep(&self) -> f64 {
        match self.step {
            Step::Arc { center, dir, .. } => {
                cam_cldata::arc_sweep(self.from, self.end, *center, *dir)
                    * if *dir == ArcDir::Cw { -1. } else { 1. }
            }
            _ => 0.,
        }
    }
    fn piece(&self, lo: f64, hi: f64) -> Step {
        let mut step = self.step.clone();
        let end = self.at(hi);
        match &mut step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => *to = end,
            Step::Arc { end: to, .. } => *to = end,
            _ => unreachable!(),
        }
        // Very short arc pieces can round to coincident XY endpoints, which
        // the IR would interpret as an unintended full circle.
        if let Step::Arc { feed, tag, .. } = &step {
            let start = self.at(lo);
            if (start.x - end.x).hypot(start.y - end.y) < 1e-7
                && self.sweep().abs() * (hi - lo) < 1e-5
            {
                return Step::Linear {
                    to: end,
                    feed: *feed,
                    tag: *tag,
                };
            }
        }
        step
    }
}

/// Exact parameter intervals inside a tool-expanded fixture box. Arc roots on
/// all six box planes partition the circle/helix; midpoint tests then classify
/// entire intervals, so a thin obstacle or a full circle cannot be missed.
fn blocked(m: Motion<'_>, min: [f64; 3], max: [f64; 3]) -> Vec<(f64, f64)> {
    let Step::Arc { center, .. } = m.step else {
        return slab_interval(array(m.from), array(m.end), min, max)
            .into_iter()
            .collect();
    };
    let radius = (m.from.x - center.x).hypot(m.from.y - center.y);
    let start = (m.from.y - center.y).atan2(m.from.x - center.x);
    let sweep = m.sweep();
    let mut ts = vec![0., 1.];
    let mut root = |angle: f64| {
        let t = ((angle - start) * sweep.signum()).rem_euclid(TAU) / sweep.abs();
        if t > 0. && t < 1. {
            ts.push(t);
        }
    };
    for x in [min[0], max[0]] {
        let c = (x - center.x) / radius;
        if (-1. ..=1.).contains(&c) {
            let a = c.acos();
            root(a);
            root(-a);
        }
    }
    for y in [min[1], max[1]] {
        let c = (y - center.y) / radius;
        if (-1. ..=1.).contains(&c) {
            let a = c.asin();
            root(a);
            root(std::f64::consts::PI - a);
        }
    }
    if (m.end.z - m.from.z).abs() > 1e-14 {
        for z in [min[2], max[2]] {
            let t = (z - m.from.z) / (m.end.z - m.from.z);
            if t > 0. && t < 1. {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    ts.windows(2)
        .filter_map(|w| {
            let p = array(m.at((w[0] + w[1]) * 0.5));
            (0..3)
                .all(|i| p[i] >= min[i] && p[i] <= max[i])
                .then_some((w[0], w[1]))
        })
        .collect()
}
fn free_intervals(
    m: Motion<'_>,
    fixtures: &[FixtureEnvelope],
    radius: f64,
    length: f64,
) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    for f in fixtures {
        let min = [
            f.min[0] - radius - MARGIN,
            f.min[1] - radius - MARGIN,
            f.min[2] - length - MARGIN,
        ];
        let max = [
            f.max[0] + radius + MARGIN,
            f.max[1] + radius + MARGIN,
            f.max[2] + MARGIN,
        ];
        intervals.extend(blocked(m, min, max));
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut free = Vec::new();
    let mut from = 0.0_f64;
    for (lo, hi) in intervals {
        if lo > from + 1e-10 {
            free.push((from, lo));
        }
        from = from.max(hi);
    }
    if from < 1. - 1e-10 {
        free.push((from, 1.));
    }
    free
}
fn reconnect(
    out: &mut Program,
    current: Option<Point3>,
    start: Point3,
    clearance: f64,
    plunge_feed: f64,
    tag: Tag,
) {
    if current.is_some_and(|p| near(p, start)) {
        return;
    }
    let z = clearance
        .max(start.z)
        .max(current.map_or(clearance, |p| p.z));
    if let Some(p) = current {
        if p.z < z - 1e-9 {
            out.push(Step::Rapid {
                to: Point3::new(p.x, p.y, z),
                tag: Tag {
                    kind: MoveKind::Retract,
                    ..tag
                },
            });
        }
    }
    out.push(Step::Rapid {
        to: Point3::new(start.x, start.y, z),
        tag: Tag {
            kind: MoveKind::Link,
            ..tag
        },
    });
    if start.z < z - 1e-9 {
        out.push(Step::Linear {
            to: start,
            feed: plunge_feed,
            tag: Tag {
                kind: MoveKind::Plunge,
                ..tag
            },
        });
    }
}

/// `profiles` contains only computed-compensation profiles that permit straight
/// re-entry without tabs. All other operations remain subject to verification.
/// The caller verifies the complete rewritten program, including every escape
/// and re-entry, before accepting it. This does not certify machine/holder travel.
pub(crate) fn avoid_profile_fixtures(
    program: &Program,
    profiles: &BTreeMap<u32, f64>,
    tools: &[Tool],
    fixtures: &[FixtureEnvelope],
    clearance: f64,
    cancel: &CancelToken,
) -> Result<StrategyResult, String> {
    if fixtures.is_empty() || profiles.is_empty() {
        return Ok(StrategyResult {
            program: program.clone(),
            ..Default::default()
        });
    }
    program.validate().map_err(|e| e.to_string())?;
    let clearance = fixtures
        .iter()
        .map(|f| f.max[2] + MARGIN * 2.)
        .fold(clearance, f64::max);
    let mut source = None;
    let mut actual = None;
    let mut tool = tools.first();
    let mut envelopes = BTreeMap::new();
    let mut out = Program::new();
    let mut excluded = BTreeMap::<Owner, usize>::new();
    let mut retained = BTreeMap::<Owner, usize>::new();
    let mut omitted_cuts = BTreeMap::<Owner, usize>::new();
    for step in program.steps() {
        if cancel.is_cancelled() {
            return Err("Fixture avoidance cancelled".into());
        }
        match step {
            Step::ToolChange { tool: number } => tool = tools.iter().find(|t| t.number == *number),
            Step::Datum(_) => {
                source = None;
                actual = None;
            }
            Step::Drill(cycle) => {
                if let Some(&[x, y]) = cycle.points.last() {
                    source = Some(Point3::new(x, y, cycle.retract));
                    actual = source;
                }
            }
            _ => {}
        }
        let end = match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
            Step::Arc { end, .. } => Some(*end),
            _ => None,
        };
        let (Some(end), Some(tag)) = (end, step.tag()) else {
            out.push(step.clone());
            continue;
        };
        let motion = Motion {
            from: source.unwrap_or(end),
            step,
            end,
        };
        source = Some(end);
        let Some(&plunge_feed) = profiles.get(&tag.op_id) else {
            out.push(step.clone());
            actual = Some(end);
            continue;
        };
        let tool = tool.ok_or("Fixture avoidance requires the selected tool")?;
        if let std::collections::btree_map::Entry::Vacant(e) = envelopes.entry(tool.number) {
            e.insert(tool_envelope(tool)?);
        }
        let (radius, length) = envelopes[&tool.number];
        let free = free_intervals(motion, fixtures, radius, length);
        let cut = tag.kind == MoveKind::Cutting;
        let changed = free.len() != 1 || free.first().is_none_or(|r| r.0 != 0. || r.1 != 1.);
        if changed {
            *excluded.entry(owner(tag)).or_default() += 1;
            if cut {
                *omitted_cuts.entry(owner(tag)).or_default() += 1;
            }
        }
        for (lo, hi) in free {
            let start = motion.at(lo);
            if actual.is_none() && lo == 0. && matches!(step, Step::Rapid { .. }) {
                // Preserve the planner's first machine-position anchor.
            } else {
                reconnect(&mut out, actual, start, clearance, plunge_feed, tag);
            }
            out.push(motion.piece(lo, hi));
            actual = Some(motion.at(hi));
            if cut {
                *retained.entry(owner(tag)).or_default() += 1;
            }
        }
        if out.len() > cam_model::MAX_PROFILE_MOVES {
            return Err("Fixture avoidance exceeds the profile motion budget".into());
        }
    }
    let mut diagnostics = Vec::new();
    for (key, count) in excluded {
        let omitted = omitted_cuts.get(&key).copied().unwrap_or(0);
        let mut d = if omitted == 0 {
            Diagnostic::warning(format!("Fixture clearance rerouted {count} non-cutting profile motion section(s) above the fixtures."))
        } else if retained.get(&key).copied().unwrap_or(0) == 0 {
            Diagnostic::error("Fixture clearances block all cutting passes of this profile instance. Move the fixture or choose another setup.")
        } else {
            Diagnostic::warning(format!("Fixture clearance excluded {omitted} profile cutting section(s). The remaining paths reconnect above the fixtures; protected material requires another setup."))
        }.for_op(key.0);
        if let (Some(pattern), Some(instance)) = (key.1, key.2) {
            d = d.for_pattern(pattern, instance);
        }
        diagnostics.push(d);
    }
    Ok(StrategyResult {
        program: out,
        diagnostics,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify_fixture_clearance;
    fn tool() -> Tool {
        Tool {
            number: 1,
            diameter: 2.,
            length: 10.,
            ..Default::default()
        }
    }
    fn fixture(min: [f64; 3], max: [f64; 3]) -> FixtureEnvelope {
        FixtureEnvelope {
            instance_id: 1,
            name: "clamp".into(),
            min,
            max,
        }
    }
    fn tag(kind: MoveKind) -> Tag {
        Tag::new(7, kind)
    }
    fn rapid(x: f64, y: f64, z: f64) -> Step {
        Step::Rapid {
            to: Point3::new(x, y, z),
            tag: tag(MoveKind::Link),
        }
    }
    fn line(x: f64, y: f64, z: f64, kind: MoveKind) -> Step {
        Step::Linear {
            to: Point3::new(x, y, z),
            feed: 100.,
            tag: tag(kind),
        }
    }
    fn route(steps: Vec<Step>, fixtures: &[FixtureEnvelope]) -> StrategyResult {
        avoid_profile_fixtures(
            &Program { steps },
            &BTreeMap::from([(7, 60.)]),
            &[tool()],
            fixtures,
            5.,
            &CancelToken::new(),
        )
        .unwrap()
    }
    fn clean(result: &StrategyResult, fixtures: &[FixtureEnvelope]) {
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let contacts =
            verify_fixture_clearance(&result.program, &[tool()], fixtures, &CancelToken::new())
                .unwrap();
        assert!(contacts.is_empty(), "{contacts:?}");
    }
    #[test]
    fn fixture_routing_splits_a_profile_and_retracts_across_a_thin_clamp() {
        let fixtures = [fixture([-0.001, -1., -1.], [0.001, 1., 2.])];
        let result = route(
            vec![
                rapid(-10., 0., 10.),
                line(-10., 0., 0., MoveKind::Plunge),
                line(10., 0., 0., MoveKind::Cutting),
                rapid(10., 0., 10.),
            ],
            &fixtures,
        );
        clean(&result, &fixtures);
        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.program.steps().iter().any(
            |s| matches!(s,Step::Rapid { to,tag } if tag.kind==MoveKind::Retract && to.z>=5.)
        ));
        let cuts: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter(|s| s.tag().is_some_and(|t| t.kind == MoveKind::Cutting))
            .collect();
        assert_eq!(cuts.len(), 2);
        assert!(result.program.steps().iter().any(|s|matches!(s,Step::Linear { to,feed,tag } if tag.kind==MoveKind::Plunge && to.x>1. && *feed==60.)));
    }
    #[test]
    fn fixture_routing_preserves_exact_clockwise_counterclockwise_and_helical_arcs() {
        for dir in [ArcDir::Cw, ArcDir::Ccw] {
            let fixtures = [fixture([-0.01, 9.9, -1.], [0.01, 10.1, 1.])];
            let result = route(
                vec![
                    rapid(10., 0., 10.),
                    line(10., 0., 0., MoveKind::Plunge),
                    Step::Arc {
                        end: Point3::new(10., 0., -2.),
                        center: Point3::new(0., 0., 0.),
                        dir,
                        feed: 100.,
                        tag: tag(MoveKind::Cutting),
                    },
                    rapid(10., 0., 10.),
                ],
                &fixtures,
            );
            clean(&result, &fixtures);
            let arcs: Vec<_> = result
                .program
                .steps()
                .iter()
                .filter_map(|s| {
                    if let Step::Arc {
                        center,
                        dir: d,
                        end,
                        ..
                    } = s
                    {
                        assert_eq!(*d, dir);
                        assert_eq!(*center, Point3::new(0., 0., 0.));
                        Some(end)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(arcs.len(), 2);
            assert_eq!(**arcs.last().unwrap(), Point3::new(10., 0., -2.));
        }
    }
    #[test]
    fn fixture_routing_merges_overlapping_clamp_envelopes() {
        let mut fixtures = [
            fixture([-2., -1., -1.], [1., 1., 1.]),
            fixture([0., -1., -1.], [3., 1., 1.]),
        ];
        fixtures[1].instance_id = 2;
        let result = route(
            vec![
                rapid(-10., 0., 10.),
                line(-10., 0., 0., MoveKind::Plunge),
                line(10., 0., 0., MoveKind::Cutting),
                rapid(10., 0., 10.),
            ],
            &fixtures,
        );
        clean(&result, &fixtures);
        assert_eq!(
            result
                .program
                .steps()
                .iter()
                .filter(|s| s.tag().is_some_and(|t| t.kind == MoveKind::Cutting))
                .count(),
            2
        );
    }
    #[test]
    fn fixture_routing_reports_fully_blocked_pattern_instances_with_their_ownership() {
        let fixtures = [fixture([-20., -5., -1.], [20., 5., 1.])];
        let mut steps = vec![
            rapid(-10., 0., 10.),
            line(-10., 0., 0., MoveKind::Plunge),
            line(10., 0., 0., MoveKind::Cutting),
            rapid(10., 0., 10.),
        ];
        for step in &mut steps {
            match step {
                Step::Rapid { tag, .. } | Step::Linear { tag, .. } => {
                    tag.pattern_id = Some(9);
                    tag.instance_index = Some(2);
                }
                _ => {}
            }
        }
        let result = route(steps, &fixtures);
        assert!(result.has_errors());
        assert_eq!(result.diagnostics[0].source.pattern_id, Some(9));
        assert_eq!(result.diagnostics[0].source.instance_index, Some(2));
        assert!(verify_fixture_clearance(
            &result.program,
            &[tool()],
            &fixtures,
            &CancelToken::new()
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn fixture_routing_keeps_pattern_cuts_when_an_unowned_traverse_needs_a_detour() {
        let fixtures = [fixture([-1., -1., -1.], [1., 1., 1.])];
        let mut steps = vec![
            rapid(-10., 0., 10.),
            line(-10., 0., 0., MoveKind::Plunge),
            line(10., 0., 0., MoveKind::Cutting),
        ];
        for step in &mut steps {
            match step {
                Step::Rapid { tag, .. } | Step::Linear { tag, .. } => {
                    tag.pattern_id = Some(9);
                    tag.instance_index = Some(2);
                }
                _ => {}
            }
        }
        steps.push(Step::Rapid {
            to: Point3::new(-10., 0., 0.),
            tag: tag(MoveKind::Traverse),
        });
        steps.push(rapid(-10., 0., 10.));
        let result = route(steps, &fixtures);
        clean(&result, &fixtures);
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.source.pattern_id == Some(9)));
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.source.pattern_id.is_none() && d.message.contains("non-cutting")));
    }

    #[test]
    fn fixture_routing_leaves_unobstructed_motion_unchanged_and_honors_cancellation() {
        let fixtures = [fixture([20., 20., -1.], [21., 21., 1.])];
        let steps = vec![
            rapid(-10., 0., 10.),
            line(-10., 0., 0., MoveKind::Plunge),
            line(10., 0., 0., MoveKind::Cutting),
            rapid(10., 0., 10.),
        ];
        let result = route(steps.clone(), &fixtures);
        assert_eq!(result.program.steps, steps);
        assert!(result.diagnostics.is_empty());
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(avoid_profile_fixtures(
            &result.program,
            &BTreeMap::from([(7, 60.)]),
            &[tool()],
            &fixtures,
            5.,
            &cancel
        )
        .is_err());
    }
}
