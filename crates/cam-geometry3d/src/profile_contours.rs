//! Exact face boundaries and connected edge chains in setup coordinates.
use super::*;

pub(super) fn extract(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<(Vec<ProfileContour>, CuttingHeights), String> {
    extract_impl(state, refs, false)
}

/// Explicit open chains provide a planar path; operation heights define its
/// vertical extension, independently of the adjoining surface's shape.
pub(super) fn open_chain(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<Vec<ProfileContour>, String> {
    if refs.iter().any(|r| r.kind != EntityKind::Edge) {
        return Err("Open Chain requires connected planar edges".into());
    }
    match extract_impl(state, refs, true) {
        Ok(input) => Ok(input.0),
        Err(reason) => surface_geometry::unverified_path(state, refs, false)
            .map(|input| input.0)
            .map_err(|_| reason),
    }
}

/// A closed Chain selects an exact planar path, independently of the shape of
/// its supporting face or wall trims. Cutting heights belong to the operation.
pub(super) fn closed_chain(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<Vec<ProfileContour>, String> {
    let view = View::for_path(state, refs)?;
    if refs.len() > 1
        && refs
            .iter()
            .all(|reference| reference.kind == EntityKind::Loop)
    {
        return unverified_contours(state, refs);
    }
    if refs.len() == 1 && refs[0].kind == EntityKind::Loop {
        let wire = view.wire(refs[0].id)?;
        let (chain, _) = view.contour(wire)?;
        Polygon::new(chain.clone()).map_err(|error| error.to_string())?;
        return Ok(vec![ProfileContour {
            chain,
            closed: true,
            side: if wire.is_outer {
                Side::Outside
            } else {
                Side::Inside
            },
        }]);
    }
    let (contours, _) = surface_geometry::unverified_path(state, refs, false)?;
    if contours.iter().any(|contour| !contour.closed) {
        return Err("Complete the chain before selecting Closed mode".into());
    }
    Ok(contours)
}

pub(super) fn unverified_contours(
    state: &GeometryState,
    refs: &[EntityRef],
) -> Result<Vec<ProfileContour>, String> {
    if refs
        .iter()
        .all(|reference| reference.kind == EntityKind::Edge)
    {
        return surface_geometry::unverified_path(state, refs, false).map(|input| input.0);
    }
    let view = View::for_path(state, refs)?;
    let mut loops = BTreeSet::new();
    for reference in refs {
        match reference.kind {
            EntityKind::Loop => {
                loops.insert(reference.id);
            }
            EntityKind::Face => {
                loops.extend(view.face(reference.id)?.loop_ids.iter().copied());
            }
            _ => return Err("Select boundary loops, faces, or connected edges".into()),
        }
    }
    loops
        .into_iter()
        .map(|id| {
            let wire = view.wire(id)?;
            let (chain, _) = view.contour(wire)?;
            Polygon::new(chain.clone()).map_err(|error| error.to_string())?;
            Ok(ProfileContour {
                chain,
                closed: true,
                side: if wire.is_outer {
                    Side::Outside
                } else {
                    Side::Inside
                },
            })
        })
        .collect()
}

fn extract_impl(
    state: &GeometryState,
    refs: &[EntityRef],
    open_curves: bool,
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
                return Err(
                    "Selected contours lie on different planes; use separate profile operations"
                        .into(),
                );
            }
            // All contours may be cut only as deep as the shallowest certified wall.
            CuttingHeights {
                top: previous.top,
                bottom: previous.bottom.max(interval.bottom),
            }
        } else {
            interval
        });
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
        let derived = derive(state, &binding, 0.);
        if let Ok((regions, _, interval)) = &derived {
            accept(
                ProfileContour {
                    chain: regions[0].outer().clone(),
                    closed: true,
                    side,
                },
                *interval,
            )?;
            continue;
        }
        let derive_error = derived.unwrap_err();
        // Upward faces can bound either a feature's rim or its floor.
        // Their adjoining constant-section walls certify the complete depth.
        let face = view.face(wire.face_id)?;
        let (z, normal) = view.plane(face)?;
        if normal[2] <= 0. {
            return Err(derive_error);
        }
        let (chain, _) = view.contour(wire)?;
        let walls = view.walls(wire)?;
        if !walls.is_empty()
            && walls.iter().all(|wall| {
                view.extent(wall)
                    .is_ok_and(|(lo, hi)| (lo - z).abs() <= T && hi > z + T)
            })
        {
            // A floor contour follows the same wall as its upper rim. Reuse
            // pocket certification for access, full-height walls and rim adjacency.
            let floor_binding = GeometryBinding {
                selected: vec![EntityRef {
                    kind: EntityKind::Face,
                    id: face.id,
                    ..refs[0].clone()
                }],
                recipe: ExtractionRecipe::PocketFloor,
                ..binding
            };
            let (_, _, interval) = derive(state, &floor_binding, 0.)?;
            accept(
                ProfileContour {
                    chain,
                    closed: true,
                    side: if wire.is_outer {
                        Side::Inside
                    } else {
                        Side::Outside
                    },
                },
                interval,
            )?;
            continue;
        }
        let mut bottom: Option<f64> = None;
        for wall in walls {
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
        let region = Polygon::new(chain.clone()).map_err(|error| error.to_string())?;
        view.roofs_over(&region, z)?;
        // Face Contours can select the perimeter of a raised island as well
        // as the inner boundary of a pocket. The wall certification above
        // and roof check below establish that the selected face is the top of
        // the feature, so the loop's face-local outer/inner flag does not
        // determine the machining side.
        accept(
            ProfileContour {
                chain,
                closed: true,
                side: if wire.is_outer {
                    Side::Outside
                } else {
                    Side::Inside
                },
            },
            CuttingHeights {
                top: z,
                bottom: bottom.ok_or("No contour walls")?,
            },
        )?;
    }
    if !edges.is_empty() {
        let mut pieces = Vec::new();
        let mut walls = std::collections::BTreeMap::new();
        let mut interval: Option<CuttingHeights> = None;
        for id in edges {
            let edge = view.edge(id)?;
            let points = view.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)?;
            let z = points.first().ok_or("Empty edge")?[2];
            let support = view.curve_support_points(edge)?;
            if support.iter().any(|p| (p[2] - z).abs() > T) {
                let lo = support.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                let hi = support
                    .iter()
                    .map(|p| p[2])
                    .fold(f64::NEG_INFINITY, f64::max);
                return Err(format!("Profile Edge {}: Chain edges must lie on one accessible plane normal to setup Z; this edge spans setup Z {lo:.6} to {hi:.6} mm. Select a planar rim or floor edge, or align setup Z to its plane", edge.id));
            }
            if open_curves {
                // This interval only seeds extraction's internal accumulator;
                // it is not returned as a certified wall depth to the caller.
                interval = Some(CuttingHeights {
                    top: z,
                    bottom: z - 1.,
                });
                pieces.push(points);
                continue;
            }
            let wall = edge
                .face_ids
                .iter()
                .filter_map(|id| view.face(*id).ok())
                .find(|face| view.vertical(face))
                .ok_or_else(|| {
                    let faces = edge.face_ids.iter().filter_map(|id| view.face(*id).ok())
                        .map(|face| {
                            let reason = match &face.surface {
                                CadSurface::Plane { normal, .. } => {
                                    let n = view.vector(*normal);
                                    format!("plane normal [{:.6}, {:.6}, {:.6}] in setup axes", n[0], n[1], n[2])
                                }
                                CadSurface::Cylinder { .. } | CadSurface::LinearExtrusion { .. } =>
                                    "wall axis is not parallel to setup Z".into(),
                                CadSurface::Torus { .. } => "rounded torus surface".into(),
                                CadSurface::Cone { .. } => "conical wall".into(),
                                CadSurface::Sphere { .. } => "spherical wall".into(),
                                CadSurface::Unsupported { description } =>
                                    format!("unsupported surface: {description}"),
                            };
                            format!("Face {} ({reason})", face.id)
                        }).collect::<Vec<_>>().join(", ");
                    format!("Profile Edge {} lies on setup Z {z:.6} mm, but has no adjoining vertical wall to certify a constant-depth Chain cut. Adjoining faces: {faces}. For an operation-defined depth along this planar path, use Open mode in a Chain geometry entry. For a whole-body outside profile including rounded transitions, use a Silhouette geometry entry", edge.id)
                })?;
            let (lo, hi) = view.extent(wall)?;
            view.certify_open_profile_wall(wall, lo, hi)
                .map_err(|error| format!("Profile Edge {} / Face {}: {error}", edge.id, wall.id))?;
            if (hi - z).abs() > T && (lo - z).abs() > T {
                return Err(format!(
                    "Profile Edge {}: select the upper rim or lower floor edge of Face {}",
                    edge.id, wall.id
                ));
            }
            if hi < view.bounds(view.body.bounds)[1][2] - T {
                return Err(format!("Profile Edge {} / Face {}: wall top at setup Z {hi:.6} mm is below the body's accessible entry plane", edge.id, wall.id));
            }
            let current = CuttingHeights {
                top: hi,
                bottom: lo,
            };
            if interval.is_some_and(|old| (old.top - hi).abs() > T || (old.bottom - lo).abs() > T) {
                return Err(format!("Profile Edge {} / Face {}: Chain edges have different cutting heights; wall interval is {lo:.6} to {hi:.6} mm", edge.id, wall.id));
            }
            interval = Some(current);
            walls.insert(id, wall);
            pieces.push(points);
        }
        let plane = pieces[0][0][2];
        if pieces.iter().flatten().any(|p| (p[2] - plane).abs() > T) {
            return Err("Profile chain edges lie on different setup Z planes; select one rim or one floor chain".into());
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
        if closed && !open_curves {
            let interval = interval.unwrap();
            for wall in walls.values() {
                view.certify_wall(wall, interval.bottom, interval.top)?;
            }
        }
        // Infer the free side locally from the certified wall's outward normal.
        // The rest of the upward face may contain unsupported trim curves.
        // Joining can reverse the source edge, so use the actual first segment.
        let mut side = None;
        for reference in refs {
            let edge = view.edge(reference.id)?;
            let samples = view.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)?;
            let starts_here = samples.windows(2).any(|pair| {
                (distance(points[0], pair[0]) < T && distance(points[1], pair[1]) < T)
                    || (distance(points[0], pair[1]) < T && distance(points[1], pair[0]) < T)
            });
            if !starts_here {
                continue;
            }
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
                    if open_curves {
                        // An oriented upward face boundary has material on its
                        // left. Reversing the edge swaps the free side.
                        let forward = samples.windows(2).any(|pair| {
                            distance(points[0], pair[0]) < T && distance(points[1], pair[1]) < T
                        });
                        side = Some(if forward != usage.reversed {
                            Side::Inside
                        } else {
                            Side::Outside
                        });
                        break;
                    }
                    let wall = walls[&edge.id];
                    if closed {
                        side = Some(if wire.is_outer {
                            Side::Outside
                        } else {
                            Side::Inside
                        });
                        break;
                    }
                    if let CadSurface::LinearExtrusion { edge_sides, .. } = &wall.surface {
                        let local = edge_sides.iter().find(|side| side.edge_id == edge.id)
                            .ok_or("Spline wall is missing its exact edge orientation; reimport the CAD model")?;
                        let tangent = view.vector(local.tangent);
                        let normal = view.vector(local.outward);
                        let mut left_dot = -tangent[1] * normal[0] + tangent[0] * normal[1];
                        let forward = samples.windows(2).any(|pair| {
                            distance(points[0], pair[0]) < T && distance(points[1], pair[1]) < T
                        });
                        if !forward {
                            left_dot = -left_dot;
                        }
                        if left_dot.abs() < T {
                            return Err("Cannot determine the free side of the spline wall".into());
                        }
                        side = Some(if left_dot > 0. {
                            Side::Outside
                        } else {
                            Side::Inside
                        });
                        break;
                    }
                    let midpoint: [f64; 3] =
                        std::array::from_fn(|i| (points[0][i] + points[1][i]) * 0.5);
                    let normal = match wall.surface {
                        CadSurface::Plane { normal, .. } => view.vector(normal),
                        CadSurface::Cylinder {
                            origin,
                            axis,
                            inward,
                            ..
                        } => {
                            let origin = view.point(origin);
                            let axis = view.vector(axis);
                            let radial: [f64; 3] = std::array::from_fn(|i| midpoint[i] - origin[i]);
                            let axial: f64 = (0..3).map(|i| radial[i] * axis[i]).sum();
                            let sign = if inward { -1. } else { 1. };
                            std::array::from_fn(|i| sign * (radial[i] - axial * axis[i]))
                        }
                        _ => unreachable!("certified vertical wall"),
                    };
                    let tangent = [points[1][0] - points[0][0], points[1][1] - points[0][1]];
                    let left_dot = -tangent[1] * normal[0] + tangent[0] * normal[1];
                    if left_dot.abs() < T * T {
                        return Err("Cannot determine the free side of the profile wall".into());
                    }
                    side = Some(if left_dot > 0. {
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
