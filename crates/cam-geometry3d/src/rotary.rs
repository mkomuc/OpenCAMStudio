//! Exact CAD boundaries projected to a cylindrical development. Display meshes
//! are deliberately excluded, and angle unwrapping retains seam continuity.
use super::*;

pub fn wrap_cylinder(
    state: &GeometryState,
    reference: &EntityRef,
    mut wrap: RotaryWrap,
) -> Result<RotaryWrap, String> {
    let view = View::for_path(state, std::slice::from_ref(reference))?;
    if reference.kind != EntityKind::Face {
        return Err("Select a cylindrical face for Wrap Cylinder".into());
    }
    let CadSurface::Cylinder {
        origin,
        axis,
        radius,
        inward,
    } = view.face(reference.id)?.surface
    else {
        return Err(
            "Wrap Cylinder requires an analytic cylinder; cones and freeform faces are unsupported"
                .into(),
        );
    };
    if inward {
        return Err("Select an exterior cylinder for rotary wrapping".into());
    }
    let axis = view.vector(axis);
    wrap.axis = if axis[0].abs() > 1. - T && axis[1].abs() < T && axis[2].abs() < T {
        RotaryAxis::A
    } else if axis[1].abs() > 1. - T && axis[0].abs() < T && axis[2].abs() < T {
        RotaryAxis::B
    } else {
        return Err("Align the cylinder with setup X (A axis) or Y (B axis)".into());
    };
    wrap.center = view.point(origin);
    wrap.radius = radius;
    wrap.cylinder = Some(reference.clone());
    wrap.validate()?;
    Ok(wrap)
}

pub fn unwrap_boundaries(
    state: &GeometryState,
    wrap: &RotaryWrap,
) -> Result<Vec<ProfileContour>, String> {
    wrap.validate()?;
    let view = View::for_path(state, &wrap.geometry)?;
    let mut wires = Vec::<(Vec<[f64; 3]>, bool, Side, bool)>::new();
    if wrap.geometry.iter().all(|r| r.kind == EntityKind::Edge) {
        let mut remaining = wrap.geometry.clone();
        while let Some(first) = remaining.pop() {
            let mut points = sample_edge(&view, view.edge(first.id)?, false, wrap)?;
            let mut reversed_chain = false;
            loop {
                let end = *points.last().ok_or("Empty wrapped edge")?;
                let found = remaining.iter().enumerate().find_map(|(i, r)| {
                    let e = view.edge(r.id).ok()?;
                    let pts = sample_edge(&view, e, false, wrap).ok()?;
                    if distance3(end, pts[0]) < T {
                        Some((i, false))
                    } else if distance3(end, *pts.last()?) < T {
                        Some((i, true))
                    } else {
                        None
                    }
                });
                let Some((index, reverse)) = found else {
                    if !reversed_chain {
                        points.reverse();
                        reversed_chain = true;
                        continue;
                    }
                    break;
                };
                let edge = remaining.remove(index);
                points.extend(
                    sample_edge(&view, view.edge(edge.id)?, reverse, wrap)?
                        .into_iter()
                        .skip(1),
                );
                if distance3(points[0], *points.last().unwrap()) < T {
                    break;
                }
            }
            let closed = distance3(points[0], *points.last().unwrap()) < T;
            wires.push((points, closed, Side::On, false));
        }
    } else {
        let mut loops = BTreeSet::new();
        for reference in &wrap.geometry {
            match reference.kind {
                EntityKind::Face => loops.extend(view.face(reference.id)?.loop_ids.iter().copied()),
                EntityKind::Loop => {
                    loops.insert(reference.id);
                }
                _ => {
                    return Err(
                        "Select faces, loops, or connected edges for the wrapped boundary".into(),
                    )
                }
            }
        }
        for id in loops {
            let wire = view.wire(id)?;
            let mut points = Vec::new();
            for usage in &wire.edge_uses {
                let edge = sample_edge(&view, view.edge(usage.edge_id)?, usage.reversed, wrap)?;
                points.extend(edge.into_iter().skip(usize::from(!points.is_empty())));
            }
            wires.push((
                points,
                true,
                if wire.is_outer {
                    Side::Outside
                } else {
                    Side::Inside
                },
                periodic_wire(&view, wire, wrap)?,
            ));
        }
    }
    let mut contours = Vec::new();
    let mut anchor = None;
    for (points, closed, side, periodic) in wires {
        if points.len() < 2 {
            return Err("Wrapped boundary is empty".into());
        }
        let mut developed = Vec::new();
        let mut previous = anchor;
        let radius = wrap.effective_radius();
        // Subdivide straight CAD edges as well as arcs before polar projection.
        for pair in points.windows(2) {
            let radial = segment_radius(pair[0], pair[1], wrap);
            if radial <= T {
                return Err("A wrapped boundary crosses the rotary centre".into());
            }
            let spacing = radial * (wrap.tolerance / radius).sqrt() * 0.25;
            let count = (distance3(pair[0], pair[1]) / spacing).ceil().max(1.) as usize;
            if count > 100_000 || developed.len().saturating_add(count) > 1_000_000 {
                return Err(
                    "Wrapped boundary exceeds the machining budget; increase tolerance".into(),
                );
            }
            for i in 0..count {
                let t = i as f64 / count as f64;
                let p =
                    std::array::from_fn::<_, 3, _>(|k| pair[0][k] + (pair[1][k] - pair[0][k]) * t);
                developed.push(develop(p, wrap, &mut previous)?);
            }
        }
        developed.push(develop(*points.last().unwrap(), wrap, &mut previous)?);
        if anchor.is_none() {
            let point = developed[developed.len() / 2];
            anchor = Some(match wrap.axis {
                RotaryAxis::A => point.y / radius,
                RotaryAxis::B => -point.x / radius,
            });
        }
        if closed {
            let first = developed[0];
            let last = *developed.last().unwrap();
            if (first.x - last.x).hypot(first.y - last.y) > wrap.tolerance * 2. {
                return Err("A boundary winds completely around the cylinder; split it into non-winding patches before wrapping".into());
            }
            developed.pop();
            if periodic {
                let min = Point::new(
                    developed.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                    developed.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
                );
                let max = Point::new(
                    developed
                        .iter()
                        .map(|p| p.x)
                        .fold(f64::NEG_INFINITY, f64::max),
                    developed
                        .iter()
                        .map(|p| p.y)
                        .fold(f64::NEG_INFINITY, f64::max),
                );
                let width = match wrap.axis {
                    RotaryAxis::A => max.y - min.y,
                    RotaryAxis::B => max.x - min.x,
                };
                if (width - TAU * wrap.effective_radius()).abs() <= wrap.tolerance * 2. {
                    // Exact cylinder generators develop into a rectangle. Keep
                    // its artificial CAD seam out of the pocket-wall geometry.
                    developed = vec![min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)];
                }
            }
            Polygon::new(Contour::new(developed.clone()))
                .map_err(|e| format!("Invalid unwrapped boundary: {e}"))?;
        }
        contours.push(ProfileContour {
            chain: Contour::new(developed),
            closed,
            side,
        });
    }
    if contours.is_empty() {
        return Err("Select machining boundaries for the wrapped operation".into());
    }
    Ok(contours)
}

