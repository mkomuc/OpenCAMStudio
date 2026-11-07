//! Convert a completed unwrapped operation into continuous four-axis motions.
use crate::CancelToken;
use cam_cldata::{arc_sweep, ArcDir, Point3, Program, Step, Tag};
use cam_model::RotaryWrap;

pub fn wrap_program(
    source: &Program,
    wrap: &RotaryWrap,
    cancel: &CancelToken,
) -> Result<Program, String> {
    wrap.validate()?;
    source.validate().map_err(|e| e.to_string())?;
    let mut out = Program::new();
    let mut current = None;
    for step in source.steps() {
        if cancel.is_cancelled() {
            return Err("Rotary wrapping cancelled".into());
        }
        match step {
            Step::Rapid { to, tag } => {
                line(&mut out, current, *to, None, *tag, wrap, cancel)?;
                current = Some(*to);
            }
            Step::Linear { to, feed, tag } => {
                line(&mut out, current, *to, Some(*feed), *tag, wrap, cancel)?;
                current = Some(*to);
            }
            Step::Arc {
                end,
                center,
                dir,
                feed,
                tag,
            } => {
                let start = current.ok_or("Wrapped arc has no start")?;
                let radius = (start.x - center.x).hypot(start.y - center.y);
                let sweep = arc_sweep(start, *end, *center, *dir);
                let max_angle = chord_angle(radius, wrap.tolerance * 0.5);
                let count = (sweep / max_angle).ceil().max(1.) as usize;
                if count > 1_000_000 {
                    return Err("Wrap arc exceeds 1,000,000 segments; increase tolerance".into());
                }
                let sign = if *dir == ArcDir::Cw { -1. } else { 1. };
                let angle = (start.y - center.y).atan2(start.x - center.x);
                let mut from = start;
                for i in 1..=count {
                    let t = i as f64 / count as f64;
                    let a = angle + sign * sweep * t;
                    let to = if i == count {
                        *end
                    } else {
                        Point3::new(
                            center.x + radius * a.cos(),
                            center.y + radius * a.sin(),
                            start.z + (end.z - start.z) * t,
                        )
                    };
                    line(&mut out, Some(from), to, Some(*feed), *tag, wrap, cancel)?;
                    from = to;
                }
                current = Some(*end);
            }
            Step::CutterComp(cam_cldata::CutterComp::Off) => out.push(step.clone()),
            Step::CutterComp(_) => {
                return Err("Wrapped contours require computed cutter compensation".into())
            }
            Step::Drill(_) | Step::Rotary { .. } | Step::Datum(_) => {
                return Err("Wrap supports planar contour and pocket motions only".into())
            }
            _ => out.push(step.clone()),
        }
    }
    out.validate().map_err(|e| e.to_string())?;
    Ok(out)
}
fn chord_angle(radius: f64, tolerance: f64) -> f64 {
    (2. * (1. - (tolerance / radius.max(tolerance)).min(1.)).acos()).clamp(1e-6, 5_f64.to_radians())
}
fn line(
    out: &mut Program,
    from: Option<Point3>,
    to: Point3,
    feed: Option<f64>,
    tag: Tag,
    wrap: &RotaryWrap,
    cancel: &CancelToken,
) -> Result<(), String> {
    let (end, end_angle) = wrap.position(to);
    let radial = end.z - wrap.center[2];
    if radial <= 0. {
        return Err(
            "Wrapped toolpath reaches or crosses the rotary centre; reduce the cut depth".into(),
        );
    }
    if let Some(from) = from {
        let (start, start_angle) = wrap.position(from);
        let crosses = (end.x - start.x).hypot(end.y - start.y) > 1e-9
            || (end_angle - start_angle).abs() > 1e-9;
        if feed.is_none()
            && crosses
            && radial.min(start.z - wrap.center[2]) < wrap.effective_radius() - 1e-9
        {
            return Err(
                "A wrapped rapid traverses below the cylinder surface; raise the retract height"
                    .into(),
            );
        }
    }
    let count = if let Some(from) = from {
        let (start, a) = wrap.position(from);
        let radius = radial.max(start.z - wrap.center[2]);
        ((end_angle - a).to_radians().abs() / chord_angle(radius, wrap.tolerance * 0.5))
            .ceil()
            .max(1.) as usize
    } else {
        1
    };
    if count > 1_000_000 || out.len().saturating_add(count) > 5_000_000 {
        return Err("Wrapped toolpath exceeds the motion budget; increase tolerance".into());
    }
    let center = Point3::new(wrap.center[0], wrap.center[1], wrap.center[2]);
    let mut previous = from.map(|p| wrap.position(p));
    for i in 1..=count {
        if cancel.is_cancelled() {
            return Err("Rotary wrapping cancelled".into());
        }
        let t = i as f64 / count as f64;
        let p = from.map_or(to, |a| {
            Point3::new(
                a.x + (to.x - a.x) * t,
                a.y + (to.y - a.y) * t,
                a.z + (to.z - a.z) * t,
            )
        });
        let (position, angle) = wrap.position(p);
        let length = previous.map_or(0., |(a, aa)| {
            let rotation = (angle - aa).to_radians() * ((position.z + a.z) * 0.5 - center.z);
            ((position.x - a.x).powi(2)
                + (position.y - a.y).powi(2)
                + (position.z - a.z).powi(2)
                + rotation.powi(2))
            .sqrt()
        });
        if feed.is_some() && length <= 1e-12 {
            if previous.is_none() {
                return Err("Wrapped feed has no preceding rapid position".into());
            }
            continue;
        }
        out.push(Step::Rotary {
            to: position,
            angle,
            axis: wrap.axis,
            center,
            feed,
            length,
            tag,
        });
        previous = Some((position, angle));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{MoveKind, ProgramBuilder, RotaryAxis};
    #[test]
    fn quarter_turn_preserves_surface_speed_and_material_location() {
        let wrap = RotaryWrap {
            radius: 20.,
            ..Default::default()
        };
        let source = ProgramBuilder::new()
            .feed(180.)
            .rapid(Point3::new(5., 0., -2.), MoveKind::Link)
            .linear(
                Point3::new(5., 10. * std::f64::consts::PI, -2.),
                MoveKind::Cutting,
            )
            .build();
        let result = wrap_program(&source, &wrap, &CancelToken::new()).unwrap();
        let Step::Rotary { to, angle, .. } = result.steps().last().unwrap() else {
            panic!()
        };
        assert!((*angle - 90.).abs() < 1e-9);
        assert_eq!(*to, Point3::new(5., 0., 18.));
        let point = result.rotary_backplot().steps().last().unwrap().clone();
        let Step::Linear { to, .. } = point else {
            panic!()
        };
        assert!((to.y - 18.).abs() < 1e-9 && to.z.abs() < 1e-9);
        let estimate = result.processing_time(1000., 0., 100.).unwrap();
        assert!((estimate.feed_distance_mm - 9. * std::f64::consts::PI).abs() < 1e-8);
        assert!((estimate.feed_seconds - 3. * std::f64::consts::PI).abs() < 1e-8);
    }
    #[test]
    fn b_axis_keeps_multiple_turns_and_translates_without_changing_angles() {
        let wrap = RotaryWrap {
            axis: RotaryAxis::B,
            radius: 10.,
            center: [3., 4., 5.],
            ..Default::default()
        };
        let source = ProgramBuilder::new()
            .feed(200.)
            .rapid(Point3::new(0., 7., 0.), MoveKind::Link)
            .linear(
                Point3::new(50. * std::f64::consts::PI, 7., 0.),
                MoveKind::Cutting,
            )
            .build();
        let result = wrap_program(&source, &wrap, &CancelToken::new()).unwrap();
        let shifted = result.translated([-3., -4., -5.]);
        assert!(
            matches!(shifted.steps().last(),Some(Step::Rotary{angle,to,center,..}) if (*angle+900.).abs()<1e-8 && *to==Point3::new(0.,3.,10.) && *center==Point3::new(0.,0.,0.))
        );
    }
    #[test]
    fn arcs_are_linearized_and_wrap_is_cancellable_and_rejects_centre_crossings() {
        let source = ProgramBuilder::new()
            .feed(200.)
            .rapid(Point3::new(10., 0., 0.), MoveKind::Link)
            .arc(
                Point3::new(-10., 0., -1.),
                Point3::new(0., 0., 0.),
                ArcDir::Ccw,
                MoveKind::Cutting,
            )
            .build();
        let result = wrap_program(&source, &RotaryWrap::default(), &CancelToken::new()).unwrap();
        assert!(!result.steps().iter().any(|s| matches!(s, Step::Arc { .. })));
        assert!(result.len() > 20);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(wrap_program(&source, &RotaryWrap::default(), &cancel).is_err());
        let source = ProgramBuilder::new()
            .feed(200.)
            .rapid(Point3::new(0., 0., 1.), MoveKind::Link)
            .linear(Point3::new(0., 0., -30.), MoveKind::Plunge)
            .build();
        assert!(
            wrap_program(&source, &RotaryWrap::default(), &CancelToken::new())
                .unwrap_err()
                .contains("centre")
        );
    }
}
