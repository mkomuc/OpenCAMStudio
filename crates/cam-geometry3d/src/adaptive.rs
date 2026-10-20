//! Conservative 3D adaptive roughing shadows from the independent, bounded-
//! deflection CAD projection mesh. The viewport mesh is never used.
use super::*;

/// Validate the adaptive-only multi-reference selection contract.
pub fn validate_adaptive_selection(
    state: &GeometryState,
    selected: &[EntityRef],
) -> Result<(), String> {
    if selected.is_empty() {
        return Err("Select at least one body or face for 3D Adaptive".into());
    }
    let mut unique = BTreeSet::new();
    for reference in selected {
        if !matches!(reference.kind, EntityKind::Body | EntityKind::Face) {
            return Err("3D Adaptive accepts body and face references only".into());
        }
        if !unique.insert(reference) {
            return Err("3D Adaptive selection contains a duplicate reference".into());
        }
        if !state.entity_exists(reference) {
            return Err("Needs reselection: missing entity or stale asset revision".into());
        }
        let asset = state
            .asset(reference.asset_id)
            .ok_or("selected asset missing")?;
        let instance = state
            .instance(reference.instance_id)
            .ok_or("selected instance missing")?;
        if !instance.placement.is_rigid() {
            return Err("invalid machining placement".into());
        }
        asset.topology.validate()?;
        let body = asset
            .topology
            .bodies
            .iter()
            .find(|body| body.id == reference.body_id)
            .ok_or("selected body missing")?;
        if !body.valid_solid {
            return Err(format!(
                "Body {} is not a valid solid and cannot drive 3D Adaptive",
                body.name
            ));
        }
    }
    Ok(())
}

pub fn adaptive_heights(
    state: &GeometryState,
    selected: &[EntityRef],
    stock_top: f64,
) -> Result<CuttingHeights, String> {
    if !stock_top.is_finite() {
        return Err("stock top must be finite".into());
    }
    validate_adaptive_selection(state, selected)?;
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for reference in selected {
        let asset = state
            .asset(reference.asset_id)
            .ok_or("selected asset missing")?;
        let instance = state
            .instance(reference.instance_id)
            .ok_or("selected instance missing")?;
        let local = if reference.kind == EntityKind::Body {
            asset
                .topology
                .bodies
                .iter()
                .find(|body| body.id == reference.body_id)
                .ok_or("selected body missing")?
                .bounds
        } else {
            asset
                .topology
                .faces
                .iter()
                .find(|face| face.id == reference.id && face.body_id == reference.body_id)
                .ok_or("selected face missing")?
                .bounds
        };
        let world = transformed_bounds(local, instance.placement);
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(world[0][axis]);
            bounds[1][axis] = bounds[1][axis].max(world[1][axis]);
        }
    }
    let selected_bodies: BTreeSet<_> = selected
        .iter()
        .filter(|reference| reference.kind == EntityKind::Body)
        .map(|reference| (reference.asset_id, reference.instance_id, reference.body_id))
        .collect();
    let has_face = selected.iter().any(|reference| {
        reference.kind == EntityKind::Face
            && !selected_bodies.contains(&(
                reference.asset_id,
                reference.instance_id,
                reference.body_id,
            ))
    });
    let top = if has_face {
        stock_top.max(bounds[1][2])
    } else {
        bounds[1][2]
    };
    let heights = CuttingHeights {
        top,
        bottom: bounds[0][2],
    };
    if heights.top <= heights.bottom {
        return Err(
            "Adaptive selection has no cutting depth; raise stock top or select geometry below it"
                .into(),
        );
    }
    heights.validate()?;
    Ok(heights)
}

fn checked_projection(topology: &TopologySnapshot) -> Result<&CadProjectionMesh, String> {
    let projection = topology.projection.as_ref().ok_or(
        "This solid has no independent CAD machining projection; reimport it before 3D Adaptive",
    )?;
    if !projection.chord_tolerance.is_finite()
        || projection.chord_tolerance <= 0.0
        || projection.chord_tolerance > 0.0025 + T
    {
        return Err("CAD machining projection exceeds the 0.0025 mm Adaptive error budget; reimport the solid".into());
    }
    Ok(projection)
}