/// Cylindrical CAD floors/edges supply a radial bottom-height reference.
/// Different cylindrical radii require separate operations.
pub fn wrapped_boundary_height(
    state: &GeometryState,
    wrap: &RotaryWrap,
) -> Result<Option<f64>, String> {
    if wrap.geometry.is_empty() {
        return Ok(None);
    }
    let view = View::for_path(state, &wrap.geometry)?;
    let mut cylinder_radius = None;
    for reference in &wrap.geometry {
        if reference.kind == EntityKind::Face {
            if let CadSurface::Cylinder { radius, .. } = view.face(reference.id)?.surface {
                if cylinder_radius
                    .is_some_and(|previous: f64| (previous - radius).abs() > wrap.tolerance * 0.5)
                {
                    return Err("Selected wrapped geometry spans different radii. Split the cylindrical faces into separate wrapped operations so each keeps its model radius".into());
                }
                cylinder_radius = Some(radius);
            }
        }
    }
    let mut edges = BTreeSet::new();
    for reference in &wrap.geometry {
        match reference.kind {
            EntityKind::Edge => {
                edges.insert(reference.id);
            }
            EntityKind::Loop => {
                edges.extend(view.wire(reference.id)?.edge_uses.iter().map(|e| e.edge_id))
            }
            EntityKind::Face => {
                for id in &view.face(reference.id)?.loop_ids {
                    edges.extend(view.wire(*id)?.edge_uses.iter().map(|e| e.edge_id));
                }
            }
            _ => return Err("Select wrapped faces, loops or edges".into()),
        }
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for id in edges {
        let points = sample_edge(&view, view.edge(id)?, false, wrap)?;
        for p in &points {
            let p = radial_point(*p, wrap);
            max = max.max(p[0].hypot(p[1]));
        }
        for p in points.windows(2) {
            min = min.min(segment_radius(p[0], p[1], wrap));
        }
    }
    Ok((max.is_finite() && max - min <= wrap.tolerance * 0.5)
        .then_some(wrap.surface_z + max - wrap.effective_radius()))
}

fn periodic_wire(view: &View<'_>, wire: &CadLoop, wrap: &RotaryWrap) -> Result<bool, String> {
    let CadSurface::Cylinder {
        axis,
        origin,
        inward: false,
        ..
    } = view.face(wire.face_id)?.surface
    else {
        return Ok(false);
    };
    let axis = view.vector(axis);
    let aligned = match wrap.axis {
        RotaryAxis::A => axis[0].abs() > 1. - T,
        RotaryAxis::B => axis[1].abs() > 1. - T,
    };
    let origin = radial_point(view.point(origin), wrap);
    Ok(aligned
        && origin[0].hypot(origin[1]) < T
        && wire.edge_uses.iter().enumerate().any(|(i, e)| {
            wire.edge_uses[..i]
                .iter()
                .any(|p| p.edge_id == e.edge_id && p.reversed != e.reversed)
        }))
}

/// Every selected contour bounds a complete periodic exterior cylinder band.
pub fn periodic_cylinder_selection(
    state: &GeometryState,
    wrap: &RotaryWrap,
) -> Result<bool, String> {
    if wrap.geometry.is_empty() {
        return Ok(false);
    }
    let view = View::for_path(state, &wrap.geometry)?;
    for r in &wrap.geometry {
        let wires = match r.kind {
            EntityKind::Face => view.face(r.id)?.loop_ids.clone(),
            EntityKind::Loop => vec![r.id],
            _ => return Ok(false),
        };
        if wires.len() != 1 || !periodic_wire(&view, view.wire(wires[0])?, wrap)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn radial_point(p: [f64; 3], wrap: &RotaryWrap) -> [f64; 2] {
    [
        match wrap.axis {
            RotaryAxis::A => p[1] - wrap.center[1],
            RotaryAxis::B => wrap.center[0] - p[0],
        },
        p[2] - wrap.center[2],
    ]
}
fn segment_radius(a: [f64; 3], b: [f64; 3], wrap: &RotaryWrap) -> f64 {
    let a = radial_point(a, wrap);
    let b = radial_point(b, wrap);
    let d = [b[0] - a[0], b[1] - a[1]];
    let n = d[0] * d[0] + d[1] * d[1];
    let t = if n > 0. {
        (-(a[0] * d[0] + a[1] * d[1]) / n).clamp(0., 1.)
    } else {
        0.
    };
    (a[0] + t * d[0]).hypot(a[1] + t * d[1])
}
fn sample_edge(
    view: &View<'_>,
    edge: &CadEdge,
    reverse: bool,
    wrap: &RotaryWrap,
) -> Result<Vec<[f64; 3]>, String> {
    let mut tolerance = wrap.tolerance * 0.1;
    for _ in 0..8 {
        let points = view.curve_points(edge, reverse, tolerance)?;
        let radius = points
            .windows(2)
            .map(|p| segment_radius(p[0], p[1], wrap))
            .fold(f64::INFINITY, f64::min)
            - tolerance;
        if radius > T {
            // Polar projection amplifies a Cartesian curve error by R/r.
            let needed = wrap.tolerance * 0.1 * (radius / wrap.effective_radius()).min(1.);
            if tolerance <= needed * 1.001 {
                return Ok(points);
            }
            tolerance = needed;
        } else {
            tolerance *= 0.1;
        }
    }
    Err("Wrapped boundary approaches the rotary centre too closely to certify the requested tolerance".into())
}
fn develop(p: [f64; 3], wrap: &RotaryWrap, previous: &mut Option<f64>) -> Result<Point, String> {
    let z = p[2] - wrap.center[2];
    let cross = match wrap.axis {
        RotaryAxis::A => p[1] - wrap.center[1],
        RotaryAxis::B => wrap.center[0] - p[0],
    };
    if z.hypot(cross) <= T {
        return Err("A wrapped boundary crosses the rotary centre".into());
    }
    let seam = wrap.seam_angle.to_radians();
    let mut angle = cross.atan2(z) - seam;
    let anchor = previous.unwrap_or(0.);
    angle = anchor + (angle - anchor + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
    *previous = Some(angle);
    let circumference = angle * wrap.effective_radius();
    Ok(match wrap.axis {
        RotaryAxis::A => Point::new(p[0], circumference),
        RotaryAxis::B => Point::new(-circumference, p[1]),
    })
}

/// Preserve nested islands and disjoint pockets in the developed plane.
pub fn wrapped_regions(contours: &[ProfileContour]) -> Result<Vec<Polygon>, String> {
    if contours.iter().any(|c| !c.closed) {
        return Err("Wrapped pockets require closed boundaries".into());
    }
    let polygons = contours
        .iter()
        .map(|c| Polygon::new(c.chain.clone()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let parents: Vec<_> = polygons
        .iter()
        .enumerate()
        .map(|(i, p)| {
            polygons
                .iter()
                .enumerate()
                .filter(|(j, q)| {
                    *j != i && q.area() > p.area() && q.contains(p.outer().points()[0])
                })
                .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
                .map(|(j, _)| j)
        })
        .collect();
    let depth = |i: usize| {
        let mut n = 0;
        let mut parent = parents[i];
        while let Some(j) = parent {
            n += 1;
            parent = parents[j];
        }
        n
    };
    polygons
        .iter()
        .enumerate()
        .filter(|(i, _)| depth(*i) % 2 == 0)
        .map(|(i, p)| {
            let holes = polygons
                .iter()
                .enumerate()
                .filter(|(j, _)| parents[*j] == Some(i))
                .map(|(_, p)| p.outer().clone())
                .collect();
            Polygon::with_holes(p.outer().clone(), holes).map_err(|e| e.to_string())
        })
        .collect()
}
