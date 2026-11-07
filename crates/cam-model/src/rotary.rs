//! Cylindrical wrapping of planar contour and pocket toolpaths.
use crate::{EntityRef, Operation};
pub use cam_cldata::RotaryAxis;

/// Drawing coordinates remain millimetres: A wraps Y around X; B wraps X
/// around Y. Z is measured from `surface_z`, the unwrapped cylinder surface.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RotaryWrap {
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
