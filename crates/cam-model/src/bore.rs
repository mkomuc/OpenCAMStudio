//! Helical milling of equal-sized circular holes or bosses.
use crate::{Cooling, EntryFeeds, ToolPreset};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BoreOrder {
    #[default]
    Selected,
    Optimized,
    InsideOut,
    X,
    Y,
}
impl BoreOrder {
    pub const ALL: [Self; 5] = [
        Self::Selected,
        Self::Optimized,
        Self::InsideOut,
        Self::X,
        Self::Y,
    ];
}
impl std::fmt::Display for BoreOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Selected => "Selected order",
            Self::Optimized => "Optimized order",
            Self::InsideOut => "Inside to outside",
            Self::X => "Order by X",
            Self::Y => "Order by Y",
        })
    }
}

/// Dimensions are in setup coordinates. Diameter is measured at `z_top`;
/// positive taper makes the feature narrower toward the bottom.
/// Compensation is computed in CAM. The operation assumes an accessible hole
/// or boss; it is a wall milling strategy, not a solid-stock pocket clearing cycle.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BoreOp {
    pub id: u32,
    pub tool: u32,
    pub points: Vec<[f64; 2]>,
    pub internal: bool,
    pub diameter: f64,
    pub z_top: f64,
    pub z_bottom: f64,
    pub taper_angle: f64,
    /// Axial advance per revolution, used unless `ramp_angle` is set.
    pub pitch: f64,
    /// Maximum helix angle in degrees, measured at the smallest tool orbit.
    pub ramp_angle: Option<f64>,
    pub tolerance: f64,
    pub climb: bool,
    /// Additional radial roughing passes preceding the final wall pass.
    pub stepovers: u32,
    pub stepover: f64,
    /// Optional finishing allowance, removed by one additional full-depth helix.
    pub finishing_stepover: f64,
    pub repeat_pass: bool,
    pub radial_stock: f64,
    pub axial_stock: f64,
    pub safe_distance: f64,
    /// Tangential semicircular leads run to/from the hole centre when enabled.
    pub lead_to_center: bool,
    pub lead_radius: f64,
    pub order: BoreOrder,
    pub reverse_order: bool,
    pub spindle_rpm: f64,
    pub tool_preset: Option<ToolPreset>,
    pub cooling: Cooling,
    pub entry_feeds: EntryFeeds,
    pub work_offset: u32,
    pub feed: f64,
    pub plunge_feed: f64,
}

impl Default for BoreOp {
    fn default() -> Self {
        Self {
            id: 0,
            tool: 1,
            points: Vec::new(),
            internal: true,
            diameter: 12.,
            z_top: 0.,
            z_bottom: -4.,
            taper_angle: 0.,
            pitch: 1.,
            ramp_angle: None,
            tolerance: 0.01,
            climb: true,
            stepovers: 0,
            stepover: 0.5,
            finishing_stepover: 0.,
            repeat_pass: false,
            radial_stock: 0.,
            axial_stock: 0.,
            safe_distance: 1.,
            lead_to_center: true,
            lead_radius: 0.5,
            order: BoreOrder::Selected,
            reverse_order: false,
            spindle_rpm: 0.,
            tool_preset: None,
            cooling: Cooling::default(),
            entry_feeds: EntryFeeds::default(),
            work_offset: 1,
            feed: 300.,
            plunge_feed: 100.,
        }
    }
}
