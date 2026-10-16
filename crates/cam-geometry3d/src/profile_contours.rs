//! Exact face boundaries and connected edge chains in setup coordinates.
use super::*;

pub(super) fn extract(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<(Vec<ProfileContour>, CuttingHeights), String> {
    let view = View::new(state, refs)?;
    let mut loops = BTreeSet::new();
    let mut edges = Vec::new();
    for reference in refs {
        match reference.kind {
            EntityKind::Face => {
                let face = view.face(reference.id)?;
                let (_, normal) = view.plane(face)?;
                if normal[2] <= 0. {
                    return Err("Select a face facing setup Z".into());
                }
                loops.extend(face.loop_ids.iter().copied());
            }
            EntityKind::Loop => {
                loops.insert(reference.id);
            }
            EntityKind::Edge => edges.push(reference.id),
            _ => {
                return Err("Select planar faces, boundary loops, or a connected edge chain".into())
            }
        }
    }
    if !edges.is_empty() && !loops.is_empty() {
        return Err("Select edge chains separately from face contours".into());
    }
    let mut profiles = Vec::new();
    let mut heights: Option<CuttingHeights> = None;
    let mut accept = |profile, interval: CuttingHeights| -> Result<(), String> {
        interval.validate()?;
        heights = Some(if let Some(previous) = heights {
            if (previous.top - interval.top).abs() > T {
                return Err("Selected contours lie on different planes; use separate profile operations".into());
            }
            // All contours may be cut only as deep as the shallowest certified wall.
            CuttingHeights { top: previous.top, bottom: previous.bottom.max(interval.bottom) }
        } else { interval });
        profiles.push(profile);
        Ok(())
    };
    for id in loops {
        let wire = view.wire(id)?;
        let mut reference = refs[0].clone();
        reference.kind = EntityKind::Loop;
        reference.id = id;
        let side = profile_side(state, &[reference.clone()])?;
        // Retain the stricter whole-body / blind-floor certification where applicable.
        let binding = GeometryBinding {
            selected: vec![reference],
            recipe: ExtractionRecipe::ProfileLoop { side },
            heights: CuttingHeights {
                top: 0.,
                bottom: -1.,
            },
            overrides: Default::default(),
            resolved_fingerprint: None,
            status: ResolutionStatus::Resolved,
        };
        if let Ok((regions, _, interval)) = derive(state, &binding, 0.) {
            accept(
                ProfileContour {
                    chain: regions[0].outer().clone(),
                    closed: true,
                    side,
                },
                interval,
            )?;
            continue;
        }
        // Inner boundaries on an upward top face delimit an accessible void.
        // Its adjoining constant-section walls certify the complete depth.
        let face = view.face(wire.face_id)?;
        let (z, normal) = view.plane(face)?;
        if wire.is_outer || normal[2] <= 0. || z < view.bounds(view.body.bounds)[1][2] - T {
            return Err("This face boundary cannot certify an accessible vertical profile".into());
        }
        let (chain, _) = view.contour(wire)?;
        let mut bottom: Option<f64> = None;
        for wall in view.walls(wire)? {
            let (lo, hi) = view.extent(wall)?;
            if (hi - z).abs() > T {
                return Err("Face contour must be the top of its wall".into());
            }
            view.certify_wall(wall, lo, hi)?;
            if bottom.is_some_and(|old| (old - lo).abs() > T) {
                return Err("Contour walls have different depths".into());
            }
            bottom = Some(lo);
        }
        accept(
            ProfileContour {
                chain,
                closed: true,
                side: Side::Inside,
            },
            CuttingHeights {
                top: z,
                bottom: bottom.ok_or("No contour walls")?,
            },
        )?;
    }
    if !edges.is_empty() {
        let mut pieces = Vec::new();
        let mut interval: Option<CuttingHeights> = None;
        for id in edges {
            let edge = view.edge(id)?;
            let points = view.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)?;
            let z = points.first().ok_or("Empty edge")?[2];
            if points.iter().any(|p| (p[2] - z).abs() > T)
                || z < view.bounds(view.body.bounds)[1][2] - T
            {
                return Err(
                    "Chain edges must lie on one accessible plane normal to setup Z".into(),
                );
            }
            let wall = edge
                .face_ids
                .iter()
                .filter_map(|id| view.face(*id).ok())
                .find(|face| view.vertical(face))
                .ok_or("Chain edge needs an adjoining vertical wall")?;
            let (lo, hi) = view.extent(wall)?;
            view.certify_wall(wall, lo, hi)?;
            if (hi - z).abs() > T {
                return Err("Select the top edge of the profile wall".into());
            }
            let current = CuttingHeights { top: z, bottom: lo };
            if interval.is_some_and(|old| (old.top - z).abs() > T || (old.bottom - lo).abs() > T) {
                return Err("Chain edges have different cutting heights".into());
            }
            interval = Some(current);
            pieces.push(points);
        }
        let endpoints: Vec<_> = pieces
            .iter()
            .filter(|piece| distance(piece[0], *piece.last().unwrap()) >= T)
            .flat_map(|piece| [piece[0], *piece.last().unwrap()])
            .collect();
        if endpoints
            .iter()
            .any(|p| endpoints.iter().filter(|q| distance(*p, **q) < T).count() > 2)
        {
            return Err("Chain edges branch; select one unbranched path".into());
        }
        let mut points = pieces.remove(0);
        // Join at either endpoint; never invent a closing edge across a gap.
        while !pieces.is_empty() {
            let mut connection = None;
            for (i, piece) in pieces.iter().enumerate() {
                for reverse in [false, true] {
                    let start = if reverse {
                        piece.last().unwrap()
                    } else {
                        &piece[0]
                    };
                    if distance(*points.last().unwrap(), *start) < T {
                        connection = Some((i, reverse, false));
                        break;
                    }
                    if distance(points[0], *start) < T {
                        connection = Some((i, reverse, true));
                        break;
                    }
                }
                if connection.is_some() {
                    break;
                }
            }
            let (i, reverse, prepend) =
                connection.ok_or("Chain edges must connect without gaps or branches")?;
            let mut piece = pieces.remove(i);
            if reverse {
                piece.reverse();
            }
            if prepend {
                points.reverse();
            }
            points.extend(piece.into_iter().skip(1));
        }
        let closed = distance(points[0], *points.last().unwrap()) < T;
        // Infer the free side from the adjoining upward face's boundary winding.
        // Joining can reverse the source edge, so compare the actual first segment.
        let mut side = None;
        for reference in refs {
            let edge = view.edge(reference.id)?;
            let samples = view.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)?;
            let forward = samples.windows(2).find_map(|pair| {
                if distance(points[0], pair[0]) < T && distance(points[1], pair[1]) < T {
                    Some(true)
                } else if distance(points[0], pair[1]) < T && distance(points[1], pair[0]) < T {
                    Some(false)
                } else {
                    None
                }
            });
            let Some(forward) = forward else {
                continue;
            };
            for id in &edge.face_ids {
                let face = view.face(*id)?;
                if !view.plane(face).is_ok_and(|(_, normal)| normal[2] > 0.) {
                    continue;
                }
                for id in &face.loop_ids {
                    let wire = view.wire(*id)?;
                    let Some(usage) = wire.edge_uses.iter().find(|usage| usage.edge_id == edge.id)
                    else {
                        continue;
                    };
                    let (contour, _) = view.contour(wire)?;
                    let mut free_left = wire.is_outer != contour.is_ccw();
                    if forward == usage.reversed {
                        free_left = !free_left;
                    }
                    side = Some(if closed {
                        if wire.is_outer {
                            Side::Outside
                        } else {
                            Side::Inside
                        }
                    } else if free_left {
                        Side::Outside
                    } else {
                        Side::Inside
                    });
                    break;
                }
            }
        }
        if closed {
            points.pop();
        }
        let chain = Contour::new(points.into_iter().map(|p| Point::new(p[0], p[1])).collect());
        accept(
            ProfileContour {
                chain,
                closed,
                side: side.ok_or("Chain must follow an upward face boundary")?,
            },
            interval.unwrap(),
        )?;
    }
    Ok((profiles, heights.ok_or("Select profile contours")?))
}

pub fn coplanar_faces(
    state: &GeometryState,
    reference: &EntityRef,
) -> Result<Vec<EntityRef>, String> {
    let view = View::new(state, std::slice::from_ref(reference))?;
    let (height, normal) = view.plane(view.face(reference.id)?)?;
    if normal[2] <= 0. {
        return Err("Select a face facing setup Z".into());
    }
    Ok(view
        .asset
        .topology
        .faces
        .iter()
        .filter(|face| face.body_id == reference.body_id)
        .filter(|face| {
            view.plane(face)
                .is_ok_and(|(z, n)| (z - height).abs() < T && n[2] > 0.)
        })
        .map(|face| EntityRef {
            id: face.id,
            kind: EntityKind::Face,
            ..reference.clone()
        })
        .collect())
}
