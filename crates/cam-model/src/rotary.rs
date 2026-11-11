//! Cylindrical wrapping of planar contour and pocket toolpaths.
use crate::{EntityRef, Operation};
pub use cam_cldata::RotaryAxis;

/// Fixed workpiece index for ordinary three-axis machining. Operation geometry
/// and heights are in the machine frame at this angle; CAD placements are rotated
/// into that frame before extracting the operation's selected geometry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RotaryIndex {
    pub axis: RotaryAxis,
    pub center: [f64; 3],
    pub angle: f64,
    pub tolerance: f64,
    /// Optional planar face, closed loop, or straight edge used to choose the index angle.
    pub face: Option<EntityRef>,
    pub flip: bool,
}
impl Default for RotaryIndex {
    fn default() -> Self {
        Self {
            axis: RotaryAxis::A,
            center: [0.; 3],
            angle: 0.,
            tolerance: 0.01,
            face: None,
            flip: false,
        }
    }
}
impl RotaryIndex {
    pub fn validate(&self) -> Result<(), String> {
        if !self
            .center
            .into_iter()
            .chain([self.angle, self.tolerance])
            .all(f64::is_finite)
            || self.tolerance <= 0.
            || self.tolerance > 1.
        {
            return Err(
                "Index centre and angle must be finite, with tolerance in (0, 1] mm".into(),
            );
        }
        Ok(())
    }
    /// Input geometry must already be expressed in setup axes.
    pub fn geometry(&self, source: &crate::GeometryState) -> crate::GeometryState {
        let mut out = source.clone();
        let center = cam_cldata::Point3::new(self.center[0], self.center[1], self.center[2]);
        let point = |p: [f64; 3]| {
            let p = self.axis.material_point(
                cam_cldata::Point3::new(p[0], p[1], p[2]),
                center,
                -self.angle,
            );
            [p.x, p.y, p.z]
        };
        let vector = |p: [f64; 3]| {
            let p = self.axis.material_point(
                cam_cldata::Point3::new(p[0], p[1], p[2]),
                cam_cldata::Point3::new(0., 0., 0.),
                -self.angle,
            );
            [p.x, p.y, p.z]
        };
        for instance in &mut out.instances {
            instance.placement.translation = point(instance.placement.translation);
            let old = instance.placement.rotation;
            let columns: [[f64; 3]; 3] =
                std::array::from_fn(|j| vector([old[0][j], old[1][j], old[2][j]]));
            instance.placement.rotation =
                std::array::from_fn(|i| std::array::from_fn(|j| columns[j][i]));
        }
        out
    }
}

impl crate::Document {
    /// A temporary view for resolving indexed CAD selections and height references.
    pub fn indexed_view(&self, id: u32) -> Result<Option<Self>, String> {
        let Some(index) = self.rotary_indexes.get(&id) else {
            return Ok(None);
        };
        index.validate()?;
        let setup = self.setup();
        let geometry = self.geometry.geometry_in_setup(&setup.frame)?;
        let mut view = self.clone();
        view.geometry = index.geometry(&geometry);
        view.setup_mut().frame = Default::default();
        view.rotary_indexes.remove(&id);
        let center = cam_cldata::Point3::new(index.center[0], index.center[1], index.center[2]);
        let rotate = |p: [f64; 3]| {
            let p = index.axis.material_point(
                cam_cldata::Point3::new(p[0], p[1], p[2]),
                center,
                -index.angle,
            );
            [p.x, p.y, p.z]
        };
        let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        if let Some((frame, a, b)) = self.round_stock_geometry(setup)? {
            let c = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, 0.];
            let p = rotate(setup.frame.to_setup_vector(frame.to_world_vector(c)));
            let d = setup
                .frame
                .to_setup_vector(frame.to_world_vector([0., 0., 1.]));
            let end = rotate(std::array::from_fn(|i| {
                setup.frame.to_setup_vector(frame.to_world_vector(c))[i] + d[i]
            }));
            let d = std::array::from_fn::<_, 3, _>(|i| end[i] - p[i]);
            let radius = (b[0] - a[0]) * 0.5;
            for i in 0..3 {
                let r = radius * (1. - d[i] * d[i]).max(0.).sqrt();
                min[i] = (p[i] + a[2] * d[i]).min(p[i] + b[2] * d[i]) - r;
                max[i] = (p[i] + a[2] * d[i]).max(p[i] + b[2] * d[i]) + r;
            }
        } else {
            let (a, b) = self.stock_bounds_for(setup);
            for bits in 0..8 {
                let p = rotate(std::array::from_fn(|i| {
                    if bits & (1 << i) == 0 {
                        a[i]
                    } else {
                        b[i]
                    }
                }));
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
        }
        let (a, b) = view.height_model_bounds();
        view.setup_mut().stock = crate::Stock::RelativeBox {
            offsets: [
                std::array::from_fn(|i| a[i] - min[i]),
                std::array::from_fn(|i| max[i] - b[i]),
            ],
        };
        Ok(Some(view))
    }
}

