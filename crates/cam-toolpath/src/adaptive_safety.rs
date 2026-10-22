//! Check emitted 3D roughing motions against the complete model shadow.
//! Segment distances protect the swept cutter, including links and entries.
//! Arc chords reserve their sagitta, so checking endpoints cannot miss a bulge.
use cam_cldata::{ArcDir, Point3, Program, Step};
use cam_geo::{Arc, Containment, Point, Polygon};

use crate::CancelToken;

const ARC_ERROR: f64 = 2.0 * cam_geo::GRID_MM;

#[derive(Debug)]
pub(super) enum SafetyFailure {
    Cancelled,
    Motion { index: usize, reason: &'static str },
}

/// `clearance` must be above all setup material. `radius` includes the largest
/// cutter/shank radius, stock allowance and CAD approximation margin. Shadows
/// already include every setup body's material above the layer's tool-tip Z.
pub(super) fn certify_layer(
    program: &Program,
    protected: &[Polygon],
    radius: f64,
    level: f64,
    clearance: f64,
    cancel: &CancelToken,
) -> Result<(), SafetyFailure> {
    let mut current: Option<Point3> = None;
    for (index, step) in program.steps().iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(SafetyFailure::Cancelled);
        }
        let reject = |reason| SafetyFailure::Motion { index, reason };
        let end = match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => *to,
            Step::Arc { end, .. } => *end,
            Step::Comment(_) => continue,
            _ => return Err(reject("unsupported motion in an adaptive layer")),
        };
        if ![end.x, end.y, end.z].into_iter().all(f64::is_finite) {
            return Err(reject("nonfinite cutter position"));
        }
        if end.z < level - cam_geo::GRID_MM {
            return Err(reject("motion descends below its certified layer"));
        }
        let Some(start) = current.replace(end) else {
            if !matches!(step, Step::Rapid { .. }) || end.z < clearance {
                return Err(reject("layer does not start at clearance"));
            }
            continue;
        };
        if start.z.min(end.z) >= clearance {
            continue;
        }
        let a = Point::new(start.x, start.y);
        let b = Point::new(end.x, end.y);
        let (points, margin) = match step {
            Step::Arc { center, dir, .. } => {
                if ![center.x, center.y, center.z].into_iter().all(f64::is_finite) {
                    return Err(reject("nonfinite arc centre"));
                }
                let r = a.distance(Point::new(center.x, center.y));
                if !r.is_finite() || r <= 0.0
                    || (r - b.distance(Point::new(center.x, center.y))).abs() > ARC_ERROR
                {
                    return Err(reject("inconsistent arc radius"));
                }
                let arc = Arc::new(
                    Point::new(center.x, center.y), r,
                    (a.y - center.y).atan2(a.x - center.x),
                    (b.y - center.y).atan2(b.x - center.x),
                    matches!(dir, ArcDir::Ccw),
                );
                let max_angle = 2.0 * (1.0 - (ARC_ERROR / r).min(1.0)).acos();
                if max_angle <= 0.0 || arc.sweep().abs() / max_angle > 100_000.0 {
                    return Err(reject("arc exceeds the safety verification resource limit"));
                }
                (arc.flatten(ARC_ERROR), ARC_ERROR)
            }
            _ => (vec![a, b], 0.0),
        };
        for pair in points.windows(2) {
            if cancel.is_cancelled() {
                return Err(SafetyFailure::Cancelled);
            }
            if !segment_clear(pair[0], pair[1], protected, radius + margin) {
                return Err(reject("swept cutter intersects the protected model"));
            }
        }
    }
    Ok(())
}

fn segment_clear(a: Point, b: Point, protected: &[Polygon], radius: f64) -> bool {
    let r2 = radius * radius;
    protected.iter().all(|polygon| {
        if polygon.locate(a) != Containment::Outside || polygon.locate(b) != Containment::Outside {
            return false;
        }
        std::iter::once(polygon.outer()).chain(polygon.holes()).all(|ring| {
            let points = ring.points();
            (0..points.len()).all(|i| {
                segment_distance2(a, b, points[i], points[(i + 1) % points.len()]) >= r2
            })
        })
    })
}

