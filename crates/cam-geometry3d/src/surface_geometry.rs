//! Planar surface-operation geometry uses exact curves and keeps the operator's
//! cutting depth separate from the selected surface's Z plane.
use super::*;

pub(super) fn is_surface_recipe(recipe: &ExtractionRecipe) -> bool {
    matches!(
        recipe,
        ExtractionRecipe::ChamferLoop
            | ExtractionRecipe::EngravePath
            | ExtractionRecipe::CarveRegion
    )
}

pub(super) fn path(
    state: &GeometryState,
    refs: &[EntityRef],
    require_closed: bool,
) -> Result<(Vec<ProfileContour>, f64), String> {
    let view = View::new(state, refs)?;
    let (chain, closed, z, side) = if refs.len() == 1 && refs[0].kind == EntityKind::Loop {
        let wire = view.wire(refs[0].id)?;
        let (chain, z) = view.contour(wire)?;
        let (_, normal) = view.plane(view.face(wire.face_id)?)?;
        if normal[2] <= 0. {
            return Err("Select a boundary of a face facing setup Z".into());
        }
        (
            chain,
            true,
            z,
            if wire.is_outer {
                Side::Outside
            } else {
                Side::Inside
            },
        )
    } else {
        if refs.iter().any(|r| r.kind != EntityKind::Edge) {
            return Err("Select one planar boundary loop or connected edge chain".into());
        }
        let mut pieces = Vec::new();
        let mut z = None;
        for reference in refs {
            let edge = view.edge(reference.id)?;
            let points = view.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)?;
            let plane = *z.get_or_insert(points[0][2]);
            if view
                .curve_support_points(edge)?
                .iter()
                .any(|p| (p[2] - plane).abs() > T)
            {
                return Err("Selected edges must lie on one plane perpendicular to setup Z".into());
            }
            pieces.push(points);
        }
        if pieces.len() > 1
            && pieces
                .iter()
                .any(|p| distance(p[0], *p.last().unwrap()) < T)
        {
            return Err("Select a closed loop separately from other edges".into());
        }
        let endpoints: Vec<_> = pieces
            .iter()
            .filter(|p| distance(p[0], *p.last().unwrap()) >= T)
            .flat_map(|p| [p[0], *p.last().unwrap()])
            .collect();
        if endpoints
            .iter()
            .any(|p| endpoints.iter().filter(|q| distance(*p, **q) < T).count() > 2)
        {
            return Err("Selected edges branch; select one unbranched path".into());
        }
        let mut points = pieces.remove(0);
        while !pieces.is_empty() {
            let connection = pieces
                .iter()
                .enumerate()
                .find_map(|(i, p)| {
                    for (reverse, endpoint) in [(false, p[0]), (true, *p.last().unwrap())] {
                        if distance(*points.last().unwrap(), endpoint) < T {
                            return Some((i, reverse, false));
                        }
                        if distance(points[0], endpoint) < T {
                            return Some((i, reverse, true));
                        }
                    }
                    None
                })
                .ok_or("Selected edges must connect without gaps")?;
            let (index, reverse, prepend) = connection;
            let mut piece = pieces.remove(index);
            if reverse {
                piece.reverse();
            }
            if prepend {
                points.reverse();
            }
            points.extend(piece.into_iter().skip(1));
        }
        let closed = distance(points[0], *points.last().unwrap()) < T;
        if closed {
            points.pop();
        }
        let chain = Contour::new(points.into_iter().map(|p| Point::new(p[0], p[1])).collect());
        // Closed chamfers need the same certified free-side inference as profiles.
        let side = if require_closed && closed {
            let profiles = profile_contours::extract(state, refs)?.0;
            profiles.first().ok_or("Select a closed chamfer rim")?.side
        } else {
            Side::Outside
        };
        (chain, closed, z.unwrap(), side)
    };
    if require_closed && !closed {
        return Err(
            "Chamfer requires a closed rim; complete the edge chain or select a boundary loop"
                .into(),
        );
    }
    if chain.points().len() < if closed { 3 } else { 2 } {
        return Err("Degenerate machining path".into());
    }
    if closed {
        Polygon::new(chain.clone()).map_err(|e| e.to_string())?;
    }
    Ok((
        vec![ProfileContour {
            chain,
            closed,
            side,
        }],
        z,
    ))
}

pub(super) fn derive(
    state: &GeometryState,
    binding: &GeometryBinding,
) -> Result<(Vec<Polygon>, Vec<ResolvedHole>, CuttingHeights), String> {
    let refs = &binding.selected;
    let (regions, z) = if binding.recipe == ExtractionRecipe::CarveRegion {
        let view = View::new(state, refs)?;
        if refs.len() != 1 {
            return Err("Carve requires one planar face or closed boundary loop".into());
        }
        match refs[0].kind {
            EntityKind::Face => {
                let face = view.face(refs[0].id)?;
                let (z, normal) = view.plane(face)?;
                if normal[2] <= 0. {
                    return Err("Select a carving face facing setup Z".into());
                }
                let region = view.region(face)?;
                view.roofs_over(&region, z)?;
                (vec![region], z)
            }
            EntityKind::Loop => {
                let (profiles, z) = path(state, refs, false)?;
                (
                    vec![Polygon::new(profiles[0].chain.clone()).map_err(|e| e.to_string())?],
                    z,
                )
            }
            _ => return Err("Carve requires one planar face or closed boundary loop".into()),
        }
    } else {
        let (profiles, z) = path(state, refs, binding.recipe == ExtractionRecipe::ChamferLoop)?;
        let regions = profiles
            .into_iter()
            .filter(|p| p.closed)
            .map(|p| Polygon::new(p.chain).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        (regions, z)
    };
    let heights = CuttingHeights {
        top: z,
        bottom: z - binding.heights.depth(),
    };
    heights.validate()?;
    Ok((regions, vec![], heights))
}