fn triangles_for_faces(
    mesh: &CadMesh,
    faces: &[EntityId],
    placement: RigidTransform,
) -> Result<Vec<[[f64; 3]; 3]>, String> {
    mesh.triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, face)| faces.contains(face))
        .map(|(indices, _)| {
            indices
                .iter()
                .map(|index| {
                    let point = mesh.vertices.get(*index as usize).copied().ok_or_else(|| {
                        "CAD machining projection has an invalid vertex index".to_string()
                    })?;
                    Ok(transform_point(placement, point))
                })
                .collect::<Result<Vec<_>, String>>()?
                .try_into()
                .map_err(|_| "CAD machining projection triangle is malformed".to_string())
        })
        .collect()
}

fn transform_point(transform: RigidTransform, point: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| {
        (0..3)
            .map(|j| transform.rotation[i][j] * point[j])
            .sum::<f64>()
            + transform.translation[i]
    })
}

fn transformed_bounds(bounds: [[f64; 3]; 2], placement: RigidTransform) -> [[f64; 3]; 2] {
    let mut out = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for x in [bounds[0][0], bounds[1][0]] {
        for y in [bounds[0][1], bounds[1][1]] {
            for z in [bounds[0][2], bounds[1][2]] {
                let point = transform_point(placement, [x, y, z]);
                for axis in 0..3 {
                    out[0][axis] = out[0][axis].min(point[axis]);
                    out[1][axis] = out[1][axis].max(point[axis]);
                }
            }
        }
    }
    out
}

fn bounds_polygon(bounds: [[f64; 3]; 2]) -> Result<Polygon, String> {
    Polygon::new(Contour::new(vec![
        Point::new(bounds[0][0], bounds[0][1]),
        Point::new(bounds[1][0], bounds[0][1]),
        Point::new(bounds[1][0], bounds[1][1]),
        Point::new(bounds[0][0], bounds[1][1]),
    ]))
    .map_err(|e| e.to_string())
}

