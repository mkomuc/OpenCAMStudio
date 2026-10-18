//! Persistent reference-plus-offset operation heights, resolved in setup axes.
use crate::{CuttingHeights, Document, EntityKind, EntityRef, Heights, Operation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HeightPlaneKind {
    Clearance,
    Retract,
    Feed,
    Top,
    Bottom,
}
impl HeightPlaneKind {
    pub const ALL: [Self; 5] = [
        Self::Clearance,
        Self::Retract,
        Self::Feed,
        Self::Top,
        Self::Bottom,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap()
    }
}
impl std::fmt::Display for HeightPlaneKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Clearance => "Clearance height",
            Self::Retract => "Retract height",
            Self::Feed => "Feed height",
            Self::Top => "Top height",
            Self::Bottom => "Bottom height",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeightReference {
    Plane(HeightPlaneKind),
    Disabled,
    ModelTop,
    ModelBottom,
    StockTop,
    StockBottom,
    FixtureTop,
    FixtureBottom,
    SelectedContours,
    Selection,
    Origin,
    Highest,
    Lowest,
}
impl HeightReference {
    pub fn options(plane: HeightPlaneKind) -> Vec<Self> {
        let mut choices: Vec<_> = HeightPlaneKind::ALL
            .into_iter()
            .filter(|p| *p != plane)
            .map(Self::Plane)
            .collect();
        if plane == HeightPlaneKind::Feed {
            choices.insert(2, Self::Disabled);
        }
        choices.extend([
            Self::ModelTop,
            Self::ModelBottom,
            Self::StockTop,
            Self::StockBottom,
            Self::FixtureTop,
            Self::FixtureBottom,
            Self::SelectedContours,
            Self::Selection,
            Self::Origin,
            Self::Highest,
            Self::Lowest,
        ]);
        choices
    }
}
impl std::fmt::Display for HeightReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plane(p) => p.fmt(f),
            _ => f.write_str(match self {
                Self::Disabled => "Disabled",
                Self::ModelTop => "Model top",
                Self::ModelBottom => "Model bottom",
                Self::StockTop => "Stock top",
                Self::StockBottom => "Stock bottom",
                Self::FixtureTop => "Fixture top",
                Self::FixtureBottom => "Fixture bottom",
                Self::SelectedContours => "Selected contour(s)",
                Self::Selection => "Selection",
                Self::Origin => "Origin (absolute)",
                Self::Highest => "Highest of…",
                Self::Lowest => "Lowest of…",
                _ => unreachable!(),
            }),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingHeightSelection {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeightDefinition {
    pub reference: HeightReference,
    pub offset: f64,
    /// Stable CAD geometry for selection/fixture references; never a screen mesh.
    #[serde(default)]
    pub entities: Vec<EntityRef>,
    #[serde(default)]
    pub drawings: Vec<DrawingHeightSelection>,
    /// Reference set for Highest/Lowest. Nested aggregates are rejected.
    #[serde(default)]
    pub sources: Vec<HeightReference>,
}
impl HeightDefinition {
    pub fn new(reference: HeightReference, offset: f64) -> Self {
        Self {
            reference,
            offset,
            entities: vec![],
            drawings: vec![],
            sources: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationHeightSettings {
    pub planes: [HeightDefinition; 5],
    /// A worker resolves source geometry before transforming it into setup axes.
    /// This transient snapshot prevents applying that transformation twice.
    #[serde(skip)]
    pub resolved: Option<ResolvedOperationHeights>,
}
impl Default for OperationHeightSettings {
    fn default() -> Self {
        use HeightPlaneKind::*;
        use HeightReference::*;
        Self {
            planes: [
                HeightDefinition::new(Plane(Retract), 10.),
                HeightDefinition::new(StockTop, 5.),
                HeightDefinition::new(Plane(Top), 5.),
                HeightDefinition::new(StockTop, 0.),
                HeightDefinition::new(SelectedContours, 0.),
            ],
            resolved: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedOperationHeights {
    pub stock_top: f64,
    pub clearance: f64,
    pub retract: f64,
    pub feed: Option<f64>,
    pub top: f64,
    pub bottom: f64,
}
impl ResolvedOperationHeights {
    pub fn plane(self, kind: HeightPlaneKind) -> Option<f64> {
        match kind {
            HeightPlaneKind::Clearance => Some(self.clearance),
            HeightPlaneKind::Retract => Some(self.retract),
            HeightPlaneKind::Feed => self.feed,
            HeightPlaneKind::Top => Some(self.top),
            HeightPlaneKind::Bottom => Some(self.bottom),
        }
    }
    pub fn safety(self) -> Heights {
        Heights::new(self.clearance, self.retract, self.stock_top)
    }
    pub fn cutting(self) -> CuttingHeights {
        CuttingHeights {
            top: self.top,
            bottom: self.bottom,
        }
    }
    pub fn validate(self) -> Result<(), String> {
        if ![
            self.stock_top,
            self.clearance,
            self.retract,
            self.top,
            self.bottom,
        ]
        .into_iter()
        .chain(self.feed)
        .all(f64::is_finite)
        {
            return Err("Height references must resolve to finite coordinates".into());
        }
        if self.top <= self.bottom {
            return Err("Top height must be above bottom height".into());
        }
        if self.clearance < self.retract || self.retract < self.top {
            return Err(
                "Clearance must be at or above retract, and retract at or above top height".into(),
            );
        }
        if self
            .feed
            .is_some_and(|z| z < self.top || z > self.clearance)
        {
            return Err("Feed height must be between top and clearance heights".into());
        }
        Ok(())
    }
}
impl OperationHeightSettings {
    pub fn definition(&self, plane: HeightPlaneKind) -> &HeightDefinition {
        &self.planes[plane.index()]
    }
    pub fn definition_mut(&mut self, plane: HeightPlaneKind) -> &mut HeightDefinition {
        self.resolved = None;
        &mut self.planes[plane.index()]
    }
    pub fn validate_structure(&self) -> Result<(), String> {
        for plane in HeightPlaneKind::ALL {
            let d = self.definition(plane);
            if !d.offset.is_finite() {
                return Err(format!("{plane}: offset must be a finite number"));
            }
            if d.reference == HeightReference::Disabled && plane != HeightPlaneKind::Feed {
                return Err("Only feed height may be disabled".into());
            }
            if d.reference == HeightReference::Plane(plane) {
                return Err(format!("{plane} cannot reference itself"));
            }
            if matches!(
                d.reference,
                HeightReference::Highest | HeightReference::Lowest
            ) && (d.sources.is_empty()
                || d.sources.iter().any(|s| {
                    matches!(
                        s,
                        HeightReference::Highest
                            | HeightReference::Lowest
                            | HeightReference::Disabled
                    )
                }))
            {
                return Err(format!(
                    "{plane}: choose at least one non-aggregate reference"
                ));
            }
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        document: &Document,
        operation: &Operation,
    ) -> Result<ResolvedOperationHeights, String> {
        if let Some(resolved) = self.resolved {
            return Ok(resolved);
        }
        self.validate_structure()?;
        let mut values = [None; 5];
        let mut visiting = [false; 5];
        for p in HeightPlaneKind::ALL {
            self.resolve_plane(p, document, operation, &mut values, &mut visiting)?;
        }
        let (model_min, model_max) = document.height_model_bounds();
        let resolved = ResolvedOperationHeights {
            stock_top: document
                .setup()
                .stock
                .resolve_bounds(model_min, model_max)
                .1[2],
            clearance: values[0].unwrap(),
            retract: values[1].unwrap(),
            feed: values[2],
            top: values[3].unwrap(),
            bottom: values[4].unwrap(),
        };
        resolved.validate()?;
        if let Operation::Chamfer(op) = operation {
            let minimum = document.chamfer_minimum_depth(op);
            if resolved.top - resolved.bottom < minimum - 1e-9 {
                return Err(format!("Bottom height must allow at least {minimum:.3} mm below top for this chamfer width and cutter angle"));
            }
        }
        Ok(resolved)
    }
    fn resolve_plane(
        &self,
        p: HeightPlaneKind,
        doc: &Document,
        op: &Operation,
        values: &mut [Option<f64>; 5],
        visiting: &mut [bool; 5],
    ) -> Result<Option<f64>, String> {
        let i = p.index();
        if let Some(z) = values[i] {
            return Ok(Some(z));
        }
        if visiting[i] {
            return Err(format!("Circular height reference involving {p}"));
        }
        let d = self.definition(p);
        if d.reference == HeightReference::Disabled {
            return Ok(None);
        }
        visiting[i] = true;
        let base = self.resolve_reference(d.reference, d, doc, op, values, visiting)?;
        let z = base + d.offset;
        if !z.is_finite() {
            return Err(format!("{p}: resolved height is not finite"));
        }
        visiting[i] = false;
        values[i] = Some(z);
        Ok(Some(z))
    }
    fn resolve_reference(
        &self,
        reference: HeightReference,
        d: &HeightDefinition,
        doc: &Document,
        op: &Operation,
        values: &mut [Option<f64>; 5],
        visiting: &mut [bool; 5],
    ) -> Result<f64, String> {
        use HeightReference::*;
        let setup = doc.setup();
        let (model_min, model_max) = doc.height_model_bounds();
        let (stock_min, stock_max) = setup.stock.resolve_bounds(model_min, model_max);
        Ok(match reference {
            Plane(p) => self
                .resolve_plane(p, doc, op, values, visiting)?
                .ok_or("A height cannot reference disabled feed height")?,
            Disabled => return Err("Disabled height cannot be a reference".into()),
            ModelTop => model_max[2],
            ModelBottom => model_min[2],
            StockTop => stock_max[2],
            StockBottom => stock_min[2],
            Origin => setup.origin_position(op.work_offset())[2],
            SelectedContours => doc.selected_contour_height(op)?,
            Selection | FixtureTop | FixtureBottom => {
                let (min, max) = doc.height_definition_bounds(d)?;
                if reference == Selection && (max[2] - min[2]).abs() > 1e-5 {
                    return Err("Selection must lie on one plane perpendicular to setup Z; use Highest/Lowest or a fixture limit for non-planar geometry".into());
                }
                if reference == FixtureBottom {
                    min[2]
                } else {
                    max[2]
                }
            }
            Highest | Lowest => {
                let zs = d
                    .sources
                    .iter()
                    .map(|source| {
                        if *source == Selection {
                            let (min, max) = doc.height_definition_bounds(d)?;
                            Ok(if reference == Highest { max[2] } else { min[2] })
                        } else {
                            self.resolve_reference(*source, d, doc, op, values, visiting)
                        }
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                zs.into_iter()
                    .reduce(if reference == Highest {
                        f64::max
                    } else {
                        f64::min
                    })
                    .ok_or("Choose references for Highest/Lowest")?
            }
        })
    }
    /// Preserve a pre-reference operation without silently changing its toolpath.
    pub fn legacy(document: &Document, operation: &Operation) -> Self {
        let safety = document.setup().heights_for(operation.id());
        let cutting = document
            .geometry
            .bindings
            .get(&operation.id())
            .map(|b| b.effective_heights())
            .unwrap_or_else(|| document.operation_base_cutting_heights(operation));
        let origin = document.setup().origin_position(operation.work_offset())[2];
        let mut settings = Self::default();
        for (p, z) in [
            (HeightPlaneKind::Clearance, safety.clearance),
            (HeightPlaneKind::Retract, safety.retract),
            (HeightPlaneKind::Top, cutting.top),
            (HeightPlaneKind::Bottom, cutting.bottom),
        ] {
            settings.planes[p.index()] = HeightDefinition::new(HeightReference::Origin, z - origin);
        }
        settings.planes[2] = HeightDefinition::new(HeightReference::Disabled, 0.);
        settings
    }
}
pub fn legacy_cutting(op: &Operation, h: &Heights) -> CuttingHeights {
    op.legacy_cutting_heights(h).unwrap_or_else(|| match op {
        Operation::Thread(o) => CuttingHeights {
            top: o.z_top,
            bottom: o.z_bottom,
        },
        Operation::Chamfer(o) => CuttingHeights {
            top: o.top,
            bottom: o.top - o.depth,
        },
        Operation::Engrave(o) => CuttingHeights {
            top: o.top,
            bottom: o.top - o.depth,
        },
        Operation::Carve(o) => CuttingHeights {
            top: o.top,
            bottom: o.top - o.depth,
        },
        _ => unreachable!(),
    })
}
impl Document {
    fn selected_contour_height(&self, op: &Operation) -> Result<f64, String> {
        let Some(binding) = self.geometry.bindings.get(&op.id()) else {
            return Ok(self.operation_base_cutting_heights(op).bottom);
        };
        // A selected edge chain has its own Z plane. The binding's interval
        // describes the adjoining wall that is safe to cut, and its bottom can
        // be well below that plane (or contain an older cached depth).
        let references: Vec<_> = match &binding.recipe {
            crate::ExtractionRecipe::ProfileEntries { entries }
                if entries.iter().all(|entry| {
                    entry.kind == crate::ProfileGeometryKind::Chain
                        && matches!(&entry.source, crate::ProfileGeometrySource::Cad(_))
                }) =>
            {
                entries
                    .iter()
                    .flat_map(|entry| match &entry.source {
                        crate::ProfileGeometrySource::Cad(refs) => refs.clone(),
                        _ => unreachable!(),
                    })
                    .collect()
            }
            crate::ExtractionRecipe::ProfileContours => binding.selected.clone(),
            _ => vec![],
        };
        if references.is_empty() || references.iter().any(|r| r.kind != EntityKind::Edge) {
            return Ok(binding.heights.bottom);
        }
        let (min, max) = self.height_selection_bounds(&references)?;
        if (max[2] - min[2]).abs() > 1e-5 {
            return Err(
                "Selected contour(s) must lie on one plane perpendicular to setup Z".into(),
            );
        }
        Ok(max[2])
    }
    fn chamfer_minimum_depth(&self, op: &crate::ChamferOp) -> f64 {
        let angle = self
            .setup()
            .tools
            .iter()
            .find(|tool| tool.number == op.tool)
            .and_then(|tool| match tool.kind {
                crate::ToolKind::ChamferMill {
                    included_angle_deg, ..
                }
                | crate::ToolKind::VBit {
                    included_angle_deg, ..
                } => Some(included_angle_deg),
                _ => None,
            })
            .unwrap_or(90.);
        op.width / (angle.to_radians() * 0.5).tan()
    }
    /// A chamfer's zero depth means its natural cutter depth, rather than a zero
    /// cutting interval. All other strategies store their target interval directly.
    pub fn operation_base_cutting_heights(&self, op: &Operation) -> CuttingHeights {
        let mut cutting = legacy_cutting(op, &self.setup().heights_for(op.id()));
        if let Operation::Chamfer(chamfer) = op {
            cutting.bottom = chamfer.top - chamfer.depth.max(self.chamfer_minimum_depth(chamfer));
        }
        cutting
    }
    /// Same conservative source bounds used for stock, independent of display mesh.
    pub fn height_model_bounds(&self) -> ([f64; 3], [f64; 3]) {
        let setup = self.setup();
        let mut points = vec![];
        for instance in &self.geometry.instances {
            if !setup.model_instances.contains(&instance.id) {
                continue;
            }
            if let Some(body) = self
                .geometry
                .asset(instance.asset_id)
                .and_then(|a| a.topology.bodies.iter().find(|b| b.id == instance.body_id))
            {
                points.extend(box_corners(body.bounds).map(|p| {
                    setup
                        .frame
                        .to_setup_vector(instance.placement.transform_point(p))
                }));
            }
        }
        for p in self
            .geometry
            .drawing_regions
            .iter()
            .flat_map(|r| r.outer().points())
            .chain(self.geometry.drawing_paths.iter().flat_map(|p| p.points()))
        {
            points.push(setup.frame.to_setup_vector([p.x, p.y, 0.]));
        }
        bounds(&points).unwrap_or(([0.; 3], [0.; 3]))
    }
    pub fn height_definition_bounds(
        &self,
        d: &HeightDefinition,
    ) -> Result<([f64; 3], [f64; 3]), String> {
        let mut points = vec![];
        if !d.entities.is_empty() {
            let (min, max) = self.height_selection_bounds(&d.entities)?;
            points.extend([min, max]);
        }
        for drawing in &d.drawings {
            let matches = |source: &[cam_geo::Point]| {
                source
                    .iter()
                    .map(|p| [p.x, p.y])
                    .eq(drawing.points.iter().copied())
            };
            let exists = if drawing.closed {
                self.geometry.drawing_regions.iter().any(|r| {
                    std::iter::once(r.outer())
                        .chain(r.holes())
                        .any(|c| matches(c.points()))
                })
            } else {
                self.geometry
                    .drawing_paths
                    .iter()
                    .any(|p| matches(p.points()))
            };
            if !exists {
                return Err("Drawing height reference changed; pick it again".into());
            }
            points.extend(
                drawing
                    .points
                    .iter()
                    .map(|[x, y]| self.setup().frame.to_setup_vector([*x, *y, 0.])),
            );
        }
        bounds(&points).ok_or_else(|| "Pick reference geometry in the viewport".into())
    }
    pub fn height_selection_bounds(
        &self,
        refs: &[EntityRef],
    ) -> Result<([f64; 3], [f64; 3]), String> {
        let mut points = vec![];
        for r in refs {
            if !self.geometry.entity_exists(r) {
                return Err(
                    "Height reference geometry is missing or changed; pick it again".into(),
                );
            }
            let instance = self.geometry.instance(r.instance_id).unwrap();
            let t = &self.geometry.asset(r.asset_id).unwrap().topology;
            let transform = |p| {
                self.setup()
                    .frame
                    .to_setup_vector(instance.placement.transform_point(p))
            };
            let face_id = if r.kind == EntityKind::Loop {
                t.loops.iter().find(|l| l.id == r.id).map(|l| l.face_id)
            } else {
                None
            };
            match r.kind {
                EntityKind::Body | EntityKind::Silhouette => points.extend(
                    box_corners(t.bodies.iter().find(|b| b.id == r.body_id).unwrap().bounds)
                        .map(transform),
                ),
                EntityKind::Face | EntityKind::Loop => {
                    let face = t
                        .faces
                        .iter()
                        .find(|f| f.id == face_id.unwrap_or(r.id))
                        .unwrap();
                    if let crate::CadSurface::Plane { origin, normal } = face.surface {
                        let n = self
                            .setup()
                            .frame
                            .to_setup_vector(instance.placement.transform_vector(normal));
                        if n[0].abs() < 1e-7 && n[1].abs() < 1e-7 {
                            points.push(transform(origin));
                            continue;
                        }
                    }
                    points.extend(box_corners(face.bounds).map(transform));
                }
                EntityKind::Vertex => points.push(transform(
                    t.vertices.iter().find(|v| v.id == r.id).unwrap().point,
                )),
                EntityKind::Edge => match &t.edges.iter().find(|e| e.id == r.id).unwrap().curve {
                    crate::CadCurve::Line { start, end } => {
                        points.extend([transform(*start), transform(*end)])
                    }
                    crate::CadCurve::CircleArc {
                        center,
                        normal,
                        x_axis,
                        radius,
                        start_angle,
                        end_angle,
                    } => {
                        let y = [
                            normal[1] * x_axis[2] - normal[2] * x_axis[1],
                            normal[2] * x_axis[0] - normal[0] * x_axis[2],
                            normal[0] * x_axis[1] - normal[1] * x_axis[0],
                        ];
                        let xz = self
                            .setup()
                            .frame
                            .to_setup_vector(instance.placement.transform_vector(*x_axis))[2];
                        let yz = self
                            .setup()
                            .frame
                            .to_setup_vector(instance.placement.transform_vector(y))[2];
                        let mut angles = vec![*start_angle, *end_angle];
                        let peak = yz.atan2(xz);
                        let lo = start_angle.min(*end_angle);
                        let hi = start_angle.max(*end_angle);
                        for base in [peak, peak + std::f64::consts::PI] {
                            let turn = ((lo - base) / std::f64::consts::TAU).ceil();
                            let angle = base + turn * std::f64::consts::TAU;
                            if angle <= hi {
                                angles.push(angle);
                            }
                        }
                        points.extend(angles.into_iter().map(|a| {
                            transform(std::array::from_fn(|i| {
                                center[i] + radius * (x_axis[i] * a.cos() + y[i] * a.sin())
                            }))
                        }));
                    }
                    crate::CadCurve::BezierSpans { spans } => {
                        // Control hull bounds conservatively enclose every point
                        // of a positive-weight rational spline in any setup.
                        points.extend(spans.iter().flat_map(|s| &s.poles).map(|p| transform(*p)));
                    }
                    _ => return Err("This edge cannot define an exact height reference".into()),
                },
            }
        }
        bounds(&points).ok_or_else(|| "Pick reference geometry in the viewport".into())
    }
}
fn box_corners(b: [[f64; 3]; 2]) -> [[f64; 3]; 8] {
    std::array::from_fn(|i| std::array::from_fn(|a| b[usize::from(i & (1 << a) != 0)][a]))
}
fn bounds(points: &[[f64; 3]]) -> Option<([f64; 3], [f64; 3])> {
    if points.is_empty() {
        None
    } else {
        Some((
            std::array::from_fn(|a| points.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min)),
            std::array::from_fn(|a| {
                points
                    .iter()
                    .map(|p| p[a])
                    .fold(f64::NEG_INFINITY, f64::max)
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    fn doc() -> Document {
        let mut doc:Document=serde_json::from_value(serde_json::json!({"schema_version":18,"setups":[{"id":1,"name":"reference test","heights":{"clearance":5.,"retract":2.,"top_of_stock":0.},"stock":{"BoundingBox":{"x_offset":0.,"y_offset":0.,"top":10.,"thickness":15.}},"tools":[],"operations":[]}]})).unwrap();
        doc.setup_mut().operations.push(Operation::Drill(DrillOp {
            id: 1,
            tool: 1,
            points: vec![[0., 0.]],
            depth: 4.,
            start_offset: 0.,
            peck: None,
            dwell: None,
            spindle_rpm: 1000.,
            tool_preset: None,
            entry_feeds: Default::default(),
            work_offset: 1,
            feed: 100.,
        }));
        doc
    }
    #[test]
    fn screenshot_defaults_resolve_dependencies_and_follow_stock_changes() {
        let mut doc = doc();
        let settings = OperationHeightSettings::default();
        let resolved = settings.resolve(&doc, &doc.setup().operations[0]).unwrap();
        assert_eq!(
            (
                resolved.clearance,
                resolved.retract,
                resolved.feed,
                resolved.top,
                resolved.bottom
            ),
            (25., 15., Some(15.), 10., -4.)
        );
        doc.setup_mut().stock = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 12.,
            thickness: 15.,
        };
        let next = settings.resolve(&doc, &doc.setup().operations[0]).unwrap();
        assert_eq!(
            (
                next.clearance,
                next.retract,
                next.feed,
                next.top,
                next.bottom
            ),
            (27., 17., Some(17.), 12., -4.)
        );
    }
    #[test]
    fn rejects_indirect_cycles_disabled_dependencies_and_overflow() {
        let doc = doc();
        let op = &doc.setup().operations[0];
        let mut settings = OperationHeightSettings::default();
        settings.definition_mut(HeightPlaneKind::Retract).reference =
            HeightReference::Plane(HeightPlaneKind::Clearance);
        assert!(settings.resolve(&doc, op).unwrap_err().contains("Circular"));
        settings = OperationHeightSettings::default();
        settings.definition_mut(HeightPlaneKind::Feed).reference = HeightReference::Disabled;
        settings
            .definition_mut(HeightPlaneKind::Clearance)
            .reference = HeightReference::Plane(HeightPlaneKind::Feed);
        assert!(settings.resolve(&doc, op).unwrap_err().contains("disabled"));
        settings = OperationHeightSettings::default();
        settings.definition_mut(HeightPlaneKind::Retract).offset = f64::MAX;
        settings.definition_mut(HeightPlaneKind::Clearance).offset = f64::MAX;
        assert!(settings.resolve(&doc, op).unwrap_err().contains("finite"));
    }
    #[test]
    fn aggregate_references_are_evaluated_before_offset_and_origin_is_operation_datum() {
        let mut doc = doc();
        doc.setup_mut().extra_origins.push(Origin {
            index: 2,
            position: [0., 0., 3.],
        });
        doc.setup_mut().operations[0].set_work_offset(2);
        let op = &doc.setup().operations[0];
        let mut settings = OperationHeightSettings::default();
        let top = settings.definition_mut(HeightPlaneKind::Top);
        top.reference = HeightReference::Highest;
        top.sources = vec![HeightReference::ModelTop, HeightReference::Origin];
        top.offset = 2.;
        assert_eq!(settings.resolve(&doc, op).unwrap().top, 5.);
        let bottom = settings.definition_mut(HeightPlaneKind::Bottom);
        bottom.reference = HeightReference::Lowest;
        bottom.sources = vec![
            HeightReference::StockBottom,
            HeightReference::SelectedContours,
        ];
        bottom.offset = -1.;
        assert_eq!(settings.resolve(&doc, op).unwrap().bottom, -6.);
    }
    fn add_body(doc: &mut Document) -> EntityRef {
        let topology = TopologySnapshot {
            bodies: vec![CadBody {
                id: 1,
                name: "fixture".into(),
                valid_solid: true,
                bounds: [[0., 0., -2.], [20., 10., 7.]],
            }],
            vertices: vec![CadVertex {
                id: 2,
                body_id: 1,
                point: [0., 0., 6.],
            }],
            ..Default::default()
        };
        doc.geometry.assets.push(GeometryAsset {
            import_options: None,
            id: 1,
            revision: 1,
            name: "fixture".into(),
            source_format: CadFormat::Step,
            source_data: vec![].into(),
            source_units: SourceUnits::Millimetres,
            backend: "height test".into(),
            import_diagnostics: vec![],
            source_fingerprint: String::new(),
            topology: topology.into(),
        });
        doc.geometry.instances.push(GeometryInstance {
            id: 1,
            asset_id: 1,
            body_id: 1,
            name: "fixture".into(),
            placement: RigidTransform::default(),
            visible: true,
        });
        EntityRef {
            asset_id: 1,
            revision: 1,
            instance_id: 1,
            body_id: 1,
            kind: EntityKind::Body,
            id: 1,
        }
    }
    #[test]
    fn selected_chain_height_uses_source_plane_in_setup_axes_instead_of_cached_wall_bottom() {
        let mut doc = doc();
        let mut edge = add_body(&mut doc);
        edge.kind = EntityKind::Edge;
        edge.id = 3;
        std::sync::Arc::make_mut(&mut doc.geometry.assets[0].topology)
            .edges
            .push(CadEdge {
                id: 3,
                body_id: 1,
                vertices: [2, 2],
                face_ids: vec![],
                curve: CadCurve::BezierSpans {
                    spans: vec![CadBezierSpan {
                        poles: vec![[0., 30., 4.], [2., 30., 3.], [4., 30., 0.]],
                        weights: vec![1.; 3],
                    }],
                },
            });
        doc.setup_mut().frame.world_from_setup.rotation =
            [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]];
        doc.setup_mut().stock = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 40.,
            thickness: 45.,
        };
        let profile: Operation = serde_json::from_value(serde_json::json!({"Profile": {
            "id": 1, "tool": 1, "chain": {"points": [{"x":0.,"y":-4.},{"x":4.,"y":0.}]},
            "closed": false, "side": "Outside", "comp": "Computed", "depth": 2.,
            "stepdown": 2., "feed": 100., "plunge_feed": 100., "start": null,
            "lead_in": "None", "lead_out": "None", "plunge": "Straight"
        }}))
        .unwrap();
        doc.setup_mut().operations = vec![profile];
        let mut entry = ProfileGeometryEntry::new(ProfileGeometryKind::Chain, "Chain 1".into());
        entry.source = ProfileGeometrySource::Cad(vec![edge.clone()]);
        doc.geometry.bindings.insert(
            1,
            GeometryBinding {
                selected: vec![edge],
                recipe: ExtractionRecipe::ProfileEntries {
                    entries: vec![entry],
                },
                heights: CuttingHeights {
                    top: 30.,
                    bottom: 28.,
                },
                overrides: GeometryOverrides {
                    top: None,
                    bottom: Some(27.),
                },
                status: ResolutionStatus::Resolved,
                resolved_fingerprint: None,
            },
        );
        let mut settings = OperationHeightSettings::default();
        let resolve = |doc: &Document, settings: &OperationHeightSettings| {
            settings
                .resolve(doc, &doc.setup().operations[0])
                .unwrap()
                .bottom
        };
        assert_eq!(resolve(&doc, &settings), 30.);
        settings.definition_mut(HeightPlaneKind::Bottom).offset = -0.75;
        assert_eq!(resolve(&doc, &settings), 29.25);
        doc.geometry.instances[0].placement.translation[1] = 2.;
        assert_eq!(resolve(&doc, &settings), 31.25);
        // The legacy connected-edge recipe uses the same source plane.
        doc.geometry.bindings.get_mut(&1).unwrap().recipe = ExtractionRecipe::ProfileContours;
        assert_eq!(resolve(&doc, &settings), 31.25);
        let reopened: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(resolve(&reopened, &settings), 31.25);
        std::sync::Arc::make_mut(&mut doc.geometry.assets[0].topology).edges[0].curve =
            CadCurve::Line {
                start: [0., 30., 0.],
                end: [4., 31., 0.],
            };
        assert!(settings
            .resolve(&doc, &doc.setup().operations[0])
            .unwrap_err()
            .contains("one plane"));
        doc.geometry.bindings.get_mut(&1).unwrap().selected[0].revision = 2;
        assert!(settings
            .resolve(&doc, &doc.setup().operations[0])
            .unwrap_err()
            .contains("missing or changed"));
    }
    #[test]
    fn fixture_and_geometry_references_follow_placement_and_reject_stale_identity() {
        let mut doc = doc();
        let body = add_body(&mut doc);
        let mut settings = OperationHeightSettings::default();
        let top = settings.definition_mut(HeightPlaneKind::Top);
        top.reference = HeightReference::FixtureTop;
        top.entities = vec![body.clone()];
        top.offset = 1.;
        assert_eq!(
            settings
                .resolve(&doc, &doc.setup().operations[0])
                .unwrap()
                .top,
            8.
        );
        doc.geometry.instances[0].placement.translation[2] = 1.;
        assert_eq!(
            settings
                .resolve(&doc, &doc.setup().operations[0])
                .unwrap()
                .top,
            9.
        );
        let top = settings.definition_mut(HeightPlaneKind::Top);
        top.reference = HeightReference::Selection;
        top.entities[0].kind = EntityKind::Vertex;
        top.entities[0].id = 2;
        top.offset = 0.;
        assert_eq!(
            settings
                .resolve(&doc, &doc.setup().operations[0])
                .unwrap()
                .top,
            7.
        );
        doc.geometry.instances[0].placement.translation[2] = 0.;
        doc.setup_mut().frame.world_from_setup.rotation =
            [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
        assert_eq!(
            settings
                .resolve(&doc, &doc.setup().operations[0])
                .unwrap()
                .top,
            0.
        );
        settings.definition_mut(HeightPlaneKind::Top).entities[0].revision = 2;
        assert!(settings
            .resolve(&doc, &doc.setup().operations[0])
            .unwrap_err()
            .contains("missing or changed"));
    }
    #[test]
    fn circular_edge_height_limits_are_exact_even_for_unwrapped_curve_parameters() {
        let mut doc = doc();
        let mut edge = add_body(&mut doc);
        edge.kind = EntityKind::Edge;
        edge.id = 3;
        let start = 100.0 * std::f64::consts::TAU + std::f64::consts::PI + 0.1;
        let end = 101.0 * std::f64::consts::TAU + 0.2;
        std::sync::Arc::make_mut(&mut doc.geometry.assets[0].topology)
            .edges
            .push(CadEdge {
                id: 3,
                body_id: 1,
                vertices: [2, 2],
                face_ids: vec![],
                curve: CadCurve::CircleArc {
                    center: [0.; 3],
                    normal: [0., 1., 0.],
                    x_axis: [1., 0., 0.],
                    radius: 4.,
                    start_angle: start,
                    end_angle: end,
                },
            });
        let (_, max) = doc.height_selection_bounds(&[edge]).unwrap();
        assert!((max[2] - 4.).abs() < 1e-10);
    }
}
