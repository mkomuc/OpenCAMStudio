//! Exact planar chamfer strips, including their upper/lower trimming boundaries.
use super::*;

pub(super) fn extract(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<(Vec<ChamferFace>, CuttingHeights), String> {
    let view = View::new(state, refs)?;
    let mut result: Vec<ChamferFace> = Vec::new();
    for reference in refs {
        if reference.kind != EntityKind::Face {
            return Err(
                "Select modeled chamfer faces separately from boundary loops or edges".into(),
            );
        }
        let face = view.face(reference.id)?;
        let CadSurface::Plane { normal, .. } = face.surface else {
            return Err(format!(
                "Chamfer Face {}: select an inclined planar chamfer face",
                face.id
            ));
        };
        let normal = view.vector(normal);
        let horizontal = normal[0].hypot(normal[1]);
        if normal[2] <= T || horizontal <= T {
            return Err(format!("Chamfer Face {}: select an upward inclined chamfer face; horizontal tops and vertical walls are not bevels", face.id));
        }
        if face.loop_ids.len() != 1 {
            return Err("Chamfer face requires one boundary without holes".into());
        }
        let wire = view.wire(face.loop_ids[0])?;
        let mut points = Vec::new();
        for usage in &wire.edge_uses {
            let edge = view.edge(usage.edge_id)?;
            if !matches!(edge.curve, CadCurve::Line { .. }) {
                return Err("Modeled chamfer faces currently require straight upper/lower boundaries and straight side edges".into());
            }
            let segment = view.curve_points(edge, usage.reversed, MACHINING_CHORD_TOLERANCE)?;
            if points
                .last()
                .is_some_and(|last| distance(*last, segment[0]) > T)
            {
                return Err("Chamfer face boundary has disconnected edges".into());
            }
            if !points.is_empty() {
                points.pop();
            }
            points.extend(segment);
        }
        if points.len() < 5 || distance(points[0], *points.last().unwrap()) > T {
            return Err("Chamfer face boundary must be closed".into());
        }
        points.pop();
        let top = points
            .iter()
            .map(|p| p[2])
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        let outward = [normal[0] / horizontal, normal[1] / horizontal];
        let tangent = [-outward[1], outward[0]];
        let boundary = |z: f64| -> Result<[[f64; 2]; 2], String> {
            let mut ends: Vec<_> = points
                .iter()
                .filter(|p| (p[2] - z).abs() < T)
                .map(|p| [p[0], p[1]])
                .collect();
            if ends.len() < 2 {
                return Err("Chamfer face needs horizontal upper and lower boundaries".into());
            }
            ends.sort_by(|a, b| {
                (a[0] * tangent[0] + a[1] * tangent[1])
                    .total_cmp(&(b[0] * tangent[0] + b[1] * tangent[1]))
            });
            Ok([ends[0], *ends.last().unwrap()])
        };
        let strip = ChamferFace {
            upper: boundary(top)?,
            lower: boundary(bottom)?,
            top,
            bottom,
            outward,
        };
        strip.validate()?;
        // Every trim vertex must lie on the trapezoid's four straight sides.
        // This admits split collinear edges, without replacing a trimmed face by a hull.
        let corners = [
            [strip.upper[0][0], strip.upper[0][1], top],
            [strip.upper[1][0], strip.upper[1][1], top],
            [strip.lower[1][0], strip.lower[1][1], bottom],
            [strip.lower[0][0], strip.lower[0][1], bottom],
        ];
        let side_of = |point: [f64; 3]| -> Option<usize> {
            (0..4).find(|i| {
                let a = corners[*i];
                let b = corners[(*i + 1) % 4];
                let v: [f64; 3] = std::array::from_fn(|j| b[j] - a[j]);
                let t = (0..3).map(|j| (point[j] - a[j]) * v[j]).sum::<f64>()
                    / v.iter().map(|x| x * x).sum::<f64>();
                (-T..=1. + T).contains(&t)
                    && distance(point, std::array::from_fn(|j| a[j] + t * v[j])) < T
            })
        };
        for index in 0..points.len() {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            if side_of(a).is_none()
                || side_of(std::array::from_fn(|i| (a[i] + b[i]) / 2.)).is_none()
            {
                return Err(
                    "Chamfer face must be a complete strip between its upper and lower boundaries"
                        .into(),
                );
            }
        }
        if (strip.width() / (top - bottom) - normal[2] / horizontal).abs() > T {
            return Err("Chamfer boundaries do not match the exact face inclination".into());
        }
        if result
            .first()
            .is_some_and(|old| (old.top - top).abs() > T || (old.bottom - bottom).abs() > T)
        {
            return Err(
                "Chamfer faces with different upper/lower heights need separate operations".into(),
            );
        }
        result.push(strip);
    }
    let first = result.first().ok_or("Select modeled chamfer faces")?;
    let heights = CuttingHeights {
        top: first.top,
        bottom: first.bottom,
    };
    heights.validate()?;
    Ok((result, heights))
}