fn project_triangles(triangles: &[[[f64; 3]; 3]]) -> Result<Vec<Polygon>, String> {
    let mut polygons = Vec::new();
    for triangle in triangles {
        let contour = Contour::new(
            triangle
                .iter()
                .map(|point| Point::new(point[0], point[1]))
                .collect(),
        );
        if contour.area().abs() > cam_geo::GRID_MM * cam_geo::GRID_MM {
            polygons.push(
                Polygon::new(contour)
                    .map_err(|e| format!("invalid selected-face machining projection: {e}"))?,
            );
        }
    }
    if polygons.is_empty() {
        return Ok(polygons);
    }
    cam_geo::union(&polygons, &[])
        .map_err(|e| format!("could not union selected-face machining projection: {e}"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveLayer {
    /// Tool-tip Z for this level.
    pub z: f64,
    /// XY shadow of all solid material at or above `z`. Keeping this entire
    /// shadow clear of the cutter protects the tool profile, including overhangs.
    /// Internal voids are filled conservatively and may leave stock.
    pub protected: Vec<Polygon>,
    /// Projected boundaries that limit where this operation may clear stock.
    /// Body bounds are expanded by the toolpath strategy to provide local runout.
    pub body_domains: Vec<Polygon>,
    /// Projected selected face meshes. The clearing strategy keeps the swept
    /// cutter inside these domains.
    pub face_domains: Vec<Polygon>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveGeometry {
    pub layers: Vec<AdaptiveLayer>,
    pub fingerprint: String,
    /// Outward CAD approximation allowance, mm.
    pub error_bound: f64,
}

/// User and setup inputs for conservative adaptive geometry resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveResolveConfig {
    pub heights: CuttingHeights,
    pub stock_to_leave: StockToLeave,
    pub stepdown: f64,
    pub fine_stepdown: f64,
    pub tolerance: f64,
}

/// Resolve a CAD solid into top-down tool-tip levels. Each level protects the
/// complete projected shadow above the cutter tip, so a higher boss or an
/// overhang remains an obstacle even when it is outside the nominal stepdown.
pub fn resolve_adaptive(
    state: &GeometryState,
    binding: &GeometryBinding,
    config: AdaptiveResolveConfig,
    cancelled: impl Fn() -> bool,
) -> Result<AdaptiveGeometry, String> {
    let AdaptiveResolveConfig {
        heights,
        stock_to_leave,
        stepdown,
        fine_stepdown,
        tolerance,
    } = config;
    if binding.recipe != ExtractionRecipe::AdaptiveClearing {
        return Err("Adaptive clearing requires an AdaptiveClearing geometry binding".into());
    }
    validate_adaptive_selection(state, &binding.selected)?;
    heights.validate()?;
    if !stepdown.is_finite() || stepdown <= 0.0 {
        return Err("Adaptive stepdown must be finite and positive".into());
    }
    if !fine_stepdown.is_finite() || fine_stepdown <= 0.0 {
        return Err("Adaptive fine stepdown must be finite and positive".into());
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("Adaptive tolerance must be finite and positive".into());
    }
    if !stock_to_leave.radial.is_finite()
        || stock_to_leave.radial < 0.0
        || !stock_to_leave.axial.is_finite()
        || stock_to_leave.axial < 0.0
    {
        return Err("Adaptive stock allowance must be finite and nonnegative".into());
    }
    let axial = if stock_to_leave.enabled {
        stock_to_leave.axial
    } else {
        0.0
    };
    let bottom = heights.bottom + axial;
    if bottom >= heights.top {
        return Err("Adaptive axial stock allowance leaves no cutting depth".into());
    }
    let selected_bodies: BTreeSet<_> = binding
        .selected
        .iter()
        .filter(|r| r.kind == EntityKind::Body)
        .map(|r| (r.asset_id, r.instance_id, r.body_id))
        .collect();
    let legacy_full_stock = state.instances.len() == 1
        && selected_bodies.contains(&(
            state.instances[0].asset_id,
            state.instances[0].id,
            state.instances[0].body_id,
        ))
        && binding.selected.iter().all(|r| {
            matches!(r.kind, EntityKind::Body | EntityKind::Face)
                && (r.kind == EntityKind::Body
                    || selected_bodies.contains(&(r.asset_id, r.instance_id, r.body_id)))
        });
    let mut body_domains = Vec::new();
    let mut face_domains = Vec::new();
    let mut selected_face_floors = Vec::new();
    for reference in &binding.selected {
        if reference.kind == EntityKind::Face
            && selected_bodies.contains(&(
                reference.asset_id,
                reference.instance_id,
                reference.body_id,
            ))
        {
            continue;
        }
        let asset = state
            .asset(reference.asset_id)
            .ok_or("selected asset missing")?;
        let instance = state
            .instance(reference.instance_id)
            .ok_or("selected instance missing")?;
        let body = asset
            .topology
            .bodies
            .iter()
            .find(|b| b.id == reference.body_id)
            .ok_or("selected body missing")?;
        if reference.kind == EntityKind::Body {
            body_domains.push(bounds_polygon(transformed_bounds(
                body.bounds,
                instance.placement,
            ))?);
        } else {
            let face = asset
                .topology
                .faces
                .iter()
                .find(|f| f.id == reference.id && f.body_id == body.id)
                .ok_or("selected face missing")?;
            selected_face_floors.push(transformed_bounds(face.bounds, instance.placement)[0][2]);
            let projection = checked_projection(&asset.topology)?;
            let triangles =
                triangles_for_faces(&projection.mesh, &[reference.id], instance.placement)?;
            let polygons = project_triangles(&triangles)?;
            if polygons.is_empty() {
                return Err(format!("Selected face {} has no positive-area XY projection in the setup; select a face with an XY footprint", reference.id));
            }
            face_domains.extend(polygons);
        }
    }
    // Obstacles include every solid body instance in the setup, regardless of selection.
    let mut triangles = Vec::new();
    let mut obstacle_error_bound: f64 = 0.0;
    for instance in &state.instances {
        if !instance.placement.is_rigid() {
            return Err(format!("Setup instance {} has an invalid placement and cannot be certified as a 3D Adaptive obstacle", instance.name));
        }
        let asset = state
            .asset(instance.asset_id)
            .ok_or("setup model asset missing")?;
        asset.topology.validate()?;
        let body = asset
            .topology
            .bodies
            .iter()
            .find(|b| b.id == instance.body_id)
            .ok_or("setup model body missing")?;
        if !body.valid_solid {
            return Err(format!("Setup body {} is not a valid solid and cannot be certified as a 3D Adaptive obstacle", body.name));
        }
        let projection = checked_projection(&asset.topology)?;
        let face_ids: Vec<_> = asset
            .topology
            .faces
            .iter()
            .filter(|f| f.body_id == body.id)
            .map(|f| f.id)
            .collect();
        let covered: BTreeSet<_> = projection
            .mesh
            .triangle_faces
            .iter()
            .copied()
            .filter(|id| face_ids.contains(id))
            .collect();
        let expected: BTreeSet<_> = face_ids.iter().copied().collect();
        if expected.is_empty()
            || covered != expected
            || projection.mesh.triangles.len() != projection.mesh.triangle_faces.len()
        {
            return Err(format!("Incomplete CAD machining projection for setup body {}; reimport it before Adaptive clearing", body.name));
        }
        triangles.extend(triangles_for_faces(
            &projection.mesh,
            &face_ids,
            instance.placement,
        )?);
        obstacle_error_bound = obstacle_error_bound.max(projection.chord_tolerance);
    }
    if triangles.is_empty() {
        return Err("CAD machining projections contain no solid triangles".into());
    }
    if legacy_full_stock {
        body_domains.clear();
    }
    // Existing ring emission may refit line paths as arcs within 0.01 mm. Add
    // that displacement to the protected envelope before constructing toolpaths.
    let error_bound = obstacle_error_bound + 0.01 + 2.0 * cam_geo::GRID_MM;
    if tolerance + T < error_bound {
        return Err(format!("Adaptive tolerance {tolerance:.4} mm is below the certified machining error {error_bound:.4} mm"));
    }
    let mut tips = vec![heights.top];
    let fine = fine_stepdown.min(stepdown);
    let mut coarse_top = heights.top;
    while coarse_top - bottom > 1e-9 {
        let base = (coarse_top - stepdown).max(bottom);
        if base >= coarse_top {
            return Err("Adaptive stepdown is too small for the coordinate range".into());
        }
        // Rough to the base of this axial slice first, then clear finer levels
        // upward through the slice as a finishing refinement.
        tips.push(base);
        let mut z = base + fine;
        while z < coarse_top - 1e-9 {
            tips.push(z);
            z += fine;
            if tips.len() > 100_000 {
                return Err("Adaptive level count exceeds its resource limit".into());
            }
        }
        coarse_top = base;
        if tips.len() > 100_000 {
            return Err("Adaptive level count exceeds its resource limit".into());
        }
    }
    if tips.len() > 100_000 {
        return Err("Adaptive level count exceeds its resource limit".into());
    }
    // Add a reachable pass just above every explicitly selected face floor. At
    // the exact floor Z, the conservative obstacle shadow includes the face and
    // blocks the entire machining domain; the certified lift keeps the cutter
    // clear while still allowing it to remove the remaining face stock.
    for floor in selected_face_floors {
        let reachable = floor + axial + error_bound + cam_geo::GRID_MM;
        if reachable < heights.top - T
            && reachable >= bottom - T
            && !tips.iter().any(|tip| (tip - reachable).abs() <= 1e-9)
        {
            tips.push(reachable);
            if tips.len() > 100_000 {
                return Err("Adaptive level count exceeds its resource limit".into());
            }
        }
    }
    let mut layers = Vec::with_capacity(tips.len());
    for tip in tips {
        if cancelled() {
            return Err("Adaptive geometry resolution cancelled".into());
        }
        let mut projected = Vec::new();
        for tri in &triangles {
            if cancelled() {
                return Err("Adaptive geometry resolution cancelled".into());
            }
            let clipped = clip_above(*tri, tip - axial - error_bound);
            if clipped.len() < 3 {
                continue;
            }
            let contour = Contour::new(
                clipped
                    .into_iter()
                    .map(|p| Point::new(p[0], p[1]))
                    .collect(),
            );
            if contour.area() > cam_geo::GRID_MM * cam_geo::GRID_MM {
                projected.push(
                    Polygon::new(contour)
                        .map_err(|e| format!("invalid CAD machining projection section: {e}"))?,
                );
            }
        }
        let protected = if projected.is_empty() {
            Vec::new()
        } else {
            cam_geo::union(&projected, &[])
                .map_err(|e| format!("could not union CAD machining projection: {e}"))?
        };
        layers.push(AdaptiveLayer {
            z: tip,
            protected,
            body_domains: body_domains.clone(),
            face_domains: face_domains.clone(),
        });
    }
    Ok(AdaptiveGeometry {
        layers,
        fingerprint: state.binding_fingerprint(binding),
        error_bound,
    })
}

/// Clip one triangle by the half-space z >= plane. The resulting XY polygon is
/// a conservative part shadow: material inside a closed solid is projected by
/// its upper boundary; vertical faces add only boundary traces.
fn clip_above(triangle: [[f64; 3]; 3], plane: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for i in 0..3 {
        let a = triangle[i];
        let b = triangle[(i + 1) % 3];
        let a_in = a[2] >= plane;
        let b_in = b[2] >= plane;
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (plane - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, plane]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[allow(clippy::too_many_arguments)]
    fn resolve_adaptive(
        state: &GeometryState,
        binding: &GeometryBinding,
        heights: CuttingHeights,
        stock_to_leave: StockToLeave,
        stepdown: f64,
        fine_stepdown: f64,
        tolerance: f64,
        cancelled: impl Fn() -> bool,
    ) -> Result<AdaptiveGeometry, String> {
        super::resolve_adaptive(
            state,
            binding,
            AdaptiveResolveConfig {
                heights,
                stock_to_leave,
                stepdown,
                fine_stepdown,
                tolerance,
            },
            cancelled,
        )
    }

    fn state(overhang: bool) -> (GeometryState, GeometryBinding) {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut triangle_faces = Vec::new();
        let mut faces = Vec::new();
        let mut add_cap = |id: u64, x0: f64, x1: f64, y0: f64, y1: f64, z: f64| {
            let base = vertices.len() as u32;
            vertices.extend([[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]);
            triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
            triangle_faces.extend([id, id]);
            faces.push(CadFace {
                id,
                body_id: 1,
                surface: CadSurface::Plane {
                    origin: [x0, y0, z],
                    normal: [0., 0., 1.],
                },
                loop_ids: vec![],
                bounds: [[x0, y0, z], [x1, y1, z]],
            });
        };
        if overhang {
            add_cap(1, 2., 8., 2., 8., 5.);
            add_cap(2, 0., 10., 0., 10., 10.);
        } else {
            add_cap(1, 0., 10., 0., 10., 5.);
            add_cap(2, 2., 8., 2., 8., 10.);
        }
        let topology = TopologySnapshot {
            bodies: vec![CadBody {
                id: 1,
                name: "stepped test body".into(),
                valid_solid: true,
                bounds: [[0., 0., 0.], [10., 10., 10.]],
            }],
            faces,
            projection: Some(CadProjectionMesh {
                mesh: CadMesh {
                    vertices,
                    triangles,
                    triangle_faces,
                },
                chord_tolerance: 0.0025,
            }),
            ..Default::default()
        };
        let state = GeometryState {
            assets: vec![GeometryAsset {
                import_options: None,
                id: 1,
                revision: 1,
                name: "test".into(),
                source_format: CadFormat::Step,
                source_data: Arc::new(vec![]),
                source_units: SourceUnits::Millimetres,
                backend: "test".into(),
                import_diagnostics: vec![],
                source_fingerprint: String::new(),
                topology: Arc::new(topology),
            }],
            instances: vec![GeometryInstance {
                id: 1,
                asset_id: 1,
                body_id: 1,
                name: "part".into(),
                placement: Default::default(),
                visible: true,
            }],
            ..Default::default()
        };
        let binding = GeometryBinding {
            selected: vec![EntityRef {
                asset_id: 1,
                revision: 1,
                instance_id: 1,
                body_id: 1,
                kind: EntityKind::Body,
                id: 1,
            }],
            recipe: ExtractionRecipe::AdaptiveClearing,
            heights: CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            overrides: Default::default(),
            status: ResolutionStatus::Resolved,
            resolved_fingerprint: None,
        };
        (state, binding)
    }

    fn bounds(poly: &Polygon) -> ([f64; 2], [f64; 2]) {
        let pts = poly.outer().points();
        (
            [
                pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            ],
            [
                pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
                pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
            ],
        )
    }

    #[test]
    fn adaptive_uses_variable_height_cad_projection_and_ignores_display_mesh() {
        let (mut state, binding) = state(false);
        let topo = Arc::make_mut(&mut state.assets[0].topology);
        topo.mesh = CadMesh {
            vertices: vec![
                [1000., 1000., 1000.],
                [1010., 1000., 1000.],
                [1000., 1010., 1000.],
            ],
            triangles: vec![[0, 1, 2]],
            triangle_faces: vec![1],
        };
        let result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave {
                enabled: false,
                radial: 0.,
                axial: 0.,
            },
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap();
        let levels: Vec<_> = result.layers.iter().map(|layer| layer.z).collect();
        assert_eq!(levels, vec![10., 6., 7., 8., 9., 2., 3., 4., 5., 0., 1.]);
        let high = bounds(&result.layers[1].protected[0]);
        let low = bounds(&result.layers[5].protected[0]);
        assert!(
            (high.0[0] - 2.).abs() < 0.01 && (high.1[0] - 8.).abs() < 0.01,
            "upper step shadow: {high:?}"
        );
        assert!(
            (low.0[0] - 0.).abs() < 0.01 && (low.1[0] - 10.).abs() < 0.01,
            "lower ledge shadow: {low:?}"
        );
        assert!(result.error_bound > 0.01);
    }

    #[test]
    fn incomplete_or_loose_projection_fails_closed() {
        let (mut state, binding) = state(false);
        Arc::make_mut(&mut state.assets[0].topology)
            .projection
            .as_mut()
            .unwrap()
            .chord_tolerance = 0.01;
        assert!(resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.
            },
            StockToLeave {
                enabled: false,
                radial: 0.,
                axial: 0.
            },
            5.,
            5.,
            0.02,
            || false
        )
        .is_err());
    }

    #[test]
    fn upper_overhang_remains_in_the_protected_shadow_below_its_shelf() {
        let (state, binding) = state(true);
        let result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave {
                enabled: false,
                radial: 0.,
                axial: 0.,
            },
            3.,
            3.,
            0.02,
            || false,
        )
        .unwrap();
        let at_z7 = bounds(&result.layers[1].protected[0]);
        assert!(
            (at_z7.0[0] - 0.).abs() < 0.01 && (at_z7.1[0] - 10.).abs() < 0.01,
            "overhang shadow: {at_z7:?}"
        );
    }

    #[test]
    fn axial_allowance_protects_intermediate_shelves_as_well_as_the_floor() {
        let (state, binding) = state(false);
        let no_allowance = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave {
                enabled: false,
                radial: 0.,
                axial: 0.,
            },
            4.5,
            4.5,
            0.02,
            || false,
        )
        .unwrap();
        let with_allowance = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave {
                enabled: true,
                radial: 0.,
                axial: 0.5,
            },
            4.5,
            4.5,
            0.02,
            || false,
        )
        .unwrap();
        assert_eq!(no_allowance.layers[1].z, 5.5);
        assert_eq!(with_allowance.layers[1].z, 5.5);
        let no_skin = bounds(&no_allowance.layers[1].protected[0]);
        let skin = bounds(&with_allowance.layers[1].protected[0]);
        assert!(
            no_skin.1[0] < 9.0,
            "without axial stock the lower shelf is below the tool: {no_skin:?}"
        );
        assert!(
            (skin.0[0] - 0.).abs() < 0.01 && (skin.1[0] - 10.).abs() < 0.01,
            "axial allowance protects the lower shelf in the full tool shadow: {skin:?}"
        );
    }

    #[test]
    fn geometry_resolution_honors_cancellation() {
        let (state, binding) = state(false);
        let result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || true,
        );
        assert!(result.unwrap_err().contains("cancelled"));
    }

    #[test]
    fn every_setup_instance_remains_an_obstacle_and_subset_body_is_local() {
        let (mut state, binding) = state(false);
        let one_instance_fingerprint = state.binding_fingerprint(&binding);
        let mut neighbor = state.instances[0].clone();
        neighbor.id = 2;
        neighbor.name = "neighbor".into();
        neighbor.placement.translation = [15., 0., 0.];
        state.instances.push(neighbor);
        let resolved = resolve_binding(&state, &binding)
            .expect("adaptive binding resolution supports additional setup instances");
        assert_eq!(resolved.fingerprint, state.binding_fingerprint(&binding));
        let result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        );
        let result = result.unwrap();
        let layer = &result.layers[0];
        assert!(
            !layer.body_domains.is_empty(),
            "a subset selection gets a local body domain"
        );
        assert!(
            layer.protected.iter().any(|poly| bounds(poly).0[0] >= 15.0),
            "the unselected setup instance remains a protected obstacle"
        );
        assert_ne!(
            result.fingerprint, one_instance_fingerprint,
            "all setup instances enter the cache dependency"
        );

        let mut invalid_neighbor = state.clone();
        invalid_neighbor.instances[1].placement.rotation[0][0] = 2.0;
        let error = resolve_adaptive(
            &invalid_neighbor,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap_err();
        assert!(error.contains("invalid placement"), "{error}");
    }

    #[test]
    fn selected_face_uses_its_projected_mesh_and_old_body_default_keeps_full_stock() {
        let (state, mut binding) = state(false);
        let body_result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap();
        assert!(body_result.layers[0].body_domains.is_empty());
        assert!(body_result.layers[0].face_domains.is_empty());

        let body_binding = binding.clone();
        binding.selected[0].kind = EntityKind::Face;
        binding.selected[0].id = 1;
        let face_result = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 5.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap();
        assert!(face_result.layers[0].body_domains.is_empty());
        assert!(!face_result.layers[0].face_domains.is_empty());
        let face_bounds = bounds(&face_result.layers[0].face_domains[0]);
        assert_eq!(face_bounds, ([0., 0.], [10., 10.]));
        assert_eq!(
            adaptive_heights(&state, &binding.selected, 12.).unwrap(),
            CuttingHeights {
                top: 12.,
                bottom: 5.
            }
        );

        let mut body_and_face = body_binding;
        let body_ref = body_and_face.selected[0].clone();
        body_and_face.selected.push(EntityRef {
            kind: EntityKind::Face,
            id: 1,
            ..body_ref
        });
        let redundant = resolve_adaptive(
            &state,
            &body_and_face,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap();
        assert!(
            redundant.layers[0].body_domains.is_empty(),
            "body selection subsumes same-body faces"
        );
        assert_eq!(
            adaptive_heights(&state, &body_and_face.selected, 12.).unwrap(),
            CuttingHeights {
                top: 10.,
                bottom: 0.
            }
        );

        let mut top_face = binding.clone();
        top_face.selected[0].id = 2;
        let floor_heights = adaptive_heights(&state, &top_face.selected, 12.).unwrap();
        assert_eq!(
            floor_heights,
            CuttingHeights {
                top: 12.,
                bottom: 10.
            }
        );
        let floor_pass = resolve_adaptive(
            &state,
            &top_face,
            floor_heights,
            StockToLeave::default(),
            10.,
            10.,
            0.02,
            || false,
        )
        .unwrap();
        let reachable = floor_pass
            .layers
            .iter()
            .find(|layer| layer.z > 10. && layer.z < 10.1)
            .expect("selected floor gets a reachable pass above its surface");
        assert!(
            reachable.protected.is_empty(),
            "the certified lift clears the selected floor shadow"
        );

        let near_floor_tip = 10. + floor_pass.error_bound;
        let stepdown = floor_heights.top - near_floor_tip;
        let near_existing = resolve_adaptive(
            &state,
            &top_face,
            floor_heights,
            StockToLeave::default(),
            stepdown,
            stepdown,
            0.02,
            || false,
        )
        .unwrap();
        assert!(near_existing
            .layers
            .iter()
            .any(|layer| (layer.z - near_floor_tip).abs() < 1e-9));
        assert!(
            near_existing
                .layers
                .iter()
                .any(|layer| (layer.z - reachable.z).abs() < 1e-9),
            "a tip one grid below the certified floor lift must not suppress the safe level"
        );
    }

    #[test]
    fn vertical_face_projection_and_zero_depth_selection_fail_clearly() {
        let (mut state, mut binding) = state(false);
        let topology = Arc::make_mut(&mut state.assets[0].topology);
        topology.faces.push(CadFace {
            id: 3,
            body_id: 1,
            surface: CadSurface::Plane {
                origin: [0., 0., 0.],
                normal: [1., 0., 0.],
            },
            loop_ids: vec![],
            bounds: [[0., 0., 0.], [0., 10., 10.]],
        });
        let mesh = &mut topology.projection.as_mut().unwrap().mesh;
        let base = mesh.vertices.len() as u32;
        mesh.vertices
            .extend([[0., 0., 0.], [0., 10., 0.], [0., 10., 10.]]);
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.triangle_faces.push(3);
        binding.selected = vec![EntityRef {
            kind: EntityKind::Face,
            id: 3,
            ..binding.selected[0].clone()
        }];
        let error = resolve_adaptive(
            &state,
            &binding,
            CuttingHeights {
                top: 10.,
                bottom: 0.,
            },
            StockToLeave::default(),
            4.,
            1.,
            0.02,
            || false,
        )
        .unwrap_err();
        assert!(error.contains("no positive-area XY projection"), "{error}");

        binding.selected[0].id = 2;
        assert!(adaptive_heights(&state, &binding.selected, 10.)
            .unwrap_err()
            .contains("no cutting depth"));
    }

    #[test]
    fn adaptive_selection_rejects_duplicate_and_stale_references() {
        let (state, binding) = state(false);
        let duplicate = [binding.selected[0].clone(), binding.selected[0].clone()];
        assert!(validate_adaptive_selection(&state, &duplicate)
            .unwrap_err()
            .contains("duplicate"));
        let mut stale = binding.selected[0].clone();
        stale.revision += 1;
        assert!(validate_adaptive_selection(&state, &[stale])
            .unwrap_err()
            .contains("stale"));
    }
}
