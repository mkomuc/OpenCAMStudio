//! Fixtures are attached to the workpiece. Sweep the whole cutter in their
//! stationary material frame, padding sampled poses by a bound on motion.
use super::*;

pub(super) fn verify(
    program: &Program,
    tools: &[Tool],
    fixtures: &[FixtureEnvelope],
    cancel: &CancelToken,
) -> Result<Vec<FixtureContact>, String> {
    let (axis, center) = program
        .steps()
        .iter()
        .find_map(|s| {
            if let Step::Rotary { axis, center, .. } = s {
                Some((*axis, *center))
            } else {
                None
            }
        })
        .unwrap();
    if program.steps().iter().any(|s|matches!(s,Step::Rotary{axis:a,center:c,..} if *a!=axis || match axis {cam_cldata::RotaryAxis::A=>(c.y-center.y).hypot(c.z-center.z)>1e-6,cam_cldata::RotaryAxis::B=>(c.x-center.x).hypot(c.z-center.z)>1e-6})) {
        return Err("Rotary fixture checks require one physical axis and centre per setup".into());
    }
    let mut contacts = Vec::new();
    let mut current = None;
    let mut angle = 0.;
    let mut selected = tools.first();
    let mut compensation = false;
    let mut last_tag = None;
    for (index, step) in program.steps().iter().enumerate() {
        if cancel.is_cancelled() {
            return Err("Fixture verification cancelled".into());
        }
        match step {
            Step::ToolChange { tool } => {
                selected = Some(
                    tools
                        .iter()
                        .find(|t| t.number == *tool)
                        .ok_or_else(|| format!("Missing fixture verification tool {tool}"))?,
                )
            }
            Step::CutterComp(value) => compensation = *value != CutterComp::Off,
            Step::Datum(_) => current = None,
            _ => {}
        }
        let Some(tag) = step.tag().or({
            if matches!(step, Step::ToolChange { .. }) {
                last_tag
            } else {
                None
            }
        }) else {
            continue;
        };
        last_tag = Some(tag);
        let tool = selected.ok_or("Fixture verification requires a tool")?;
        let (mut radius, length) = tool_envelope(tool)?;
        if compensation {
            radius += tool.diameter * 0.5;
        }
        let mut hit = std::collections::BTreeSet::new();
        let mut check = |a: Point3,
                         b: Point3,
                         aa: f64,
                         ba: f64,
                         range: (f64, f64)|
         -> Result<(), String> {
            let distance = |p: Point3, q: Point3| (p.x - q.x).hypot(p.y - q.y).hypot(p.z - q.z);
            let reach = distance(a, center).max(distance(b, center)) + length + radius;
            let travel = distance(a, b) + (ba - aa).to_radians().abs() * reach;
            let count = (travel / 0.1).ceil().max(1.) as usize;
            if count > 1_000_000 {
                return Err("Rotary fixture sweep exceeds the verification budget".into());
            }
            let pad = travel / count as f64 * 0.5;
            for j in 0..=count {
                if cancel.is_cancelled() {
                    return Err("Fixture verification cancelled".into());
                }
                let t = j as f64 / count as f64;
                let tip = Point3::new(
                    a.x + (b.x - a.x) * t,
                    a.y + (b.y - a.y) * t,
                    a.z + (b.z - a.z) * t,
                );
                let angle = aa + (ba - aa) * t;
                let start = axis.material_point(tip, center, angle);
                let end =
                    axis.material_point(Point3::new(tip.x, tip.y, tip.z + length), center, angle);
                for fixture in fixtures {
                    if hit.contains(&fixture.instance_id) {
                        continue;
                    }
                    let direction =
                        std::array::from_fn::<_, 3, _>(|i| (xyz(end)[i] - xyz(start)[i]) / length);
                    let expansion = std::array::from_fn::<_, 3, _>(|i| {
                        radius * (1. - direction[i] * direction[i]).max(0.).sqrt()
                            + pad
                            + if matches!(step, Step::Arc { .. }) {
                                0.005
                            } else {
                                0.
                            }
                    });
                    let min = std::array::from_fn(|i| fixture.min[i] - expansion[i]);
                    let max = std::array::from_fn(|i| fixture.max[i] + expansion[i]);
                    if slab_interval(xyz(start), xyz(end), min, max).is_some() {
                        hit.insert(fixture.instance_id);
                        contacts.push(FixtureContact {
                            fixture_id: fixture.instance_id,
                            fixture_name: fixture.name.clone(),
                            at: xyz(start),
                            tag,
                            step_index: index,
                            fraction: range.0
                                + (range.1 - range.0) * ((j as f64 - 0.5) / count as f64).max(0.),
                        });
                    }
                }
            }
            Ok(())
        };
        match step {
            Step::ToolChange { .. } => {
                if let Some(p) = current {
                    check(p, p, angle, angle, (0., 1.))?;
                }
            }
            Step::Rotary {
                to, angle: next, ..
            } => {
                check(current.unwrap_or(*to), *to, angle, *next, (0., 1.))?;
                current = Some(*to);
                angle = *next;
            }
            Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                check(current.unwrap_or(*to), *to, angle, angle, (0., 1.))?;
                current = Some(*to);
            }
            Step::Arc {
                end, center, dir, ..
            } => {
                if let Some(a) = current {
                    let sweep = cam_cldata::arc_sweep(a, *end, *center, *dir)
                        * if *dir == cam_cldata::ArcDir::Cw {
                            -1.
                        } else {
                            1.
                        };
                    let r = (a.x - center.x).hypot(a.y - center.y);
                    let start = (a.y - center.y).atan2(a.x - center.x);
                    let n = (sweep.abs() * r / 0.01).ceil().max(1.) as usize;
                    if n > 1_000_000 {
                        return Err("Rotary fixture arc exceeds the verification budget".into());
                    }
                    let mut last = a;
                    for j in 1..=n {
                        let t = j as f64 / n as f64;
                        let p = if j == n {
                            *end
                        } else {
                            Point3::new(
                                center.x + r * (start + sweep * t).cos(),
                                center.y + r * (start + sweep * t).sin(),
                                a.z + (end.z - a.z) * t,
                            )
                        };
                        check(last, p, angle, angle, ((j - 1) as f64 / n as f64, t))?;
                        last = p;
                    }
                }
                current = Some(*end);
            }
            Step::Drill(cycle) => {
                for &[x, y] in &cycle.points {
                    let retract = Point3::new(x, y, cycle.retract);
                    let bottom = Point3::new(x, y, cycle.depth);
                    check(current.unwrap_or(retract), retract, angle, angle, (0., 1.))?;
                    check(retract, bottom, angle, angle, (0., 1.))?;
                    check(bottom, retract, angle, angle, (0., 1.))?;
                    current = Some(retract);
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
    #[test]
    fn angular_sweep_hits_a_fixture_between_clear_endpoints() {
        let step = |angle| Step::Rotary {
            to: Point3::new(0., 0., 6.),
            angle,
            axis: cam_cldata::RotaryAxis::A,
            center: Point3::new(0., 0., 0.),
            feed: None,
            length: 20.,
            tag: Tag::new(7, cam_cldata::MoveKind::Traverse),
        };
        let program = Program {
            steps: vec![step(0.), step(180.)],
        };
        let tool = Tool {
            number: 1,
            diameter: 2.,
            length: 10.,
            ..Default::default()
        };
        let fixture = FixtureEnvelope {
            instance_id: 99,
            name: "Chuck jaw".into(),
            min: [-0.1, 8., -0.02],
            max: [0.1, 9., 0.02],
        };
        let hits = super::super::verify_fixture_clearance(
            &program,
            &[tool.clone()],
            &[fixture.clone()],
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].step_index, 1);
        assert!(hits[0].fraction > 0.2 && hits[0].fraction < 0.6);
        assert_eq!(hits[0].fixture_id, 99);
        let far = FixtureEnvelope {
            min: [20., 8., 0.],
            max: [21., 9., 1.],
            ..fixture
        };
        assert!(super::super::verify_fixture_clearance(
            &program,
            &[tool],
            &[far],
            &CancelToken::new()
        )
        .unwrap()
        .is_empty());
    }
}