fn point_segment_distance2(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.x - a.x - t * dx).powi(2) + (p.y - a.y - t * dy).powi(2)
}

fn segment_distance2(a: Point, b: Point, c: Point, d: Point) -> f64 {
    let u = (b.x - a.x, b.y - a.y);
    let v = (d.x - c.x, d.y - c.y);
    let w = (c.x - a.x, c.y - a.y);
    let cross = |a: (f64, f64), b: (f64, f64)| a.0 * b.1 - a.1 * b.0;
    let det = cross(u, v);
    if det != 0.0 {
        let t = cross(w, v) / det;
        let s = cross(w, u) / det;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
            return 0.0;
        }
    }
    point_segment_distance2(a, c, d)
        .min(point_segment_distance2(b, c, d))
        .min(point_segment_distance2(c, a, b))
        .min(point_segment_distance2(d, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{MoveKind, Tag};
    use cam_geo::Contour;

    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
        Polygon::new(Contour::new(vec![
            Point::new(x0, y0), Point::new(x1, y0),
            Point::new(x1, y1), Point::new(x0, y1),
        ])).unwrap()
    }

    fn entry(x: f64, y: f64) -> Program {
        Program { steps: vec![
            Step::Rapid { to: Point3::new(x, y, 5.), tag: Tag::new(1, MoveKind::Link) },
            Step::Linear { to: Point3::new(x, y, 0.), feed: 100., tag: Tag::new(1, MoveKind::Plunge) },
        ] }
    }

    #[test]
    fn rejects_cutting_and_rapid_shortcuts_through_a_thin_obstacle() {
        let protected = [rectangle(-0.001, -1., 0.001, 1.)];
        for rapid in [false, true] {
            let mut program = entry(-2., 0.);
            let to = Point3::new(2., 0., 0.);
            let tag = Tag::new(1, MoveKind::Cutting);
            program.push(if rapid { Step::Rapid { to, tag } }
                else { Step::Linear { to, feed: 300., tag } });
            assert!(matches!(certify_layer(&program, &protected, 0.5, 0., 5., &CancelToken::new()),
                Err(SafetyFailure::Motion { index: 2, .. })));
        }
    }

    #[test]
    fn rejects_flank_contact_even_when_the_tool_axis_stays_outside_the_part() {
        let protected = [rectangle(0., 0., 1., 1.)];
        let program = entry(-0.2, -0.2);
        assert!(certify_layer(&program, &protected, 0.5, 0., 5., &CancelToken::new()).is_err());
    }

    #[test]
    fn checks_arc_bulges_between_safe_endpoints() {
        let protected = [rectangle(-0.1, 1.9, 0.1, 2.1)];
        for (dir, safe) in [(ArcDir::Cw, false), (ArcDir::Ccw, true)] {
            let mut program = entry(-2., 0.);
            program.push(Step::Arc {
                end: Point3::new(2., 0., 0.), center: Point3::new(0., 0., 0.),
                dir, feed: 300., tag: Tag::new(1, MoveKind::Cutting),
            });
            assert_eq!(certify_layer(&program, &protected, 0.2, 0., 5., &CancelToken::new()).is_ok(), safe);
        }
    }

    #[test]
    fn respects_holes_without_ignoring_their_walls() {
        let protected = [Polygon::with_holes(
            rectangle(-5., -5., 5., 5.).outer().clone(),
            vec![rectangle(-2., -2., 2., 2.).outer().clone()],
        ).unwrap()];
        assert!(certify_layer(&entry(0., 0.), &protected, 0.5, 0., 5., &CancelToken::new()).is_ok());
        assert!(certify_layer(&entry(1.75, 0.), &protected, 0.5, 0., 5., &CancelToken::new()).is_err());
    }

    #[test]
    fn invalid_positions_levels_and_cancellation_cannot_pass() {
        let token = CancelToken::new();
        assert!(certify_layer(&entry(f64::NAN, 0.), &[], 0.5, 0., 5., &token).is_err());
        assert!(certify_layer(&entry(0., 0.), &[], 0.5, 1., 5., &token).is_err());
        token.cancel();
        assert!(matches!(certify_layer(&entry(0., 0.), &[], 0.5, 0., 5., &token), Err(SafetyFailure::Cancelled)));
    }
}
