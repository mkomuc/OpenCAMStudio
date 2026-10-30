//! Optional radial finishing for a pocket's outer walls and islands.
use serde::{Deserialize, Serialize};

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
