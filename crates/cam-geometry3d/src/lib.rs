//! Fixed-axis feature extraction. Analytic operations use exact topology;
//! silhouettes use a separate CAD projection tessellation with an explicit
//! allowance. Display meshes never enter machining derivation.
mod profile_contours;
mod profile_entries;
use cam_geo::{Contour, Point, Polygon};
use cam_model::*;
pub use profile_contours::coplanar_faces as coplanar_profile_faces;
pub use profile_entries::profile_entry_contours;
use std::{collections::BTreeSet, f64::consts::TAU};

/// Recognition and machining budgets are independent of display tessellation.
pub const RECOGNITION_TOLERANCE: f64 = 1e-5;
pub const MACHINING_CHORD_TOLERANCE: f64 = 0.01;
const T: f64 = RECOGNITION_TOLERANCE;
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedHole {
    pub center: Point,
    pub diameter: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedInput {
    pub profiles: Vec<ProfileContour>,
    pub regions: Vec<Polygon>,
    pub holes: Vec<ResolvedHole>,
    pub heights: CuttingHeights,
    /// Includes topology/projection data, placement, recipe and overrides; excludes the display mesh.
    pub fingerprint: String,
    pub diagnostics: Vec<String>,
}

/// A bore boundary must be cut on its inside. Strategy extraction still checks
/// the complete selected feature; this only chooses the initial profile side.
pub fn profile_side(state: &GeometryState, selected: &[EntityRef]) -> Result<Side, String> {
    let view = View::new(state, selected)?;
    if selected.len() == 1 && selected[0].kind == EntityKind::Silhouette {
        return Ok(Side::Outside);
    }
    if selected.len() != 1 || selected[0].kind != EntityKind::Loop {
        return Err(
            "Profile requires one closed coplanar loop; select one boundary per operation".into(),
        );
    }
    let wire = view.wire(selected[0].id)?;
    for usage in &wire.edge_uses {
        for id in &view.edge(usage.edge_id)?.face_ids {
            if matches!(
                view.face(*id)?.surface,
                CadSurface::Cylinder { inward: true, .. }
            ) {
                return Ok(Side::Inside);
            }
        }
    }
    Ok(Side::Outside)
}

pub fn resolve_selection(
    state: &GeometryState,
    selected: &[EntityRef],
    recipe: ExtractionRecipe,
    stock_top: f64,
) -> Result<GeometryBinding, String> {
    if !stock_top.is_finite() {
        return Err("stock top must be finite".into());
    }
    let mut binding = GeometryBinding {
        selected: selected.to_vec(),
        recipe,
        heights: CuttingHeights {
            top: stock_top,
            bottom: stock_top - 1.0,
        },
        overrides: Default::default(),
        resolved_fingerprint: None,
        status: ResolutionStatus::Resolved,
    };
    let (_, _, heights) = derive(state, &binding, stock_top)?;
    binding.heights = heights;
    Ok(binding)
}

pub fn resolve_binding(
    state: &GeometryState,
    binding: &GeometryBinding,
) -> Result<ResolvedInput, String> {
    if let ExtractionRecipe::ProfileEntries { entries } = &binding.recipe {
        return profile_entries::resolve(state, binding, entries);
    }
    // Cached contour or status is never sufficient: validate every entity revision.
    let (regions, holes, base) = derive(state, binding, binding.heights.top)?;
    let heights = CuttingHeights {
        top: binding.overrides.top.unwrap_or(base.top),
        bottom: binding.overrides.bottom.unwrap_or(base.bottom),
    };
    heights.validate()?;
    if heights.bottom < base.bottom - T {
        return Err("bottom override extends below the geometry-certified cutting interval".into());
    }
    if heights.top < base.top - T {
        return Err("top override skips material above the geometry-certified entry plane".into());
    }
    let view = View::new(state, &binding.selected)?;
    let mut exact = (*view.asset.topology).clone();
    exact.mesh = CadMesh::default();
    let encoded = serde_json::to_vec(&(
        view.asset.id,
        view.asset.revision,
        &exact,
        view.instance.placement,
        &binding.selected,
        &binding.recipe,
        &binding.overrides,
    ))
    .map_err(|e| e.to_string())?;
    // Deterministic FNV-1a dependency fingerprint (not a security/content hash).
    let hash = encoded.into_iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    let diagnostics = if binding
        .selected
        .iter()
        .any(|r| r.kind == EntityKind::Silhouette)
    {
        let tolerance = view
            .asset
            .topology
            .projection
            .as_ref()
            .unwrap()
            .chord_tolerance;
        vec![format!("XY silhouette from independent CAD projection tessellation ({tolerance} mm deflection); outward allowance {} mm. This profiles the whole body envelope, including rounded transitions.", silhouette_allowance(tolerance))]
    } else {
        vec![format!("Exact curves approximated within {MACHINING_CHORD_TOLERANCE} mm; planar grid error ≤ {} mm",cam_geo::GRID_MM/2f64.sqrt())]
    };
    let profiles = if binding.recipe == ExtractionRecipe::ProfileContours {
        profile_contours::extract(state, &binding.selected)?.0
    } else if binding.recipe == ExtractionRecipe::ProfileSilhouette {
        regions
            .iter()
            .flat_map(|region| {
                std::iter::once(ProfileContour {
                    chain: region.outer().clone(),
                    closed: true,
                    side: Side::Outside,
                })
                .chain(region.holes().iter().map(|hole| ProfileContour {
                    chain: hole.clone(),
                    closed: true,
                    side: Side::Inside,
                }))
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(ResolvedInput {
        profiles,
        regions,
        holes,
        heights,
        fingerprint: format!("{hash:016x}"),
        diagnostics,
    })
}
/// Project one placed body onto setup XY. Only the external outline is used;
/// holes are filled because this selection represents an outside profile.
/// Old projects need reimporting to obtain the independent projection data.
pub fn silhouette(state: &GeometryState, reference: &EntityRef) -> Result<Polygon, String> {
    let view = View::new(state, std::slice::from_ref(reference))?;
    view.silhouette()
}
fn silhouette_allowance(tolerance: f64) -> f64 {
    // Cover tessellation deviation and planar-grid rounding. Leave this allowance
    // on the part rather than offsetting a faceted approximation into its surface.
    tolerance + cam_geo::GRID_MM * 2.
}
struct CachedSilhouette {
    topology: std::sync::Weak<TopologySnapshot>,
    body: BodyId,
    placement: RigidTransform,
    include_holes: bool,
    region: Polygon,
}
thread_local! {
    // Hover reuses immutable CAD data. Weak ownership does not keep deleted
    // imports alive; placement and snapshot identity prevent stale outlines.
    static SILHOUETTES: std::cell::RefCell<Vec<CachedSilhouette>> = const { std::cell::RefCell::new(Vec::new()) };
}
struct View<'a> {
    asset: &'a GeometryAsset,
    instance: &'a GeometryInstance,
    body: &'a CadBody,
}
impl<'a> View<'a> {
    fn new(state: &'a GeometryState, refs: &[EntityRef]) -> Result<Self, String> {
        let first = refs.first().ok_or("select geometry first")?;
        if !refs.iter().all(|r| state.entity_exists(r)) {
            return Err("Needs reselection: missing entity or stale asset revision".into());
        }
        if !refs.iter().all(|r| {
            r.instance_id == first.instance_id
                && r.body_id == first.body_id
                && r.asset_id == first.asset_id
        }) {
            return Err("machining requires one explicitly selected body instance".into());
        }
        let asset = state.asset(first.asset_id).unwrap();
        let instance = state.instance(first.instance_id).unwrap();
        if !instance.placement.is_rigid() {
            return Err("invalid machining placement".into());
        }
        asset.topology.validate()?;
        let body = asset
            .topology
            .bodies
            .iter()
            .find(|b| b.id == first.body_id)
            .ok_or("body missing")?;
        if !body.valid_solid {
            return Err("invalid/open solid is inspectable but cannot drive machining".into());
        }
        Ok(Self {
            asset,
            instance,
            body,
        })
    }
    fn silhouette(&self) -> Result<Polygon, String> {
        self.projected_outline(false)
    }
    fn projected_outline(&self, include_holes: bool) -> Result<Polygon, String> {
        if let Some(region) = SILHOUETTES.with(|cache| {
            cache
                .borrow()
                .iter()
                .find(|entry| {
                    entry.body == self.body.id
                        && entry.include_holes == include_holes
                        && entry.placement == self.instance.placement
                        && entry
                            .topology
                            .upgrade()
                            .is_some_and(|t| std::sync::Arc::ptr_eq(&t, &self.asset.topology))
                })
                .map(|entry| entry.region.clone())
        }) {
            return Ok(region);
        }
        let projection = self.asset.topology.projection.as_ref().ok_or(
            "Reimport this STEP file to select its XY silhouette; this project has no independent CAD projection data"
        )?;
        if projection.chord_tolerance > MACHINING_CHORD_TOLERANCE / 2. {
            return Err(
                "CAD projection exceeds the machining tolerance; reimport the STEP file".into(),
            );
        }
        let face_ids: BTreeSet<_> = self
            .asset
            .topology
            .faces
            .iter()
            .filter(|f| f.body_id == self.body.id)
            .map(|f| f.id)
            .collect();
        let covered: BTreeSet<_> = projection
            .mesh
            .triangle_faces
            .iter()
            .copied()
            .filter(|id| face_ids.contains(id))
            .collect();
        if covered != face_ids || covered.is_empty() {
            return Err("Incomplete CAD projection data; reimport the STEP file".into());
        }
        let mut triangles = Vec::new();
        for (indices, face) in projection
            .mesh
            .triangles
            .iter()
            .zip(&projection.mesh.triangle_faces)
        {
            if !face_ids.contains(face) {
                continue;
            }
            let points: Vec<_> = indices
                .iter()
                .map(|i| {
                    let p = self.point(projection.mesh.vertices[*i as usize]);
                    Point::new(p[0], p[1])
                })
                .collect();
            let contour = Contour::new(points);
            // Vertical triangles project to a segment. The planar boolean handles
            // rounding and tiny slivers consistently with every other strategy.
            if contour.area() > 0. {
                triangles.push(Polygon::new(contour).map_err(|e| e.to_string())?);
            }
        }
        let regions = cam_geo::union(&triangles, &[]).map_err(|e| e.to_string())?;
        // Preserve every projected component. Tiny seams introduced by curve
        // discretization/grid rounding close under the declared allowance;
        // genuinely separate outlines still fail below, never become a hull.
        let envelopes: Vec<_> = if include_holes {
            regions
        } else {
            regions
                .iter()
                .map(|region| Polygon::new(region.outer().clone()))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        };
        let expanded = cam_geo::offset(
            &envelopes,
            silhouette_allowance(projection.chord_tolerance),
            cam_geo::JoinStyle::Round,
        )
        .map_err(|e| e.to_string())?;
        let mut expanded = cam_geo::union(&expanded, &[]).map_err(|e| e.to_string())?;
        if expanded.len() != 1 {
            return Err("The projected outline cannot form one profile boundary".into());
        }
        let region = expanded.remove(0);
        SILHOUETTES.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| entry.topology.strong_count() > 0);
            if cache.len() >= 8 {
                cache.remove(0);
            }
            cache.push(CachedSilhouette {
                topology: std::sync::Arc::downgrade(&self.asset.topology),
                body: self.body.id,
                placement: self.instance.placement,
                include_holes,
                region: region.clone(),
            });
        });
        Ok(region)
    }
    fn face(&self, id: u64) -> Result<&CadFace, String> {
        self.asset
            .topology
            .faces
            .iter()
            .find(|f| f.id == id && f.body_id == self.body.id)
            .ok_or_else(|| "face missing".into())
    }
    fn edge(&self, id: u64) -> Result<&CadEdge, String> {
        self.asset
            .topology
            .edges
            .iter()
            .find(|f| f.id == id && f.body_id == self.body.id)
            .ok_or_else(|| "edge missing".into())
    }
    fn wire(&self, id: u64) -> Result<&CadLoop, String> {
        self.asset
            .topology
            .loops
            .iter()
            .find(|f| f.id == id && f.body_id == self.body.id)
            .ok_or_else(|| "loop missing".into())
    }
    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        self.instance.placement.transform_point(p)
    }
    fn vector(&self, p: [f64; 3]) -> [f64; 3] {
        self.instance.placement.transform_vector(p)
    }
    fn plane(&self, f: &CadFace) -> Result<(f64, [f64; 3]), String> {
        match f.surface {
            CadSurface::Plane { origin, normal } => {
                let n = self.vector(normal);
                if n[0].abs() > T || n[1].abs() > T {
                    return Err("selected plane is not aligned with setup Z; align setup Z with the target face normal".into());
                }
                Ok((self.point(origin)[2], n))
            }
            _ => Err("select a planar face".into()),
        }
    }
    /// Conservative B-rep bounds in the machining frame. All eight corners are
    /// needed when the instance or setup is rotated; display meshes are unused.
    fn bounds(&self, bounds: [[f64; 3]; 2]) -> [[f64; 3]; 2] {
        let mut result = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for x in [bounds[0][0], bounds[1][0]] {
            for y in [bounds[0][1], bounds[1][1]] {
                for z in [bounds[0][2], bounds[1][2]] {
                    let p = self.point([x, y, z]);
                    for axis in 0..3 {
                        result[0][axis] = result[0][axis].min(p[axis]);
                        result[1][axis] = result[1][axis].max(p[axis]);
                    }
                }
            }
        }
        result
    }
    fn footprint(&self, bounds: [[f64; 3]; 2]) -> Result<Polygon, String> {
        // Preserve a conservative enclosure after planar-grid quantization,
        // including for zero-width projections.
        let margin = T.max(cam_geo::GRID_MM);
        Polygon::new(Contour::new(vec![
            Point::new(bounds[0][0] - margin, bounds[0][1] - margin),
            Point::new(bounds[1][0] + margin, bounds[0][1] - margin),
            Point::new(bounds[1][0] + margin, bounds[1][1] + margin),
            Point::new(bounds[0][0] - margin, bounds[1][1] + margin),
        ]))
        .map_err(|e| e.to_string())
    }
    fn vertical(&self, f: &CadFace) -> bool {
        match f.surface {
            CadSurface::Plane { normal, .. } => self.vector(normal)[2].abs() < T,
            CadSurface::Cylinder { axis, .. } | CadSurface::LinearExtrusion { axis, .. } => {
                let n = self.vector(axis);
                n[0].abs() < T && n[1].abs() < T
            }
            _ => false,
        }
    }
    fn curve_points(
        &self,
        e: &CadEdge,
        reversed: bool,
        tolerance: f64,
    ) -> Result<Vec<[f64; 3]>, String> {
        let mut p = match e.curve {
            CadCurve::Line { start, end } => vec![self.point(start), self.point(end)],
            CadCurve::CircleArc {
                center,
                normal,
                x_axis,
                radius,
                start_angle,
                end_angle,
            } => {
                let sweep = end_angle - start_angle;
                if sweep.abs() <= T || sweep.abs() > TAU + T {
                    return Err("invalid circular arc parameter interval".into());
                }
                let step = 2.0 * (1.0 - (tolerance / radius).min(1.0)).acos();
                let segments = (sweep.abs() / step).ceil().max(2.0) as usize;
                if segments > 65536 {
                    return Err("machining curve exceeds approximation resource budget".into());
                }
                let y = cross(normal, x_axis);
                (0..=segments)
                    .map(|i| {
                        let a = start_angle + sweep * i as f64 / segments as f64;
                        self.point(std::array::from_fn(|k| {
                            center[k] + radius * (x_axis[k] * a.cos() + y[k] * a.sin())
                        }))
                    })
                    .collect()
            }
            CadCurve::BezierSpans { ref spans } => approximate_bezier_spans(spans, tolerance)?
                .into_iter()
                .map(|p| self.point(p))
                .collect(),
            _ => {
                return Err(
                    "unsupported exact boundary curve; display tessellation cannot be machined"
                        .into(),
                )
            }
        };
        if reversed {
            p.reverse();
        }
        Ok(p)
    }
    /// Positive-weight Bezier curves lie inside their control hull. Use it for
    /// conservative height/planarity certificates, never just sampled vertices.
    fn curve_support_points(&self, edge: &CadEdge) -> Result<Vec<[f64; 3]>, String> {
        if let CadCurve::BezierSpans { spans } = &edge.curve {
            Ok(spans
                .iter()
                .flat_map(|s| &s.poles)
                .map(|p| self.point(*p))
                .collect())
        } else {
            self.curve_points(edge, false, MACHINING_CHORD_TOLERANCE)
        }
    }
    fn contour(&self, w: &CadLoop) -> Result<(Contour, f64), String> {
        if w.edge_uses.is_empty() {
            return Err("empty boundary loop".into());
        }
        let mut points: Vec<[f64; 3]> = vec![];
        for u in &w.edge_uses {
            let segment =
                self.curve_points(self.edge(u.edge_id)?, u.reversed, MACHINING_CHORD_TOLERANCE)?;
            if let Some(last) = points.last() {
                if distance(*last, segment[0]) > T {
                    return Err("boundary edges do not form an ordered closed loop".into());
                }
            }
            if !points.is_empty() {
                points.pop();
            }
            points.extend(segment);
        }
        if distance(points[0], *points.last().unwrap()) > T {
            return Err("open boundary cannot drive an operation".into());
        }
        points.pop();
        let z = points[0][2];
        if points.iter().any(|p| (p[2] - z).abs() > T)
            || w.edge_uses.iter().any(|u| {
                self.edge(u.edge_id)
                    .and_then(|e| self.curve_support_points(e))
                    .is_ok_and(|p| p.iter().any(|p| (p[2] - z).abs() > T))
            })
        {
            return Err("boundary loop is not coplanar in setup XY".into());
        }
        let c = Contour::new(points.into_iter().map(|p| Point::new(p[0], p[1])).collect());
        if !c.is_valid() || c.area() <= T * T {
            return Err("degenerate machining boundary".into());
        }
        Ok((c, z))
    }
    fn region(&self, f: &CadFace) -> Result<Polygon, String> {
        let mut outer = None;
        let mut holes = vec![];
        for id in &f.loop_ids {
            let w = self.wire(*id)?;
            let (c, _) = self.contour(w)?;
            if w.is_outer {
                if outer.replace(c).is_some() {
                    return Err("ambiguous multiple outer boundaries".into());
                }
            } else {
                holes.push(c);
            }
        }
        Polygon::with_holes(outer.ok_or("face has no outer boundary")?, holes)
            .map_err(|e| e.to_string())
    }
    fn extent(&self, f: &CadFace) -> Result<(f64, f64), String> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for id in &f.loop_ids {
            for u in &self.wire(*id)?.edge_uses {
                let e = self.edge(u.edge_id)?;
                for p in self.curve_support_points(e)? {
                    min = min.min(p[2]);
                    max = max.max(p[2]);
                }
                // Analytic extrema rather than display/curve sample extrema.
                if let CadCurve::CircleArc {
                    center,
                    normal,
                    x_axis,
                    radius,
                    start_angle,
                    end_angle,
                } = e.curve
                {
                    let x = self.vector(x_axis)[2];
                    let y = self.vector(cross(normal, x_axis))[2];
                    let z = self.point(center)[2];
                    for a in [y.atan2(x), y.atan2(x) + std::f64::consts::PI] {
                        for k in -2..=2 {
                            let a = a + k as f64 * TAU;
                            if a >= start_angle.min(end_angle) - T
                                && a <= start_angle.max(end_angle) + T
                            {
                                let v = z + radius * (x * a.cos() + y * a.sin());
                                min = min.min(v);
                                max = max.max(v);
                            }
                        }
                    }
                }
            }
        }
        if !min.is_finite() {
            return Err("face has no exact boundary extent".into());
        }
        Ok((min, max))
    }
    fn walls(&self, w: &CadLoop) -> Result<Vec<&CadFace>, String> {
        let mut ids = BTreeSet::new();
        for u in &w.edge_uses {
            let e = self.edge(u.edge_id)?;
            let other: BTreeSet<_> = e
                .face_ids
                .iter()
                .copied()
                .filter(|id| *id != w.face_id)
                .collect();
            if other.len() != 1 {
                return Err("boundary does not have unambiguous solid wall adjacency".into());
            }
            ids.extend(other);
        }
        ids.into_iter()
            .map(|id| {
                let f = self.face(id)?;
                if !self.vertical(f) {
                    return Err(
                        "sloped or unsupported boundary needs vertical constant-section walls"
                            .into(),
                    );
                }
                Ok(f)
            })
            .collect()
    }
    /// Certify the open approach above a locally qualified pocket/bore. Curved
    /// faces elsewhere on the body do not matter, but overlapping unknown
    /// surfaces must never be silently treated as empty space.
    fn roofs_over(&self, region: &Polygon, entry: f64) -> Result<(), String> {
        for f in self
            .asset
            .topology
            .faces
            .iter()
            .filter(|f| f.body_id == self.body.id)
        {
            let bounds = self.bounds(f.bounds);
            if bounds[1][2] <= entry + T {
                continue;
            }
            let overlap =
                cam_geo::intersection(std::slice::from_ref(region), &[self.footprint(bounds)?])
                    .map_err(|e| e.to_string())?;
            if overlap.is_empty() {
                continue;
            }
            if self.vertical(f) {
                continue;
            }
            if let Ok((z, n)) = self.plane(f) {
                if z <= entry + T || n[2] > 0.0 {
                    continue;
                }
                // A planar roof may have a hole giving access to this feature.
                // Unknown trim curves remain blocked by their conservative bounds.
                if let Ok(protected) = self.region(f) {
                    let overlap = cam_geo::intersection(std::slice::from_ref(region), &[protected])
                        .map_err(|e| e.to_string())?;
                    if overlap.is_empty() {
                        continue;
                    }
                }
            }
            return Err(format!(
                "overhang/undercut or unsupported surface on face {} blocks vertical access above the selected feature", f.id
            ));
        }
        Ok(())
    }

    /// Surface type alone is insufficient: a vertical wall with intermediate
    /// trims can hide a side opening or a roof. Only full-height walls bounded
    /// by horizontal rims and axial seams establish a constant-section void.
    fn certify_wall(&self, wall: &CadFace, bottom: f64, top: f64) -> Result<(), String> {
        if wall.loop_ids.len() != 1
            && !(matches!(wall.surface, CadSurface::Cylinder { .. }) && wall.loop_ids.len() == 2)
        {
            return Err("wall has intermediate openings or ambiguous trims".into());
        }
        for id in &wall.loop_ids {
            for usage in &self.wire(*id)?.edge_uses {
                let edge = self.edge(usage.edge_id)?;
                let points = self.curve_support_points(edge)?;
                let horizontal = points.iter().all(|p| (p[2] - bottom).abs() < T)
                    || points.iter().all(|p| (p[2] - top).abs() < T);
                let axial = matches!(
                    edge.curve,
                    CadCurve::Line { .. } | CadCurve::BezierSpans { .. }
                ) && points
                    .iter()
                    .all(|p| (p[0] - points[0][0]).abs() < T && (p[1] - points[0][1]).abs() < T);
                if !horizontal && !axial {
                    return Err(format!(
                        "wall has intermediate openings or nonconstant-section trims (face {}, edge {})", wall.id, edge.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// Open profiles follow a selected planar curve through the end transitions.
    /// An end may trim a vertical wall obliquely; it need not remain an axial
    /// seam. Keep the closed-feature certificate for pockets and closed loops.
    fn certify_open_profile_wall(
        &self,
        wall: &CadFace,
        bottom: f64,
        top: f64,
    ) -> Result<(), String> {
        if self.certify_wall(wall, bottom, top).is_ok() {
            return Ok(());
        }
        if wall.loop_ids.len() != 1 {
            return Err("profile wall has intermediate openings or ambiguous trims".into());
        }
        let wire = self.wire(wall.loop_ids[0])?;
        let mut uses = Vec::new();
        for usage in &wire.edge_uses {
            let edge = self.edge(usage.edge_id)?;
            let mut points = self.curve_support_points(edge)?;
            if usage.reversed {
                points.reverse();
            }
            let rim = if points.iter().all(|p| (p[2] - top).abs() < T) {
                Some(true)
            } else if points.iter().all(|p| (p[2] - bottom).abs() < T) {
                Some(false)
            } else {
                None
            };
            if rim.is_none()
                && !matches!(
                    edge.curve,
                    CadCurve::Line { .. } | CadCurve::BezierSpans { .. }
                )
            {
                return Err(format!(
                    "profile wall end Edge {} needs an exact end trim",
                    edge.id
                ));
            }
            uses.push((rim, points));
        }
        let first_rim = uses
            .iter()
            .position(|(rim, _)| rim.is_some())
            .ok_or("profile wall has no horizontal rims")?;
        uses.rotate_left(first_rim);
        if !uses.iter().any(|(rim, _)| *rim == Some(true))
            || !uses.iter().any(|(rim, _)| *rim == Some(false))
        {
            return Err("profile wall needs both an upper and a lower horizontal rim".into());
        }
        let mut caps = 0;
        let mut index = 0;
        while index < uses.len() {
            if uses[index].0.is_some() {
                index += 1;
                continue;
            }
            let before = uses[index - 1].0.unwrap();
            let mut heights = vec![if before { top } else { bottom }];
            while index < uses.len() && uses[index].0.is_none() {
                heights.extend(uses[index].1.iter().map(|p| p[2]));
                index += 1;
            }
            let after = uses[index % uses.len()].0.unwrap();
            if before == after {
                return Err("profile wall has an intermediate opening in a rim".into());
            }
            heights.push(if after { top } else { bottom });
            if (heights[1] - heights[0]).abs() > T
                || (heights[heights.len() - 2] - heights[heights.len() - 1]).abs() > T
                || heights.iter().any(|z| *z < bottom - T || *z > top + T)
            {
                return Err("profile wall end trims must connect its two horizontal rims within the cutting interval".into());
            }
            caps += 1;
        }
        if caps != 2 {
            return Err("profile wall needs exactly two end transitions".into());
        }
        Ok(())
    }

    fn certify_pocket_rim(
        &self,
        wall: &CadFace,
        floor_wire: &CadLoop,
        top: f64,
    ) -> Result<(), String> {
        let mut found = false;
        for id in &wall.loop_ids {
            for usage in &self.wire(*id)?.edge_uses {
                let edge = self.edge(usage.edge_id)?;
                let points = self.curve_support_points(edge)?;
                if !points.iter().all(|p| (p[2] - top).abs() < T) {
                    continue;
                }
                found = true;
                let others: Vec<_> = edge.face_ids.iter().filter(|id| **id != wall.id).collect();
                if others.len() != 1 {
                    return Err("pocket opening has ambiguous rim adjacency".into());
                }
                let entry = self.face(*others[0])?;
                let (z, normal) = self.plane(entry)?;
                if (z - top).abs() > T || normal[2] <= 0.0 {
                    return Err("pocket opening needs an upward planar entry face".into());
                }
                let rim = entry
                    .loop_ids
                    .iter()
                    .find_map(|id| {
                        self.wire(*id)
                            .ok()
                            .filter(|w| w.edge_uses.iter().any(|u| u.edge_id == edge.id))
                    })
                    .ok_or("pocket entry is missing its rim loop")?;
                if rim.is_outer == floor_wire.is_outer {
                    return Err(
                        "pocket entry covers the selected void or has ambiguous island adjacency"
                            .into(),
                    );
                }
            }
        }
        if !found {
            return Err("pocket wall has no certified opening rim".into());
        }
        Ok(())
    }

    /// A complete inward cylindrical wall and a disk floor certify the local
    /// void in a valid solid. Requiring its entry at/above the entire body also
    /// excludes overhangs, including surfaces outside the analytic subset.
    fn blind_bore_profile(
        &self,
        refs: &[EntityRef],
    ) -> Result<(Vec<Polygon>, Vec<ResolvedHole>, CuttingHeights), String> {
        if refs.len() != 1 || refs[0].kind != EntityKind::Loop {
            return Err(
                "Profile requires one closed coplanar loop; select one boundary per operation"
                    .into(),
            );
        }
        let wire = self.wire(refs[0].id)?;
        let floor = self.face(wire.face_id)?;
        if matches!(floor.surface, CadSurface::Cylinder { .. }) {
            return Err("This loop wraps a cylindrical wall and includes its seam. Select the circular loop on the planar recess floor for an inside Profile".into());
        }
        let (floor_z, normal) = self.plane(floor)?;
        if !wire.is_outer || floor.loop_ids.len() != 1 || normal[2] <= 0.0 {
            return Err("Inside Profile requires the circular floor boundary of one blind bore facing setup Z".into());
        }
        let mut cylinder = None;
        for usage in &wire.edge_uses {
            let edge = self.edge(usage.edge_id)?;
            if !matches!(edge.curve, CadCurve::CircleArc { .. }) {
                return Err(
                    "Inside CAD Profile currently requires a circular blind-bore floor".into(),
                );
            }
            for id in &edge.face_ids {
                if *id == floor.id {
                    continue;
                }
                let face = self.face(*id)?;
                if !matches!(face.surface, CadSurface::Cylinder { inward: true, .. }) {
                    return Err(
                        "The floor boundary must adjoin an inward cylindrical bore wall".into(),
                    );
                }
                cylinder.get_or_insert(face.id);
            }
        }
        let cylinder = cylinder.ok_or("No cylindrical wall adjoining the selected floor")?;
        let (_, entry_z) = self.extent(self.face(cylinder)?)?;
        for x in [self.body.bounds[0][0], self.body.bounds[1][0]] {
            for y in [self.body.bounds[0][1], self.body.bounds[1][1]] {
                for z in [self.body.bounds[0][2], self.body.bounds[1][2]] {
                    if self.point([x, y, z])[2] > entry_z + T {
                        return Err("Material above the bore entry prevents local profile certification; choose a setup with unobstructed access from above the body".into());
                    }
                }
            }
        }
        let mut bore = refs[0].clone();
        bore.kind = EntityKind::Face;
        bore.id = cylinder;
        let (_, holes, heights) = derive_holes(self, &[bore])?;
        if holes.len() != 1 || (heights.bottom - floor_z).abs() > T {
            return Err("The selected floor does not terminate one constant-diameter bore".into());
        }
        let (contour, _) = self.contour(wire)?;
        let hole = &holes[0];
        if contour
            .points()
            .iter()
            .any(|p| (p.distance(hole.center) - hole.diameter / 2.0).abs() > T)
        {
            return Err("The selected floor boundary does not match its certified bore".into());
        }
        Ok((
            vec![Polygon::new(contour).map_err(|e| e.to_string())?],
            vec![],
            heights,
        ))
    }
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

fn derive(
    state: &GeometryState,
    binding: &GeometryBinding,
    stock_top: f64,
) -> Result<(Vec<Polygon>, Vec<ResolvedHole>, CuttingHeights), String> {
    if let ExtractionRecipe::ProfileEntries { entries } = &binding.recipe {
        let (profiles, heights) = profile_entries::extract(state, entries, stock_top)?;
        let regions = profiles
            .into_iter()
            .filter(|p| p.closed)
            .map(|p| Polygon::new(p.chain).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((regions, vec![], heights));
    }
    if binding.recipe == ExtractionRecipe::ProfileContours {
        let (profiles, heights) = profile_contours::extract(state, &binding.selected)?;
        let regions = profiles
            .into_iter()
            .filter(|p| p.closed)
            .map(|p| Polygon::new(p.chain).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((regions, vec![], heights));
    }
    let view = View::new(state, &binding.selected)?;
    if matches!(
        binding.recipe,
        ExtractionRecipe::ProfileLoop { side: Side::Inside }
    ) {
        return view.blind_bore_profile(&binding.selected);
    }
    let refs = &binding.selected;
    if refs.len() == 1 && refs[0].kind == EntityKind::Silhouette {
        let bounds = view.bounds(view.body.bounds);
        let heights = match binding.recipe {
            ExtractionRecipe::ProfileLoop { side: Side::Outside } | ExtractionRecipe::ProfileSilhouette => CuttingHeights {
                top: bounds[1][2], bottom: bounds[0][2],
            },
            ExtractionRecipe::FaceRegion => CuttingHeights {
                top: stock_top, bottom: bounds[1][2],
            },
            _ => return Err("An XY silhouette supports outside Profile or Face; select a face for Pocket or Drill".into()),
        };
        heights.validate()?;
        let region = if binding.recipe == ExtractionRecipe::ProfileSilhouette {
            view.projected_outline(true)?
        } else {
            view.silhouette()?
        };
        return Ok((vec![region], vec![], heights));
    }
    match binding.recipe {
        ExtractionRecipe::PocketFloor => {
            if refs.len() != 1 || refs[0].kind != EntityKind::Face {
                return Err("Pocket requires one planar floor face".into());
            }
            let f = view.face(refs[0].id)?;
            let (floor, normal) = view.plane(f)?;
            if normal[2] < 0.0 {
                return Err("floor normal must face the machining direction".into());
            }
            let region = view.region(f)?;
            for face in view
                .asset
                .topology
                .faces
                .iter()
                .filter(|face| face.body_id == view.body.id)
            {
                if let Ok((z, n)) = view.plane(face) {
                    if z > floor + T && n[2] < 0.0 {
                        if cam_geo::intersection(
                            std::slice::from_ref(&region),
                            &[view.footprint(view.bounds(face.bounds))?],
                        )
                        .map_err(|e| e.to_string())?
                        .is_empty()
                        {
                            continue;
                        }
                        let protected = view.region(face).map_err(|_| format!(
                            "unsupported roof boundary on face {} prevents vertical access certification", face.id
                        ))?;
                        if !cam_geo::intersection(std::slice::from_ref(&region), &[protected])
                            .map_err(|e| e.to_string())?
                            .is_empty()
                        {
                            return Err("overhang/undercut blocks vertical access above the selected region".into());
                        }
                    }
                }
            }
            let mut top: Option<f64> = None;
            for id in &f.loop_ids {
                for wall in view.walls(view.wire(*id)?)? {
                    let (lo, hi) = view.extent(wall)?;
                    if (lo - floor).abs() > T || hi <= floor + T {
                        return Err("floor boundary is open, has a bore below it, or is not a rising pocket wall".into());
                    }
                    view.certify_wall(wall, lo, hi)?;
                    view.certify_pocket_rim(wall, view.wire(*id)?, hi)?;
                    if let Some(z) = top {
                        if (z - hi).abs() > T {
                            return Err("pocket walls have ambiguous or different openings".into());
                        }
                    } else {
                        top = Some(hi);
                    }
                }
            }
            let heights = CuttingHeights {
                top: top.ok_or("no pocket walls")?,
                bottom: floor,
            };
            view.roofs_over(&region, heights.top)?;
            heights.validate()?;
            Ok((vec![region], vec![], heights))
        }
        ExtractionRecipe::FaceRegion => {
            if refs.len() != 1 || refs[0].kind != EntityKind::Face {
                return Err("Face requires one planar target face".into());
            }
            let f = view.face(refs[0].id)?;
            let (z, n) = view.plane(f)?;
            if n[2] < 0.0 {
                return Err("facing target must face setup Z".into());
            }
            // Bounds come from the native B-rep, never the display mesh or an
            // approximation of a spline. Transform all eight corners to retain a
            // conservative enclosure in the active setup, even for curved bodies.
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            let bounds = std::iter::once(view.body.bounds).chain(
                view.asset
                    .topology
                    .faces
                    .iter()
                    .filter(|face| face.body_id == view.body.id)
                    .map(|face| face.bounds),
            );
            for bounds in bounds {
                for x in [bounds[0][0], bounds[1][0]] {
                    for y in [bounds[0][1], bounds[1][1]] {
                        for z in [bounds[0][2], bounds[1][2]] {
                            let point = view.point([x, y, z]);
                            for axis in 0..3 {
                                min[axis] = min[axis].min(point[axis]);
                                max[axis] = max[axis].max(point[axis]);
                            }
                        }
                    }
                }
            }
            for vertex in view
                .asset
                .topology
                .vertices
                .iter()
                .filter(|v| v.body_id == view.body.id)
            {
                let point = view.point(vertex.point);
                for axis in 0..3 {
                    min[axis] = min[axis].min(point[axis]);
                    max[axis] = max[axis].max(point[axis]);
                }
            }
            if max[2] > z + T {
                return Err(
                    "adjacent bosses/protected material above the target prevent whole-body facing"
                        .into(),
                );
            }
            let region = Polygon::new(Contour::new(vec![
                Point::new(min[0], min[1]),
                Point::new(max[0], min[1]),
                Point::new(max[0], max[1]),
                Point::new(min[0], max[1]),
            ]))
            .map_err(|error| error.to_string())?;
            if stock_top <= z {
                return Err("stock top must be above the facing target; increase the positive Z stock allowance".into());
            }
            let heights = CuttingHeights {
                top: stock_top,
                bottom: z,
            };
            heights.validate()?;
            Ok((vec![region], vec![], heights))
        }
        ExtractionRecipe::ProfileLoop { side } => {
            if side != Side::Outside {
                return Err("initial profile certification supports only the outside of a whole-body exterior loop".into());
            }
            if refs.len() != 1 || refs[0].kind != EntityKind::Loop {
                return Err("Profile requires one closed coplanar loop".into());
            }
            let w = view.wire(refs[0].id)?;
            if !w.is_outer {
                return Err("outside profile requires the body's exterior loop".into());
            }
            let (c, z) = view.contour(w)?;
            let region = Polygon::new(c).map_err(|e| e.to_string())?;
            let mut bottom: Option<f64> = None;
            let walls = view.walls(w)?;
            for wall in &walls {
                let (lo, hi) = view.extent(wall)?;
                if (hi - z).abs() > T || lo >= z - T {
                    return Err("profile loop must be the top of a vertical exterior wall".into());
                }
                view.certify_wall(wall, lo, hi)?;
                if let Some(v) = bottom {
                    if (lo - v).abs() > T {
                        return Err("profile walls have different depths".into());
                    }
                } else {
                    bottom = Some(lo);
                }
            }
            // Faces enclosed by the silhouette cannot be hit by an outside
            // profile, regardless of their surface/curve type. Remaining planar
            // faces can be checked by their exact boundary; unknown exterior
            // material stays unqualified.
            for face in view
                .asset
                .topology
                .faces
                .iter()
                .filter(|f| f.body_id == view.body.id)
            {
                if face.id == w.face_id || walls.iter().any(|wall| wall.id == face.id) {
                    continue;
                }
                let bounds = view.bounds(face.bounds);
                let footprint = Polygon::new(Contour::new(vec![
                    Point::new(bounds[0][0], bounds[0][1]),
                    Point::new(bounds[1][0], bounds[0][1]),
                    Point::new(bounds[1][0], bounds[1][1]),
                    Point::new(bounds[0][0], bounds[1][1]),
                ]));
                if let Ok(footprint) = footprint {
                    if cam_geo::difference(&[footprint], std::slice::from_ref(&region))
                        .map_err(|e| e.to_string())?
                        .is_empty()
                    {
                        continue;
                    }
                }
                if view.plane(face).is_ok() {
                    if let Ok(protected) = view.region(face) {
                        if cam_geo::difference(&[protected], std::slice::from_ref(&region))
                            .map_err(|e| e.to_string())?
                            .is_empty()
                        {
                            continue;
                        }
                    }
                }
                return Err(format!("material on face {} outside the selected silhouette prevents safe side certification", face.id));
            }
            let heights = CuttingHeights {
                top: z,
                bottom: bottom.ok_or("no profile walls")?,
            };
            heights.validate()?;
            Ok((vec![region], vec![], heights))
        }
        ExtractionRecipe::DrillHoles => derive_holes(&view, refs),
        ExtractionRecipe::ProfileContours | ExtractionRecipe::ProfileEntries { .. } => {
            unreachable!()
        }
        ExtractionRecipe::ProfileSilhouette => Err("Select one body for its silhouette".into()),
    }
}
fn derive_holes(
    view: &View<'_>,
    refs: &[EntityRef],
) -> Result<(Vec<Polygon>, Vec<ResolvedHole>, CuttingHeights), String> {
    let mut holes: Vec<ResolvedHole> = vec![];
    let mut heights: Option<CuttingHeights> = None;
    for r in refs {
        if r.kind != EntityKind::Face {
            return Err(
                "Drill requires cylindrical bore faces; circular edges do not establish a hole"
                    .into(),
            );
        }
        let f = view.face(r.id)?;
        let CadSurface::Cylinder {
            origin,
            axis,
            radius,
            inward,
        } = f.surface
        else {
            return Err("Drill requires cylindrical bore faces".into());
        };
        let center = view.point(origin);
        let a = view.vector(axis);
        if !inward || a[0].abs() > T || a[1].abs() > T {
            return Err("selected cylinder is a boss or its bore axis is not setup Z".into());
        }
        let (lo, hi) = view.extent(f)?;
        // Merge split cylindrical seam faces into one analytic bore before validating
        // its complete top/bottom circular rims and termination adjacency.
        let group: Vec<_> = view
            .asset
            .topology
            .faces
            .iter()
            .filter(|candidate| candidate.body_id == view.body.id)
            .filter(|candidate| match candidate.surface {
                CadSurface::Cylinder {
                    origin: o,
                    axis: ax,
                    radius: rad,
                    inward: true,
                } => {
                    let c = view.point(o);
                    let ax = view.vector(ax);
                    (rad - radius).abs() < T
                        && (c[0] - center[0]).abs() < T
                        && (c[1] - center[1]).abs() < T
                        && ax[0].abs() < T
                        && ax[1].abs() < T
                        && view
                            .extent(candidate)
                            .is_ok_and(|(l, h)| (l - lo).abs() < T && (h - hi).abs() < T)
                }
                _ => false,
            })
            .collect();
        let mut rim_edges = BTreeSet::new();
        let mut top_sweep = 0.0;
        let mut bottom_sweep = 0.0;
        for face in group {
            for id in &face.loop_ids {
                for u in &view.wire(*id)?.edge_uses {
                    let edge = view.edge(u.edge_id)?;
                    if !rim_edges.insert(edge.id) {
                        continue;
                    }
                    match edge.curve {
                        CadCurve::CircleArc {
                            center: c,
                            radius: rad,
                            start_angle,
                            end_angle,
                            ..
                        } => {
                            let c = view.point(c);
                            if (rad - radius).abs() > T
                                || (c[0] - center[0]).abs() > T
                                || (c[1] - center[1]).abs() > T
                            {
                                return Err(
                                    "bore radius or centre varies over the cutting interval".into(),
                                );
                            }
                            if (c[2] - hi).abs() < T {
                                top_sweep += (end_angle - start_angle).abs();
                            } else if (c[2] - lo).abs() < T {
                                bottom_sweep += (end_angle - start_angle).abs();
                            } else {
                                return Err(
                                    "bore has intermediate rims or ambiguous termination".into()
                                );
                            }
                            let mut terminations = 0;
                            for fid in &edge.face_ids {
                                let adjacent = view.face(*fid)?;
                                if matches!(adjacent.surface, CadSurface::Cylinder { .. }) {
                                    continue;
                                }
                                let (z,n)=view.plane(adjacent).map_err(|_|"blind drill tips, blends or ambiguous hole terminations need explicit qualification".to_string())?;
                                if (z - c[2]).abs() > T || ((c[2] - hi).abs() < T && n[2] < 0.0) {
                                    return Err(
                                        "hole opening is covered or its termination is ambiguous"
                                            .into(),
                                    );
                                }
                                if (c[2] - hi).abs() < T {
                                    let open_rim = adjacent.loop_ids.iter().any(|id| {
                                        view.wire(*id).is_ok_and(|w| {
                                            !w.is_outer
                                                && w.edge_uses.iter().any(|u| u.edge_id == edge.id)
                                        })
                                    });
                                    if !open_rim {
                                        return Err("bore entry is capped or lacks a certified inner opening rim".into());
                                    }
                                }
                                terminations += 1;
                            }
                            if terminations != 1 {
                                return Err(
                                    "bore rims require one certified planar entry/termination face"
                                        .into(),
                                );
                            }
                        }
                        CadCurve::Line { start, end } => {
                            let start = view.point(start);
                            let end = view.point(end);
                            if (start[0] - end[0]).abs() > T || (start[1] - end[1]).abs() > T {
                                return Err("cylindrical trim is not an axial seam".into());
                            }
                        }
                        _ => return Err("unsupported cylindrical bore trim".into()),
                    }
                }
            }
        }
        if (top_sweep - TAU).abs() > T || (bottom_sweep - TAU).abs() > T {
            return Err(
                "split cylindrical faces do not establish a complete constant-diameter bore".into(),
            );
        }
        let h = CuttingHeights {
            top: hi,
            bottom: lo,
        };
        h.validate()?;
        let circle = Contour::new(
            (0..128)
                .map(|i| {
                    let angle = TAU * i as f64 / 128.;
                    Point::new(
                        center[0] + radius / (std::f64::consts::PI / 128.).cos() * angle.cos(),
                        center[1] + radius / (std::f64::consts::PI / 128.).cos() * angle.sin(),
                    )
                })
                .collect(),
        );
        view.roofs_over(&Polygon::new(circle).map_err(|e| e.to_string())?, hi)?;
        if let Some(existing) = heights {
            if (h.top - existing.top).abs() > T
                || (h.bottom - existing.bottom).abs() > T
                || holes
                    .first()
                    .is_some_and(|p| (p.diameter - 2.0 * radius).abs() > T)
            {
                return Err(
                    "holes with different entry, depth or diameter require separate operations"
                        .into(),
                );
            }
        } else {
            heights = Some(h);
        }
        let p = Point::new(center[0], center[1]);
        if !holes.iter().any(|h| h.center.distance(p) < T) {
            holes.push(ResolvedHole {
                center: p,
                diameter: 2.0 * radius,
            });
        }
    }
    Ok((vec![], holes, heights.ok_or("select cylindrical holes")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        t: TopologySnapshot,
        next: u64,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                t: TopologySnapshot {
                    bodies: vec![CadBody {
                        id: 1,
                        name: "Analytic plate".into(),
                        valid_solid: true,
                        bounds: [[0., 0., -5.], [20., 20., 0.]],
                    }],
                    ..Default::default()
                },
                next: 2,
            }
        }
        fn id(&mut self) -> u64 {
            let id = self.next;
            self.next += 1;
            id
        }
        fn vertex(&mut self, p: [f64; 3]) -> u64 {
            if let Some(v) = self.t.vertices.iter().find(|v| v.point == p) {
                return v.id;
            }
            let id = self.id();
            self.t.vertices.push(CadVertex {
                id,
                body_id: 1,
                point: p,
            });
            id
        }
        fn face(&mut self, rings: Vec<Vec<[f64; 3]>>, normal: [f64; 3]) -> u64 {
            let id = self.id();
            let mut loop_ids = vec![];
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for (i, ring) in rings.iter().enumerate() {
                let wid = self.id();
                loop_ids.push(wid);
                let mut edge_uses = vec![];
                for j in 0..ring.len() {
                    let a = ring[j];
                    let b = ring[(j + 1) % ring.len()];
                    for k in 0..3 {
                        bounds[0][k] = bounds[0][k].min(a[k]);
                        bounds[1][k] = bounds[1][k].max(a[k]);
                    }
                    let va = self.vertex(a);
                    let vb = self.vertex(b);
                    let ei = self
                        .t
                        .edges
                        .iter()
                        .position(|e| e.vertices == [va, vb] || e.vertices == [vb, va]);
                    let (edge_id, reversed) = if let Some(ei) = ei {
                        let e = &mut self.t.edges[ei];
                        e.face_ids.push(id);
                        (e.id, e.vertices != [va, vb])
                    } else {
                        let edge_id = self.id();
                        self.t.edges.push(CadEdge {
                            id: edge_id,
                            body_id: 1,
                            vertices: [va, vb],
                            face_ids: vec![id],
                            curve: CadCurve::Line { start: a, end: b },
                        });
                        (edge_id, false)
                    };
                    edge_uses.push(EdgeUse { edge_id, reversed });
                }
                self.t.loops.push(CadLoop {
                    id: wid,
                    body_id: 1,
                    face_id: id,
                    edge_uses,
                    is_outer: i == 0,
                });
            }
            self.t.faces.push(CadFace {
                id,
                body_id: 1,
                surface: CadSurface::Plane {
                    origin: rings[0][0],
                    normal,
                },
                loop_ids,
                bounds,
            });
            id
        }
        fn walls(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, lo: f64, hi: f64) {
            self.face(
                vec![vec![[x0, y0, lo], [x1, y0, lo], [x1, y0, hi], [x0, y0, hi]]],
                [0., -1., 0.],
            );
            self.face(
                vec![vec![[x1, y0, lo], [x1, y1, lo], [x1, y1, hi], [x1, y0, hi]]],
                [1., 0., 0.],
            );
            self.face(
                vec![vec![[x1, y1, lo], [x0, y1, lo], [x0, y1, hi], [x1, y1, hi]]],
                [0., 1., 0.],
            );
            self.face(
                vec![vec![[x0, y1, lo], [x0, y0, lo], [x0, y0, hi], [x0, y1, hi]]],
                [-1., 0., 0.],
            );
        }
        fn state(self) -> GeometryState {
            GeometryState {
                assets: vec![GeometryAsset {
                    import_options: None,
                    import_diagnostics: vec![],
                    source_fingerprint: String::new(),
                    id: 1,
                    revision: 1,
                    name: "analytic".into(),
                    source_format: CadFormat::Step,
                    source_data: vec![].into(),
                    source_units: SourceUnits::Millimetres,
                    backend: "analytic test fixture (not imported STEP)".into(),
                    topology: self.t.into(),
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
            }
        }
    }
    fn square(x0: f64, x1: f64, y0: f64, y1: f64, z: f64) -> Vec<[f64; 3]> {
        vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]
    }
    fn reference(id: u64, kind: EntityKind) -> EntityRef {
        EntityRef {
            asset_id: 1,
            revision: 1,
            instance_id: 1,
            body_id: 1,
            kind,
            id,
        }
    }
    fn pocket() -> (GeometryState, EntityRef) {
        let mut f = Fixture::new();
        let floor = f.face(
            vec![square(2., 18., 2., 18., -3.), square(8., 12., 8., 12., -3.)],
            [0., 0., 1.],
        );
        f.walls(2., 18., 2., 18., -3., 0.);
        f.walls(8., 12., 8., 12., -3., 0.);
        f.face(
            vec![square(0., 20., 0., 20., 0.), square(2., 18., 2., 18., 0.)],
            [0., 0., 1.],
        );
        f.face(vec![square(8., 12., 8., 12., 0.)], [0., 0., 1.]);
        f.walls(0., 20., 0., 20., -5., 0.);
        f.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
        (f.state(), reference(floor, EntityKind::Face))
    }
    #[test]
    fn pocket_island_boundary_and_translated_heights_are_exact() {
        let (mut state, r) = pocket();
        state.validate().unwrap();
        state.instances[0].placement.translation = [100., -50., 40.];
        let b = resolve_selection(&state, &[r], ExtractionRecipe::PocketFloor, 40.).unwrap();
        let result = resolve_binding(&state, &b).unwrap();
        assert_eq!(
            result.heights,
            CuttingHeights {
                top: 40.,
                bottom: 37.
            }
        );
        assert_eq!(result.regions[0].holes().len(), 1);
        assert_eq!(result.regions[0].area(), 240.);
        assert_eq!(
            result.regions[0].outer().points()[0],
            Point::new(102., -48.)
        );
    }
    #[test]
    fn stale_revisions_and_deeper_overrides_never_use_cached_contours() {
        let (mut state, r) = pocket();
        let mut b = resolve_selection(&state, &[r], ExtractionRecipe::PocketFloor, 0.).unwrap();
        b.overrides.bottom = Some(-4.);
        assert!(resolve_binding(&state, &b)
            .unwrap_err()
            .contains("certified"));
        b.overrides.bottom = None;
        state.assets[0].revision += 1;
        assert!(resolve_binding(&state, &b)
            .unwrap_err()
            .contains("Needs reselection"));
    }
    #[test]
    fn display_mesh_changes_do_not_change_machining_or_fingerprint() {
        let (mut state, r) = pocket();
        let b = resolve_selection(&state, &[r], ExtractionRecipe::PocketFloor, 0.).unwrap();
        let a = resolve_binding(&state, &b).unwrap();
        let t = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        t.mesh.vertices = vec![[0., 0., -3.], [20., 0., -3.], [0., 20., -3.]];
        t.mesh.triangles = vec![[0, 1, 2]];
        t.mesh.triangle_faces = vec![b.selected[0].id];
        assert_eq!(resolve_binding(&state, &b).unwrap(), a);
    }
    #[test]
    fn slope_and_overhang_are_rejected() {
        let (mut state, r) = pocket();
        let id = r.id;
        let t = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        let wall = t
            .faces
            .iter_mut()
            .find(|f| {
                f.id != id && matches!(f.surface,CadSurface::Plane{normal,..}if normal[2]==0.)
            })
            .unwrap();
        wall.surface = CadSurface::Plane {
            origin: [2., 2., -3.],
            normal: [0., 0.6, 0.8],
        };
        assert!(
            resolve_selection(&state, &[r], ExtractionRecipe::PocketFloor, 0.)
                .unwrap_err()
                .contains("sloped")
        );
        let mut f = Fixture::new();
        let floor = f.face(vec![square(2., 18., 2., 18., -3.)], [0., 0., 1.]);
        f.walls(2., 18., 2., 18., -3., 0.);
        f.face(vec![square(2., 6., 2., 18., -1.)], [0., 0., -1.]);
        assert!(resolve_selection(
            &f.state(),
            &[reference(floor, EntityKind::Face)],
            ExtractionRecipe::PocketFloor,
            0.
        )
        .unwrap_err()
        .contains("overhang"));
    }
    fn plate_with_hole() -> (GeometryState, EntityRef) {
        let mut f = Fixture::new();
        let rings = |z| vec![square(0., 20., 0., 20., z), square(8., 12., 8., 12., z)];
        let top = f.face(rings(0.), [0., 0., 1.]);
        f.walls(0., 20., 0., 20., -5., 0.);
        f.walls(8., 12., 8., 12., -5., 0.);
        f.face(rings(-5.), [0., 0., -1.]);
        (f.state(), reference(top, EntityKind::Face))
    }

    #[test]
    fn face_contours_extract_outer_and_inner_walls_with_independent_sides() {
        let (mut state, face) = plate_with_hole();
        state.instances[0].placement.translation = [100., -50., 40.];
        let b = resolve_selection(&state, &[face], ExtractionRecipe::ProfileContours, 40.).unwrap();
        let input = resolve_binding(&state, &b).unwrap();
        assert_eq!(input.profiles.len(), 2);
        assert_eq!(input.profiles[0].side, Side::Outside);
        assert_eq!(input.profiles[1].side, Side::Inside);
        assert!(input.profiles.iter().all(|p| p.closed));
        assert_eq!(input.profiles[0].chain.area(), 400.);
        assert_eq!(input.profiles[1].chain.area(), 16.);
        assert_eq!(
            input.heights,
            CuttingHeights {
                top: 40.,
                bottom: 35.
            }
        );
        let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        topology.mesh = CadMesh::default();
        assert_eq!(resolve_binding(&state, &b).unwrap(), input);
        state.assets[0].revision += 1;
        assert!(resolve_binding(&state, &b).is_err());
    }

    #[test]
    fn connected_edges_preserve_open_endpoints_and_detect_complete_loops_and_gaps() {
        let (state, face) = plate_with_hole();
        let asset = &state.assets[0];
        let wire = asset
            .topology
            .loops
            .iter()
            .find(|w| w.face_id == face.id && w.is_outer)
            .unwrap();
        let edges: Vec<_> = wire
            .edge_uses
            .iter()
            .map(|u| reference(u.edge_id, EntityKind::Edge))
            .collect();
        let b =
            resolve_selection(&state, &edges[..2], ExtractionRecipe::ProfileContours, 0.).unwrap();
        let input = resolve_binding(&state, &b).unwrap();
        assert_eq!(input.profiles.len(), 1);
        assert!(!input.profiles[0].closed);
        assert_eq!(input.profiles[0].chain.len(), 3);
        assert_eq!(
            input.profiles[0].chain.points().first(),
            Some(&Point::new(0., 0.))
        );
        assert_eq!(
            input.profiles[0].chain.points().last(),
            Some(&Point::new(20., 20.))
        );
        let shuffled = vec![
            edges[2].clone(),
            edges[0].clone(),
            edges[3].clone(),
            edges[1].clone(),
        ];
        let b =
            resolve_selection(&state, &shuffled, ExtractionRecipe::ProfileContours, 0.).unwrap();
        assert!(resolve_binding(&state, &b).unwrap().profiles[0].closed);
        assert!(resolve_selection(
            &state,
            &[edges[0].clone(), edges[2].clone()],
            ExtractionRecipe::ProfileContours,
            0.
        )
        .is_err());
    }

    #[test]
    fn open_chain_uses_local_wall_normals_despite_unsupported_face_trims() {
        let (mut state, face) = plate_with_hole();
        let wire = state.assets[0]
            .topology
            .loops
            .iter()
            .find(|w| w.face_id == face.id && w.is_outer)
            .unwrap()
            .clone();
        let edges: Vec<_> = wire
            .edge_uses
            .iter()
            .map(|u| reference(u.edge_id, EntityKind::Edge))
            .collect();
        let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        topology
            .edges
            .iter_mut()
            .find(|e| e.id == edges[2].id)
            .unwrap()
            .curve = CadCurve::Unsupported {
            description: "unselected spline on the same face".into(),
        };
        for reversed in [false, true] {
            let selected = if reversed {
                vec![edges[1].clone(), edges[0].clone()]
            } else {
                edges[..2].to_vec()
            };
            let binding =
                resolve_selection(&state, &selected, ExtractionRecipe::ProfileContours, 0.)
                    .unwrap();
            let input = resolve_binding(&state, &binding).unwrap();
            let profile = &input.profiles[0];
            assert!(!profile.closed);
            assert_eq!(profile.chain.len(), 3);
            assert_eq!(
                profile.side,
                if reversed {
                    Side::Outside
                } else {
                    Side::Inside
                }
            );
            assert_eq!(
                input.heights,
                CuttingHeights {
                    top: 0.,
                    bottom: -5.
                }
            );
            // Transforming both the edge and its wall must preserve the free side.
            let mut rotated = state.clone();
            rotated.instances[0].placement = RigidTransform {
                rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
                translation: [100., -50., 40.],
            };
            let rotated_binding =
                resolve_selection(&rotated, &selected, ExtractionRecipe::ProfileContours, 40.)
                    .unwrap();
            assert_eq!(
                resolve_binding(&rotated, &rotated_binding)
                    .unwrap()
                    .profiles[0]
                    .side,
                profile.side
            );
        }
        // Unsupported selected geometry still cannot be machined.
        assert!(
            resolve_selection(&state, &edges[2..3], ExtractionRecipe::ProfileContours, 0.)
                .unwrap_err()
                .contains("unsupported exact boundary curve")
        );
        assert!(resolve_selection(
            &state,
            &[reference(wire.id, EntityKind::Loop)],
            ExtractionRecipe::ProfileLoop {
                side: Side::Outside
            },
            0.
        )
        .is_err());
    }

    #[test]
    fn spline_control_hulls_reject_nonplanar_curves_hidden_by_chord_tolerance() {
        let (mut state, face) = plate_with_hole();
        let edge_id = state.assets[0]
            .topology
            .loops
            .iter()
            .find(|w| w.face_id == face.id && w.is_outer)
            .unwrap()
            .edge_uses[0]
            .edge_id;
        let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        let edge = topology.edges.iter_mut().find(|e| e.id == edge_id).unwrap();
        let CadCurve::Line { start, end } = edge.curve else {
            panic!("line")
        };
        let middle = std::array::from_fn(|i| {
            if i == 2 {
                0.005
            } else {
                (start[i] + end[i]) * 0.5
            }
        });
        let spans = vec![CadBezierSpan {
            poles: vec![start, middle, end],
            weights: vec![1.; 3],
        }];
        assert_eq!(
            approximate_bezier_spans(&spans, MACHINING_CHORD_TOLERANCE)
                .unwrap()
                .len(),
            2
        );
        edge.curve = CadCurve::BezierSpans { spans };
        let error = resolve_selection(
            &state,
            &[reference(edge_id, EntityKind::Edge)],
            ExtractionRecipe::ProfileContours,
            0.,
        )
        .unwrap_err();
        assert!(error.contains("one accessible plane"), "{error}");
    }

    #[test]
    fn explicit_open_chain_uses_operation_depth_without_a_vertical_wall() {
        let (mut state, face) = plate_with_hole();
        let wire = state.assets[0]
            .topology
            .loops
            .iter()
            .find(|w| w.face_id == face.id && w.is_outer)
            .unwrap()
            .clone();
        let edges: Vec<_> = wire
            .edge_uses
            .iter()
            .map(|u| reference(u.edge_id, EntityKind::Edge))
            .collect();
        let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        let wall_id = topology
            .edges
            .iter()
            .find(|e| e.id == edges[0].id)
            .unwrap()
            .face_ids
            .iter()
            .copied()
            .find(|id| *id != face.id)
            .unwrap();
        topology
            .faces
            .iter_mut()
            .find(|f| f.id == wall_id)
            .unwrap()
            .surface = CadSurface::Unsupported {
            description: "spline blend".into(),
        };
        let mut entry =
            ProfileGeometryEntry::new(ProfileGeometryKind::Chain, "Open blend rim".into());
        entry.source = ProfileGeometrySource::Cad(edges[..2].to_vec());
        entry.chain_mode = ProfileChainMode::Open;
        let (contours, heights) = profile_entry_contours(&state, &entry).unwrap();
        assert!(
            heights.is_none(),
            "a planar path does not certify wall depth"
        );
        assert!(!contours[0].closed);
        assert_eq!(contours[0].side, Side::Inside);
        let mut backwards = entry.clone();
        backwards.source = ProfileGeometrySource::Cad(vec![edges[1].clone(), edges[0].clone()]);
        assert_eq!(
            profile_entry_contours(&state, &backwards).unwrap().0[0].side,
            Side::Outside
        );
        let mut binding = resolve_selection(
            &state,
            &edges[..2],
            ExtractionRecipe::ProfileEntries {
                entries: vec![entry.clone()],
            },
            1.,
        )
        .unwrap();
        binding.overrides = GeometryOverrides {
            top: Some(1.),
            bottom: Some(-12.),
        };
        let resolved = resolve_binding(&state, &binding).unwrap();
        assert_eq!(
            resolved.heights,
            CuttingHeights {
                top: 1.,
                bottom: -12.
            }
        );
        std::sync::Arc::make_mut(&mut state.assets[0].topology).mesh = CadMesh::default();
        assert_eq!(resolve_binding(&state, &binding).unwrap(), resolved);
        for mode in [ProfileChainMode::Auto, ProfileChainMode::Closed] {
            entry.chain_mode = mode;
            assert!(profile_entry_contours(&state, &entry)
                .unwrap_err()
                .contains("no adjoining vertical wall"));
        }
        entry.chain_mode = ProfileChainMode::Open;
        entry.source = ProfileGeometrySource::Cad(vec![edges[0].clone(), edges[2].clone()]);
        assert!(profile_entry_contours(&state, &entry)
            .unwrap_err()
            .contains("connect without gaps"));
        state.instances[0].placement.rotation = [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]];
        entry.source = ProfileGeometrySource::Cad(vec![edges[1].clone()]);
        assert!(profile_entry_contours(&state, &entry)
            .unwrap_err()
            .contains("one accessible plane"));
    }

    #[test]
    fn trimmed_wall_ends_are_usable_only_for_open_profiles() {
        let mut fixture = Fixture::new();
        let wall_id = fixture.face(
            vec![vec![
                [0., 0., 0.],
                [20., 0., 0.],
                [18., 0., -5.],
                [2., 0., -5.],
            ]],
            [0., -1., 0.],
        );
        let state = fixture.state();
        let selected = [reference(wall_id, EntityKind::Face)];
        let view = View::new(&state, &selected).unwrap();
        let wall = view.face(wall_id).unwrap();
        assert!(view.certify_open_profile_wall(wall, -5., 0.).is_ok());
        assert!(
            view.certify_wall(wall, -5., 0.).is_err(),
            "closed pockets retain their full-height trim guard"
        );
        let mut with_opening = state.clone();
        let topology = std::sync::Arc::make_mut(&mut with_opening.assets[0].topology);
        let wall = topology.faces.iter_mut().find(|f| f.id == wall_id).unwrap();
        wall.loop_ids.push(wall.loop_ids[0]);
        let view = View::new(&with_opening, &selected).unwrap();
        assert!(view
            .certify_open_profile_wall(view.face(wall_id).unwrap(), -5., 0.)
            .is_err());
    }

    #[test]
    fn coplanar_faces_stay_in_the_selected_body_and_require_matching_cutting_intervals() {
        let (state, _) = pocket();
        let top = state.assets[0].topology.faces.iter().find(|f| matches!(f.surface, CadSurface::Plane { origin, normal } if origin[2] == 0. && normal[2] > 0.)).unwrap();
        let refs = coplanar_profile_faces(&state, &reference(top.id, EntityKind::Face)).unwrap();
        assert_eq!(refs.len(), 2);
        assert!(refs
            .iter()
            .all(|r| r.kind == EntityKind::Face && r.body_id == 1));
        assert!(
            resolve_selection(&state, &refs, ExtractionRecipe::ProfileContours, 0.).is_err(),
            "different wall depths cannot silently share one profile depth"
        );
    }

    #[test]
    fn rectangle_facing_and_exterior_profile_support_only_certified_sides() {
        let mut f = Fixture::new();
        let top = f.face(vec![square(0., 20., 0., 20., 0.)], [0., 0., 1.]);
        f.walls(0., 20., 0., 20., -5., 0.);
        f.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
        let wire = f.t.faces.iter().find(|f| f.id == top).unwrap().loop_ids[0];
        let state = f.state();
        let b = resolve_selection(
            &state,
            &[reference(top, EntityKind::Face)],
            ExtractionRecipe::FaceRegion,
            2.,
        )
        .unwrap();
        assert_eq!(
            resolve_binding(&state, &b).unwrap().heights,
            CuttingHeights {
                top: 2.,
                bottom: 0.
            }
        );
        let r = reference(wire, EntityKind::Loop);
        let b = resolve_selection(
            &state,
            std::slice::from_ref(&r),
            ExtractionRecipe::ProfileLoop {
                side: Side::Outside,
            },
            0.,
        )
        .unwrap();
        assert_eq!(resolve_binding(&state, &b).unwrap().heights.bottom, -5.);
        assert!(resolve_selection(
            &state,
            &[r],
            ExtractionRecipe::ProfileLoop { side: Side::Inside },
            0.
        )
        .is_err());
    }
    #[test]
    fn exterior_profile_allows_enclosed_curves_but_rejects_unknown_material_outside() {
        let mut fixture = Fixture::new();
        let top = fixture.face(vec![square(0., 20., 0., 20., 0.)], [0., 0., 1.]);
        fixture.walls(0., 20., 0., 20., -5., 0.);
        fixture.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
        let curved = fixture.face(vec![square(5., 7., 5., 7., -1.)], [0., 0., 1.]);
        fixture
            .t
            .faces
            .iter_mut()
            .find(|f| f.id == curved)
            .unwrap()
            .surface = CadSurface::Unsupported {
            description: "enclosed blend".into(),
        };
        let edge = fixture
            .t
            .loops
            .iter()
            .find(|w| w.face_id == curved)
            .unwrap()
            .edge_uses[0]
            .edge_id;
        fixture
            .t
            .edges
            .iter_mut()
            .find(|e| e.id == edge)
            .unwrap()
            .curve = CadCurve::Unsupported {
            description: "enclosed spline".into(),
        };
        let wire = fixture
            .t
            .faces
            .iter()
            .find(|f| f.id == top)
            .unwrap()
            .loop_ids[0];
        let mut state = fixture.state();
        let selected = [reference(wire, EntityKind::Loop)];
        let recipe = ExtractionRecipe::ProfileLoop {
            side: Side::Outside,
        };
        let binding = resolve_selection(&state, &selected, recipe.clone(), 0.).unwrap();
        assert_eq!(
            resolve_binding(&state, &binding).unwrap().heights.bottom,
            -5.
        );
        // Check the enclosure in setup coordinates rather than asset coordinates.
        state.instances[0].placement.translation = [100., -50., 40.];
        let binding = resolve_selection(&state, &selected, recipe.clone(), 40.).unwrap();
        assert_eq!(
            resolve_binding(&state, &binding).unwrap().heights.bottom,
            35.
        );
        std::sync::Arc::make_mut(&mut state.assets[0].topology)
            .faces
            .iter_mut()
            .find(|f| f.id == curved)
            .unwrap()
            .bounds[1][0] = 21.;
        assert!(resolve_selection(&state, &selected, recipe, 40.)
            .unwrap_err()
            .contains("outside the selected silhouette"));
    }

    #[test]
    fn pocket_walls_with_intermediate_trims_and_capped_entries_are_rejected() {
        for defect in ["intermediate trim", "capped entry"] {
            let (mut state, selected) = pocket();
            let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
            let floor = topology.faces.iter().find(|f| f.id == selected.id).unwrap();
            let wire = topology
                .loops
                .iter()
                .find(|w| w.id == floor.loop_ids[0])
                .unwrap();
            let wall_id = topology
                .edges
                .iter()
                .find(|e| e.id == wire.edge_uses[0].edge_id)
                .unwrap()
                .face_ids
                .iter()
                .copied()
                .find(|id| *id != selected.id)
                .unwrap();
            let wall = topology.faces.iter().find(|f| f.id == wall_id).unwrap();
            let wall_wire = topology
                .loops
                .iter()
                .find(|w| w.id == wall.loop_ids[0])
                .unwrap();
            let top_edge = wall_wire
                .edge_uses
                .iter()
                .map(|u| u.edge_id)
                .find(|id| {
                    matches!(topology.edges.iter().find(|e| e.id == *id).unwrap().curve,
                    CadCurve::Line { start, end } if start[2] == 0. && end[2] == 0.)
                })
                .unwrap();
            if defect == "intermediate trim" {
                let edge = topology
                    .edges
                    .iter_mut()
                    .find(|e| e.id == top_edge)
                    .unwrap();
                if let CadCurve::Line { end, .. } = &mut edge.curve {
                    end[2] = -1.;
                }
            } else {
                let entry_id = topology
                    .edges
                    .iter()
                    .find(|e| e.id == top_edge)
                    .unwrap()
                    .face_ids
                    .iter()
                    .copied()
                    .find(|id| *id != wall_id)
                    .unwrap();
                topology
                    .loops
                    .iter_mut()
                    .find(|w| {
                        w.face_id == entry_id && w.edge_uses.iter().any(|u| u.edge_id == top_edge)
                    })
                    .unwrap()
                    .is_outer = true;
            }
            let error = resolve_selection(&state, &[selected], ExtractionRecipe::PocketFloor, 0.)
                .unwrap_err();
            assert!(
                error.contains(if defect == "intermediate trim" {
                    "trims"
                } else {
                    "covers the selected void"
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn facing_uses_exact_body_bounds_despite_splines_holes_and_blends_below_the_target() {
        let mut fixture = Fixture::new();
        let target = fixture.face(
            vec![
                vec![[0., 0., 0.], [20., 0., 0.], [18., 20., 0.], [0., 16., 0.]],
                square(5., 7., 5., 7., 0.),
            ],
            [0., 0., 1.],
        );
        fixture.t.edges[0].curve = CadCurve::Unsupported {
            description: "exact spline retained in B-rep".into(),
        };
        let side = fixture.face(vec![square(0., 20., 0., 20., -2.)], [0., 0., -1.]);
        fixture
            .t
            .faces
            .iter_mut()
            .find(|face| face.id == side)
            .unwrap()
            .surface = CadSurface::Unsupported {
            description: "blend below target".into(),
        };
        let state = fixture.state();
        let selected = [reference(target, EntityKind::Face)];
        let binding =
            resolve_selection(&state, &selected, ExtractionRecipe::FaceRegion, 1.).unwrap();
        let input = resolve_binding(&state, &binding).unwrap();
        assert_eq!(
            input.heights,
            CuttingHeights {
                top: 1.,
                bottom: 0.
            }
        );
        assert_eq!(input.regions[0].area(), 400.);
        assert!(
            input.regions[0].holes().is_empty(),
            "facing clears stock above the body, including above holes"
        );
        assert!(
            resolve_selection(&state, &selected, ExtractionRecipe::PocketFloor, 1.).is_err(),
            "pocket certification is unchanged"
        );
        let mut boss = state;
        std::sync::Arc::make_mut(&mut boss.assets[0].topology)
            .faces
            .iter_mut()
            .find(|face| face.id == side)
            .unwrap()
            .bounds[1][2] = 2.;
        assert!(
            resolve_selection(&boss, &selected, ExtractionRecipe::FaceRegion, 3.)
                .unwrap_err()
                .contains("protected material")
        );
    }

    #[test]
    fn circular_edges_alone_do_not_establish_holes() {
        let (state, r) = pocket();
        let mut edge = r;
        edge.kind = EntityKind::Edge;
        edge.id = state.assets[0].topology.edges[0].id;
        assert!(
            resolve_selection(&state, &[edge], ExtractionRecipe::DrillHoles, 0.)
                .unwrap_err()
                .contains("circular edges")
        );
    }
    fn projected_l_prism() -> (GeometryState, EntityRef) {
        let mut fixture = Fixture::new();
        let face = fixture.id();
        fixture.t.bodies[0].bounds = [[0., 0., -5.], [4., 4., 0.]];
        fixture.t.faces.push(CadFace {
            id: face,
            body_id: 1,
            surface: CadSurface::Unsupported {
                description: "rounded exterior".into(),
            },
            loop_ids: vec![],
            bounds: fixture.t.bodies[0].bounds,
        });
        fixture.t.projection = Some(CadProjectionMesh {
            chord_tolerance: 0.0025,
            mesh: CadMesh {
                vertices: vec![
                    [0., 0., 0.],
                    [4., 0., 0.],
                    [4., 1., 0.],
                    [1., 1., 0.],
                    [1., 4., 0.],
                    [0., 4., 0.],
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]],
                triangle_faces: vec![face; 4],
            },
        });
        let geometry = GeometryState {
            assets: vec![GeometryAsset {
                id: 1,
                revision: 1,
                name: "L".into(),
                source_format: CadFormat::Step,
                source_data: vec![].into(),
                source_units: SourceUnits::Millimetres,
                backend: "test".into(),
                import_options: None,
                import_diagnostics: vec![],
                source_fingerprint: String::new(),
                topology: fixture.t.into(),
            }],
            instances: vec![GeometryInstance {
                id: 1,
                asset_id: 1,
                body_id: 1,
                name: "L".into(),
                placement: RigidTransform::default(),
                visible: true,
            }],
            ..Default::default()
        };
        (
            geometry,
            EntityRef {
                asset_id: 1,
                revision: 1,
                instance_id: 1,
                body_id: 1,
                kind: EntityKind::Silhouette,
                id: 1,
            },
        )
    }

    #[test]
    fn xy_silhouette_preserves_concavities_and_is_independent_of_display_geometry() {
        let (mut geometry, selected) = projected_l_prism();
        let binding = resolve_selection(
            &geometry,
            &[selected],
            ExtractionRecipe::ProfileLoop {
                side: Side::Outside,
            },
            0.,
        )
        .unwrap();
        let before = resolve_binding(&geometry, &binding).unwrap();
        assert_eq!(
            before.heights,
            CuttingHeights {
                top: 0.,
                bottom: -5.
            }
        );
        let region = &before.regions[0];
        assert!(region.contains(Point::new(3., 0.5)));
        assert!(region.contains(Point::new(0.5, 3.)));
        assert!(
            !region.contains(Point::new(2., 2.)),
            "projection must not become a convex hull or bounding box"
        );
        assert!(region.area() > 7. && region.area() < 7.1);
        std::sync::Arc::make_mut(&mut geometry.assets[0].topology).mesh = CadMesh {
            vertices: vec![[100., 100., 100.]],
            ..Default::default()
        };
        assert_eq!(before, resolve_binding(&geometry, &binding).unwrap());
        geometry.instances[0].placement.translation = [10., 20., 30.];
        let moved = resolve_binding(&geometry, &binding).unwrap();
        assert!(moved.regions[0].contains(Point::new(13., 20.5)));
        assert!(!moved.regions[0].contains(Point::new(3., 0.5)));
        assert_eq!(
            moved.heights,
            CuttingHeights {
                top: 30.,
                bottom: 25.
            }
        );
        assert_ne!(before.fingerprint, moved.fingerprint);
        geometry.instances[0].placement.rotation = [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]];
        let frame = SetupFrame::from_axes(Some([1., 0., 0.]), Some([0., 0., 1.]), None).unwrap();
        let in_setup = geometry.geometry_in_setup(&frame).unwrap();
        let rotated = resolve_binding(&in_setup, &binding).unwrap();
        assert!(rotated.regions[0].contains(Point::new(13., 30.5)));
        assert!(!rotated.regions[0].contains(Point::new(12., 32.)));
        assert_eq!(
            rotated.heights,
            CuttingHeights {
                top: -20.,
                bottom: -25.
            }
        );
    }

    #[test]
    fn profile_silhouettes_include_through_openings_and_fill_blind_recesses() {
        let (mut geometry, selected) = projected_l_prism();
        let topology = std::sync::Arc::make_mut(&mut geometry.assets[0].topology);
        let face = topology.faces[0].id;
        let projection = topology.projection.as_mut().unwrap();
        projection.mesh.vertices = vec![
            [0., 0., 0.],
            [4., 0., 0.],
            [4., 4., 0.],
            [0., 4., 0.],
            [1., 1., 0.],
            [3., 1., 0.],
            [3., 3., 0.],
            [1., 3., 0.],
        ];
        projection.mesh.triangles = vec![
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        projection.mesh.triangle_faces = vec![face; 8];
        let binding = resolve_selection(
            &geometry,
            std::slice::from_ref(&selected),
            ExtractionRecipe::ProfileSilhouette,
            0.,
        )
        .unwrap();
        let input = resolve_binding(&geometry, &binding).unwrap();
        assert_eq!(input.profiles.len(), 2);
        assert_eq!(input.profiles[0].side, Side::Outside);
        assert_eq!(input.profiles[1].side, Side::Inside);
        assert!(!input.regions[0].contains(Point::new(2., 2.)));
        // The older envelope-only recipe and hover cache retain their original behavior.
        assert!(silhouette(&geometry, &selected).unwrap().holes().is_empty());
        let projection = std::sync::Arc::make_mut(&mut geometry.assets[0].topology)
            .projection
            .as_mut()
            .unwrap();
        projection.mesh.triangles.extend([[4, 5, 6], [4, 6, 7]]);
        projection.mesh.triangle_faces.extend([face; 2]);
        let input = resolve_binding(&geometry, &binding).unwrap();
        assert_eq!(
            input.profiles.len(),
            1,
            "a projected recess floor fills the opening"
        );
        assert!(input.regions[0].contains(Point::new(2., 2.)));
    }

    #[test]
    fn xy_silhouette_rejects_missing_incomplete_stale_and_coarse_projection_data() {
        let (mut geometry, selected) = projected_l_prism();
        let topology = std::sync::Arc::make_mut(&mut geometry.assets[0].topology);
        let saved = topology.projection.take().unwrap();
        assert!(silhouette(&geometry, &selected)
            .unwrap_err()
            .contains("Reimport"));
        std::sync::Arc::make_mut(&mut geometry.assets[0].topology).projection = Some(saved);
        let projection = std::sync::Arc::make_mut(&mut geometry.assets[0].topology)
            .projection
            .as_mut()
            .unwrap();
        projection.chord_tolerance = 0.5;
        assert!(silhouette(&geometry, &selected)
            .unwrap_err()
            .contains("tolerance"));
        let projection = std::sync::Arc::make_mut(&mut geometry.assets[0].topology)
            .projection
            .as_mut()
            .unwrap();
        projection.chord_tolerance = 0.0025;
        projection.mesh.triangles.clear();
        projection.mesh.triangle_faces.clear();
        assert!(silhouette(&geometry, &selected)
            .unwrap_err()
            .contains("Incomplete"));
        geometry.assets[0].revision += 1;
        assert!(silhouette(&geometry, &selected)
            .unwrap_err()
            .contains("stale"));
    }
}
