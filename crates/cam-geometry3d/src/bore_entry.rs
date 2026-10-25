//! Qualify a full, outward-opening quarter-torus above an internal cylinder.
//! Only the cylindrical wall is machined. The rounded entry proves access;
//! neither its display mesh nor an arbitrary curved termination is accepted.
use super::*;

pub(super) fn certify(
    view: &View<'_>,
    entry: &CadFace,
    cylinder: &CadFace,
    cylinder_rim: &CadEdge,
) -> Result<(), String> {
    let invalid = || {
        "A cylindrical feature requires a complete coaxial rounded entrance that widens upward to an open planar rim"
            .to_string()
    };
    let CadSurface::Cylinder {
        origin: c,
        radius,
        inward: true,
        ..
    } = cylinder.surface
    else {
        return Err(invalid());
    };
    let CadSurface::Torus {
        origin,
        axis,
        major_radius,
        minor_radius,
    } = entry.surface
    else {
        return Err(invalid());
    };
    let c = view.point(c);
    let origin = view.point(origin);
    let axis = view.vector(axis);
    let (_, top) = view.extent(cylinder)?;
    let (entry_bottom, entry_top) = view.extent(entry)?;
    if minor_radius <= T
        || major_radius <= minor_radius
        || axis[0].abs() > T
        || axis[1].abs() > T
        || (axis[2].abs() - 1.).abs() > T
        || (origin[0] - c[0]).abs() > T
        || (origin[1] - c[1]).abs() > T
        || (origin[2] - top).abs() > T
        || (major_radius - minor_radius - radius).abs() > T
        || (entry_bottom - top).abs() > T
        || (entry_top - top - minor_radius).abs() > T
    {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    let mut lower_sweep = 0.;
    let mut upper_sweep = 0.;
    for wire_id in &entry.loop_ids {
        for usage in &view.wire(*wire_id)?.edge_uses {
            let edge = view.edge(usage.edge_id)?;
            if !seen.insert(edge.id) {
                continue;
            }
            // A periodic seam belongs to this face on both sides, not to an
            // opening/obstacle. Its analytic extrema were checked by extent().
            if edge.face_ids.len() == 2 && edge.face_ids.iter().all(|id| *id == entry.id) {
                continue;
            }
            let CadCurve::CircleArc {
                center,
                normal,
                radius: rim_radius,
                start_angle,
                end_angle,
                ..
            } = edge.curve
            else {
                return Err(invalid());
            };
            let center = view.point(center);
            let normal = view.vector(normal);
            if (center[0] - c[0]).abs() > T
                || (center[1] - c[1]).abs() > T
                || normal[0].abs() > T
                || normal[1].abs() > T
                || edge.face_ids.len() != 2
                || !edge.face_ids.contains(&entry.id)
            {
                return Err(invalid());
            }
            let sweep = (end_angle - start_angle).abs();
            if (center[2] - top).abs() < T && (rim_radius - radius).abs() < T {
                if edge.id != cylinder_rim.id || !edge.face_ids.contains(&cylinder.id) {
                    return Err(invalid());
                }
                lower_sweep += sweep;
            } else if (center[2] - entry_top).abs() < T && (rim_radius - major_radius).abs() < T {
                let adjacent_id = edge
                    .face_ids
                    .iter()
                    .find(|id| **id != entry.id)
                    .ok_or_else(invalid)?;
                let adjacent = view.face(*adjacent_id)?;
                let (z, n) = view.plane(adjacent).map_err(|_| invalid())?;
                if (z - entry_top).abs() > T
                    || n[2] < 1. - T
                    || !adjacent.loop_ids.iter().any(|id| {
                        view.wire(*id).is_ok_and(|wire| {
                            !wire.is_outer && wire.edge_uses.iter().any(|u| u.edge_id == edge.id)
                        })
                    })
                {
                    return Err(invalid());
                }
                upper_sweep += sweep;
            } else {
                return Err(invalid());
            }
        }
    }
    if (lower_sweep - TAU).abs() > T || (upper_sweep - TAU).abs() > T {
        return Err(invalid());
    }
    Ok(())
}