/// Drawing coordinates remain millimetres: A wraps Y around X; B wraps X
/// around Y. Z is measured from `surface_z`, the unwrapped cylinder surface.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RotaryWrap {
    /// Worker-derived: selected CAD contours are complete periodic cylinder bands.
    #[serde(skip)]
    pub periodic: bool,
    #[serde(default = "default_unwrap_geometry")]
    pub unwrap_geometry: bool,
    pub axis: RotaryAxis,
    pub radius: f64,
    /// Radial offset of the wrapping cylinder, mm.
    pub offset: f64,
    pub center: [f64; 3],
    pub surface_z: f64,
    pub seam_angle: f64,
    pub tolerance: f64,
    /// Optional associative CAD cylinder and machining boundary selections.
    pub cylinder: Option<EntityRef>,
    pub geometry: Vec<EntityRef>,
    /// Derived radial height of a cylindrical CAD boundary, refreshed on resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary_z: Option<f64>,
}
impl Default for RotaryWrap {
    fn default() -> Self {
        Self {
            periodic: false,
            unwrap_geometry: true,
            axis: RotaryAxis::A,
            radius: 25.,
            offset: 0.,
            center: [0.; 3],
            surface_z: 0.,
            seam_angle: 0.,
            tolerance: 0.01,
            cylinder: None,
            geometry: Vec::new(),
            boundary_z: None,
        }
    }
}
impl RotaryWrap {
    /// Developed radial height of the input blank. A coaxial round blank is
    /// exact; other blanks use their conservative enclosing radial extent.
    pub fn stock_top(&self, document: &crate::Document) -> Result<f64, String> {
        let setup = document.setup();
        if let Some((frame, min, max)) = document.round_stock_geometry(setup)? {
            let axis = setup
                .frame
                .to_setup_vector(frame.to_world_vector([0., 0., 1.]));
            let center = setup.frame.to_setup_vector(frame.to_world_vector([
                (min[0] + max[0]) * 0.5,
                (min[1] + max[1]) * 0.5,
                0.,
            ]));
            let (aligned, offset) = match self.axis {
                RotaryAxis::A => (
                    axis[0].abs() > 1. - 1e-8,
                    (center[1] - self.center[1]).hypot(center[2] - self.center[2]),
                ),
                RotaryAxis::B => (
                    axis[1].abs() > 1. - 1e-8,
                    (center[0] - self.center[0]).hypot(center[2] - self.center[2]),
                ),
            };
            if aligned && offset < 1e-6 {
                return Ok(self.surface_z + (max[0] - min[0]) * 0.5 - self.effective_radius());
            }
        }
        let (min, max) = document.stock_bounds_for(setup);
        let radius = (0..8)
            .map(|bits| {
                let p = std::array::from_fn::<_, 3, _>(|i| {
                    if bits & (1 << i) == 0 {
                        min[i]
                    } else {
                        max[i]
                    }
                });
                match self.axis {
                    RotaryAxis::A => (p[1] - self.center[1]).hypot(p[2] - self.center[2]),
                    RotaryAxis::B => (p[0] - self.center[0]).hypot(p[2] - self.center[2]),
                }
            })
            .fold(0_f64, f64::max);
        Ok(self.surface_z + radius - self.effective_radius())
    }
    pub fn effective_radius(&self) -> f64 {
        self.radius + self.offset
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.radius.is_finite()
            || self.radius <= 0.
            || !self.offset.is_finite()
            || self.effective_radius() <= 0.
            || !self.effective_radius().is_finite()
        {
            return Err("Wrap radius and radius plus offset must be finite and positive".into());
        }
        if self.center.iter().any(|v| !v.is_finite())
            || !self.surface_z.is_finite()
            || !self.seam_angle.is_finite()
        {
            return Err("Wrap centre, surface Z and seam angle must be finite".into());
        }
        if !self.tolerance.is_finite() || self.tolerance <= 0. || self.tolerance > 1. {
            return Err("Wrap tolerance must be greater than 0 and at most 1 mm".into());
        }
        Ok(())
    }
    /// Machine position and continuous rotary angle (never reduced modulo 360).
    pub fn position(&self, p: cam_cldata::Point3) -> (cam_cldata::Point3, f64) {
        let r = self.effective_radius();
        let z = self.center[2] + r + p.z - self.surface_z;
        match self.axis {
            RotaryAxis::A => (
                cam_cldata::Point3::new(p.x, self.center[1], z),
                self.seam_angle + (p.y / r).to_degrees(),
            ),
            RotaryAxis::B => (
                cam_cldata::Point3::new(self.center[0], p.y, z),
                self.seam_angle - (p.x / r).to_degrees(),
            ),
        }
    }
}
impl Operation {
    pub fn rotary_wrap(&self) -> Option<&RotaryWrap> {
        match self {
            Self::Profile(op) => op.wrap.as_ref(),
            Self::Pocket(op) => op.wrap.as_ref(),
            _ => None,
        }
    }
    pub fn set_rotary_wrap(&mut self, wrap: Option<RotaryWrap>) {
        match self {
            Self::Profile(op) => op.wrap = wrap,
            Self::Pocket(op) => op.wrap = wrap,
            _ => {}
        }
    }
    pub fn supports_rotary_wrap(&self) -> bool {
        matches!(self, Self::Profile(_) | Self::Pocket(_))
    }
}

fn default_unwrap_geometry() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_wraps_unwrap_geometry_and_the_choice_roundtrips() {
        let old = serde_json::json!({"radius":100.});
        let mut wrap: RotaryWrap = serde_json::from_value(old).unwrap();
        assert!(wrap.unwrap_geometry);
        wrap.unwrap_geometry = false;
        let saved = serde_json::to_string(&wrap).unwrap();
        assert!(
            !serde_json::from_str::<RotaryWrap>(&saved)
                .unwrap()
                .unwrap_geometry
        );
    }
}
