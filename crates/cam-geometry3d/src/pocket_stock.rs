//! Extend an open floor to stock while retaining its supporting wall boundaries.
use super::*;

/// A straight rising wall that supports the entire selected floor also bounds
/// its stock extension. Continuing that boundary prevents clearing from wrapping
/// around the end of the wall into stock on the opposite side of the model.
/// Non-supporting walls of concave floors are left to the complete CAD shadow.
pub fn pocket_stock_domains(
    state: &GeometryState,
    selected: &[EntityRef],
    floor: f64,
    bounds: [[f64; 2]; 2],
) -> Result<Vec<Polygon>, String> {
    let mut domains = Vec::new();
    for reference in selected {
        let view = View::for_path(state, std::slice::from_ref(reference))?;
        let face = view.face(reference.id)?;
        let mut domain = vec![
            Point::new(bounds[0][0], bounds[0][1]),
            Point::new(bounds[1][0], bounds[0][1]),
            Point::new(bounds[1][0], bounds[1][1]),
            Point::new(bounds[0][0], bounds[1][1]),
        ];
        for id in &face.loop_ids {
            let wire = view.wire(*id)?;
            if !wire.is_outer {
                continue;
            }
            let (outer, _) = view.contour(wire)?;
            for usage in &wire.edge_uses {
                let edge = view.edge(usage.edge_id)?;
                let CadCurve::Line { start, end } = edge.curve else {
                    continue;
                };
                let rising = edge.face_ids.iter().filter(|id| **id != face.id).any(|id| {
                    view.face(*id).is_ok_and(|wall| {
                        view.vertical(wall) && view.bounds(wall.bounds)[1][2] > floor + T
                    })
                });
                if !rising {
                    continue;
                }
                let a = view.point(start);
                let b = view.point(end);
                let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                if length <= T {
                    continue;
                }
                let distance = |p: Point| {
                    ((b[0] - a[0]) * (p.y - a[1]) - (b[1] - a[1]) * (p.x - a[0])) / length
                };
                let min = outer
                    .points()
                    .iter()
                    .map(|p| distance(*p))
                    .fold(f64::INFINITY, f64::min);
                let max = outer
                    .points()
                    .iter()
                    .map(|p| distance(*p))
                    .fold(f64::NEG_INFINITY, f64::max);
                let side = if min >= -T && max > T {
                    1.
                } else if max <= T && min < -T {
                    -1.
                } else {
                    continue;
                };
                domain = clip_half_plane(&domain, |p| side * distance(p));
            }
        }
        if domain.len() >= 3 {
            domains.push(Polygon::new(Contour::new(domain)).map_err(|e| e.to_string())?);
        }
    }
    Ok(domains)
}

fn clip_half_plane(points: &[Point], distance: impl Fn(Point) -> f64) -> Vec<Point> {
    let mut clipped = Vec::new();
    let Some(mut previous) = points.last().copied() else {
        return clipped;
    };
    let mut previous_d = distance(previous);
    for &point in points {
        let d = distance(point);
        if (d >= 0.) != (previous_d >= 0.) {
            let t = previous_d / (previous_d - d);
            clipped.push(Point::new(
                previous.x + t * (point.x - previous.x),
                previous.y + t * (point.y - previous.y),
            ));
        }
        if d >= 0. {
            clipped.push(point);
        }
        previous = point;
        previous_d = d;
    }
    clipped
}
