//! Material-linked cutting data. Presets seed concrete operation values; planning
//! never consults mutable shop data, so saving/reopening cannot change a cut.
use crate::{Operation, Tool};

/// A shared stock material. Physical properties can be added without changing IDs.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub id: u64,
    pub name: String,
}

/// Named cutting parameters for one cutter. IDs are local to the cutter.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolPreset {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub material_id: Option<u64>,
    /// Zero inherits the job spindle speed, as on an operation.
    pub spindle_rpm: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    /// Optional axial depth per pass, mm; only applied where supported.
    #[serde(default)]
    pub stepdown: Option<f64>,
    /// Optional radial overlap, fraction [0, 0.99]; only applied where supported.
    #[serde(default)]
    pub overlap: Option<f64>,
}

impl ToolPreset {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0 || self.name.trim().is_empty() {
            return Err("A preset needs an ID and a name.".into());
        }
        if !self.spindle_rpm.is_finite()
            || self.spindle_rpm < 0.
            || !self.feed.is_finite()
            || self.feed <= 0.
            || !self.plunge_feed.is_finite()
            || self.plunge_feed <= 0.
            || self.stepdown.is_some_and(|v| !v.is_finite() || v <= 0.)
            || self
                .overlap
                .is_some_and(|v| !v.is_finite() || !(0. ..=0.99).contains(&v))
        {
            return Err(
                "Preset feeds and stepdown must be positive; RPM nonnegative; overlap between 0% and 99%."
                    .into(),
            );
        }
        Ok(())
    }
}

impl Tool {
    pub fn validate_presets(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        for preset in &self.presets {
            preset.validate()?;
            if !ids.insert(preset.id) {
                return Err("Preset IDs must be unique within a cutter".into());
            }
        }
        Ok(())
    }

    /// The first material-specific preset wins ties, followed by an unlinked
    /// preset. A preset linked to a different material is never auto-selected.
    pub fn preset_for_material(&self, material: Option<u64>) -> Option<&ToolPreset> {
        material
            .and_then(|id| self.presets.iter().find(|p| p.material_id == Some(id)))
            .or_else(|| self.presets.iter().find(|p| p.material_id.is_none()))
    }

    pub fn next_preset_id(&mut self) -> u64 {
        self.preset_id_floor = self
            .preset_id_floor
            .max(self.presets.iter().map(|p| p.id).max().unwrap_or(0))
            + 1;
        self.preset_id_floor
    }
}

impl Operation {
    pub fn tool_preset(&self) -> Option<&ToolPreset> {
        match self {
            Self::Profile(o) => o.tool_preset.as_ref(),
            Self::Pocket(o) => o.tool_preset.as_ref(),
            Self::Face(o) => o.tool_preset.as_ref(),
            Self::Drill(o) => o.tool_preset.as_ref(),
            Self::Chamfer(o) => o.tool_preset.as_ref(),
            Self::Thread(o) => o.tool_preset.as_ref(),
            Self::Engrave(o) => o.tool_preset.as_ref(),
            Self::Carve(o) => o.tool_preset.as_ref(),
        }
    }

    /// Clear the association while retaining all current operation values.
    pub fn clear_tool_preset(&mut self) {
        let slot = match self {
            Self::Profile(o) => &mut o.tool_preset,
            Self::Pocket(o) => &mut o.tool_preset,
            Self::Face(o) => &mut o.tool_preset,
            Self::Drill(o) => &mut o.tool_preset,
            Self::Chamfer(o) => &mut o.tool_preset,
            Self::Thread(o) => &mut o.tool_preset,
            Self::Engrave(o) => &mut o.tool_preset,
            Self::Carve(o) => &mut o.tool_preset,
        };
        *slot = None;
    }

    pub fn apply_tool_preset(&mut self, preset: &ToolPreset) -> Result<(), String> {
        preset.validate()?;
        self.set_spindle_rpm(preset.spindle_rpm);
        let (feed, plunge, stepdown, overlap, slot) = match self {
            Self::Profile(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                Some(&mut o.stepdown),
                None,
                &mut o.tool_preset,
            ),
            Self::Pocket(o) => (
                &mut o.clear.feed,
                Some(&mut o.clear.plunge_feed),
                Some(&mut o.clear.stepdown),
                Some(&mut o.clear.overlap),
                &mut o.tool_preset,
            ),
            Self::Face(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                Some(&mut o.stepdown),
                Some(&mut o.overlap),
                &mut o.tool_preset,
            ),
            // A drilling feed is axial: use the preset's plunge feed.
            Self::Drill(o) => {
                if let Some(step) = preset.stepdown {
                    o.peck = Some(step);
                }
                (&mut o.feed, None, None, None, &mut o.tool_preset)
            }
            Self::Chamfer(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                None, // Chamfer steps measure width, not axial stepdown.
                None,
                &mut o.tool_preset,
            ),
            Self::Thread(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                None,
                None,
                &mut o.tool_preset,
            ),
            Self::Engrave(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                Some(&mut o.stepdown),
                None,
                &mut o.tool_preset,
            ),
            Self::Carve(o) => (
                &mut o.feed,
                Some(&mut o.plunge_feed),
                None,
                None,
                &mut o.tool_preset,
            ),
        };
        *feed = if plunge.is_none() {
            preset.plunge_feed
        } else {
            preset.feed
        };
        if let Some(value) = plunge {
            *value = preset.plunge_feed;
        }
        if let (Some(value), Some(v)) = (stepdown, preset.stepdown) {
            *value = v;
        }
        if let (Some(value), Some(v)) = (overlap, preset.overlap) {
            *value = v;
        }
        *slot = Some(preset.clone());
        Ok(())
    }
}
