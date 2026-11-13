//! Optional radial finishing for a pocket's outer walls and islands.
use serde::{Deserialize, Serialize};

/// Optional stock footprint for 2D pocket/adaptive clearing. An empty selection
/// uses the setup stock box. These contours describe material, not containment
/// of the cutter centre: entries and edge runout may lie outside them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockContours {
    pub enabled: bool,
    pub regions: Vec<cam_geo::Polygon>,
    /// CAD sources are re-resolved when planning; drawing contours are snapshots.
    pub source: Option<crate::GeometryBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PocketFinishing {
    pub enabled: bool,
    /// Radial cuts including the final wall, after roughing.
    pub count: u32,
    /// Material reserved per finishing cut, mm. Independent of roughing overlap.
    pub stepover: f64,
    pub lead_in_out_all: bool,
    /// Zero inherits the pocket's cutting feed.
    pub feed: f64,
    /// One extra cut along the final wall to remove tool-deflection marks.
    pub repeat_pass: bool,
    /// Run all finishing cuts only at the final depth instead of at each level.
    pub finish_only_final: bool,
    /// Distance cut beyond each loop's closure, mm.
    pub overlap: f64,
}

impl Default for PocketFinishing {
    fn default() -> Self {
        Self {
            enabled: false,
            count: 1,
            stepover: 0.6,
            lead_in_out_all: false,
            feed: 0.,
            repeat_pass: false,
            finish_only_final: false,
            overlap: 0.,
        }
    }
}

impl PocketFinishing {
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.count == 0 || self.count > 1000 {
            return Err("Pocket finishing needs between 1 and 1000 passes".into());
        }
        if !self.stepover.is_finite() || self.stepover <= 0. {
            return Err("Pocket finishing stepover must be positive and finite".into());
        }
        if !self.feed.is_finite()
            || self.feed < 0.
            || !self.overlap.is_finite()
            || self.overlap < 0.
        {
            return Err("Pocket finishing feed and overlap must be finite and nonnegative".into());
        }
        Ok(())
    }
}

/// Axial scheduling for pocket roughing.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MultipleDepths {
    pub enabled: bool,
    pub even_stepdowns: bool,
    pub order_by_depth: bool,
    pub order_by_area: bool,
}
impl Default for MultipleDepths {
    fn default() -> Self {
        Self {
            enabled: true,
            even_stepdowns: false,
            order_by_depth: false,
            order_by_area: false,
        }
    }
}
