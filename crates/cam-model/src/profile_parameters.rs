//! Profile parameters shared by the inspector and the strategy.
use crate::{Comp, Lead, Plunge, ProfileOp, Side};
use serde::{Deserialize, Serialize};

pub const DEFAULT_ARC_FIT_TOLERANCE: f64 = 0.01;
pub const MAX_PROFILE_LEVELS: usize = 100_000;
pub const MAX_PROFILE_MOVES: usize = 5_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileCorner {
    #[default]
    Round,
    Sharp,
}
impl ProfileCorner {
    pub const ALL: [Self; 2] = [Self::Round, Self::Sharp];
    pub fn join(self) -> cam_geo::JoinStyle {
        match self {
            Self::Round => cam_geo::JoinStyle::Round,
            Self::Sharp => cam_geo::JoinStyle::Miter,
        }
    }
}
impl std::fmt::Display for ProfileCorner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Round => "Roll around corner",
            Self::Sharp => "Keep sharp corner",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabShape {
    #[default]
    Rectangular,
    Triangular,
}
impl TabShape {
    pub const ALL: [Self; 2] = [Self::Rectangular, Self::Triangular];
}
impl std::fmt::Display for TabShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rectangular => "Rectangular",
            Self::Triangular => "Triangular",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileOrder {
    #[default]
    Contour,
    Depth,
    Radial,
}
impl ProfileOrder {
    pub const ALL: [Self; 3] = [Self::Contour, Self::Depth, Self::Radial];
}
impl std::fmt::Display for ProfileOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Contour => "By contour",
            Self::Depth => "By depth",
            Self::Radial => "By radial pass",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfilePasses {
    /// An explicit drawing-boundary snapshot; None uses the setup stock.
    pub stock_boundary: Option<cam_geo::Contour>,
    pub outer_corner: ProfileCorner,
    pub finishing: bool,
    pub finish_count: u32,
    pub finish_stepover: f64,
    /// Zero inherits the cutting feed.
    pub finish_feed: f64,
    pub spring_passes: u32,
    pub finish_only_final: bool,
    pub floor_allowance: f64,
    pub even_stepdowns: bool,
    pub finishing_depths: u32,
    pub finishing_stepdown: f64,
    /// Apply the smaller bottom increments to intermediate radial/roughing cuts too.
    pub rough_final: bool,
    pub order: ProfileOrder,
    pub preserve_order: bool,
    pub start_extension: f64,
    pub end_extension: f64,
    pub both_ways: bool,
    /// Arc-fitting tolerance; source projection accuracy remains independent.
    pub tolerance: f64,
    /// Offset slope from the selected contour at the final nominal depth.
    pub taper_angle: f64,
    pub feed_retract: bool,
    pub same_lead_out: bool,
    /// Distance before the contour end at which the exit feed begins.
    pub lead_end_distance: f64,
    /// Zero keeps the cutting RPM during downward entry.
    pub ramp_rpm: f64,
}
impl Default for ProfilePasses {
    fn default() -> Self {
        Self {
            stock_boundary: None,
            outer_corner: ProfileCorner::Round,
            finishing: false,
            finish_count: 1,
            finish_stepover: 0.5,
            finish_feed: 0.,
            spring_passes: 0,
            finish_only_final: false,
            floor_allowance: 0.,
            even_stepdowns: false,
            finishing_depths: 0,
            finishing_stepdown: 0.5,
            rough_final: true,
            order: ProfileOrder::Contour,
            preserve_order: true,
            start_extension: 0.,
            end_extension: 0.,
            both_ways: false,
            tolerance: DEFAULT_ARC_FIT_TOLERANCE,
            taper_angle: 0.,
            feed_retract: false,
            same_lead_out: false,
            lead_end_distance: 0.,
            ramp_rpm: 0.,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProfileCapabilities {
    pub tabs: bool,
    pub roughing: bool,
    pub control: bool,
    pub ramp: bool,
}
impl ProfileOp {
    pub fn validate_control_leads(&self, radius: f64) -> Result<(), String> {
        if self.comp == Comp::Computed {
            return Ok(());
        }
        let valid =
            |lead| matches!(lead,Lead::Linear{length} if length.is_finite() && length>=radius);
        let exit = if self.passes.same_lead_out {
            self.lead_in
        } else {
            self.lead_out
        };
        if !radius.is_finite()
            || radius <= 0.
            || self.contours().any(|c| c.side == Side::On)
            || !valid(self.lead_in)
            || !valid(exit)
        {
            return Err("Control compensation needs Inside/Outside cutter side and linear entry/exit at least one tool radius long".into());
        }
        Ok(())
    }
    pub fn validate_profile_source(&self, linked: bool) -> Result<(), String> {
        if linked
            && (self.comp != Comp::Computed
                || self.offset < 0.
                || self.stepover > 0.
                || self.passes.floor_allowance < 0.
                || self.passes.taper_angle != 0.)
        {
            return Err("Linked CAD profiles require computed compensation, nonnegative wall/floor allowance, no radial roughing and no operator-defined taper".into());
        }
        Ok(())
    }
    pub fn capabilities(&self, linked: bool, controller_compensation: bool) -> ProfileCapabilities {
        let closed = self.closed && self.additional_chains.iter().all(|c| c.closed);
        ProfileCapabilities {
            tabs: closed
                && self.additional_chains.is_empty()
                && self.comp == Comp::Computed
                && self.stepover == 0.
                && self.plunge == Plunge::Straight
                && self.lead_overlap == 0.
                && self.passes.taper_angle == 0.,
            roughing: !linked
                && self.closed
                && self.additional_chains.is_empty()
                && self.side == Side::Outside
                && self.comp == Comp::Computed
                && !self.tabs.enabled
                && self.passes.taper_angle == 0.,
            control: !linked
                && closed
                && controller_compensation
                && !self.tabs.enabled
                && self.stepover == 0.
                && self.passes.taper_angle == 0.,
            ramp: !self.tabs.enabled && self.comp == Comp::Computed,
        }
    }

    /// Independent of GUI callers and checked before allocating motion buffers.
    pub fn validate_profile(&self, top: f64) -> Result<(), String> {
        self.entry_feeds.validate()?;
        if let Some(boundary) = &self.passes.stock_boundary {
            cam_geo::Polygon::new(boundary.clone())
                .map_err(|e| format!("Invalid roughing boundary: {e}"))?;
        }
        for (name, v, positive) in [
            ("depth", self.depth, true),
            ("stepdown", self.stepdown, true),
            ("cutting feed", self.feed, true),
            ("plunge feed", self.plunge_feed, true),
            ("allowance", self.offset, false),
            ("stock top", top, false),
        ] {
            if !v.is_finite() || (positive && v <= 0.) {
                return Err(format!(
                    "Profile {name} must be {}finite",
                    if positive { "positive and " } else { "" }
                ));
            }
        }
        for v in [
            self.stepover,
            self.lead_overlap,
            self.spindle_rpm,
            self.clearing.engagement,
        ] {
            if !v.is_finite() || v < 0. {
                return Err(
                    "Profile spacing, overlap, RPM and engagement must be finite and nonnegative"
                        .into(),
                );
            }
        }
        for contour in self.contours() {
            if contour.chain.len() < if contour.closed { 3 } else { 2 }
                || contour
                    .chain
                    .points()
                    .iter()
                    .any(|p| !p.x.is_finite() || !p.y.is_finite())
            {
                return Err("Select a profile with finite coordinates and enough vertices".into());
            }
        }
        let contours: Vec<_> = self.contours().collect();
        for (index, a) in contours.iter().enumerate() {
            if contours[..index]
                .iter()
                .any(|b| equivalent_profile_contours(a, b))
            {
                return Err(format!("Profile contour {} repeats an earlier selection; remove the duplicate or use explicit spring passes",index+1));
            }
        }
        if self.start.is_some_and(|p| p.iter().any(|v| !v.is_finite())) {
            return Err("Profile start must be finite".into());
        }
        for lead in [self.lead_in, self.lead_out] {
            if matches!(lead, Lead::ArcSweep { angle_deg, .. } if !angle_deg.is_finite() || angle_deg<=0. || angle_deg>170.)
            {
                return Err("Lead sweep must be finite and between 0 and 170 degrees".into());
            }
            let size = match lead {
                Lead::None => continue,
                Lead::Linear { length } => length,
                Lead::Arc { radius } | Lead::ArcSweep { radius, .. } => radius,
            };
            if !size.is_finite() || size <= 0. {
                return Err("Lead size must be positive and finite".into());
            }
        }
        match self.plunge {
            Plunge::Straight => {}
            Plunge::Ramp { angle_deg }
                if angle_deg.is_finite() && angle_deg > 0. && angle_deg < 90. => {}
            Plunge::ZigZag { length, angle_deg }
                if length.is_finite()
                    && length > 0.
                    && angle_deg.is_finite()
                    && angle_deg > 0.
                    && angle_deg < 90. => {}
            Plunge::Helix { radius, pitch }
                if radius.is_finite() && pitch.is_finite() && radius > 0. && pitch > 0. => {}
            _ => {
                return Err(
                    "Enter a finite positive descent size and an angle between 0 and 90 degrees"
                        .into(),
                )
            }
        }
        let p = &self.passes;
        if [
            p.finish_stepover,
            p.finish_feed,
            p.floor_allowance,
            p.finishing_stepdown,
            p.start_extension,
            p.end_extension,
            p.tolerance,
            p.taper_angle,
            p.lead_end_distance,
            p.ramp_rpm,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || p.finish_feed < 0.
            || p.lead_end_distance < 0.
            || p.ramp_rpm < 0.
            || p.start_extension < 0.
            || p.end_extension < 0.
            || p.tolerance <= 0.
            || p.taper_angle.abs() >= 80.
        {
            return Err("Profile pass parameters must be finite; extensions/feed nonnegative, tolerance positive and taper between -80 and 80 degrees".into());
        }
        if p.finishing
            && (p.finish_count == 0
                || p.finish_count > 1000
                || p.spring_passes > 1000
                || (p.finish_count > 1 && p.finish_stepover <= 0.))
        {
            return Err(
                "Finish count must be 1..1000, repeats 0..1000, and radial spacing positive".into(),
            );
        }
        if p.ramp_rpm > 0. && (self.plunge == Plunge::Straight || self.spindle_rpm <= 0.) {
            return Err("Ramp RPM requires ramp/helix/zigzag descent and an explicit cutting RPM to restore".into());
        }
        if p.lead_end_distance > 0. && self.tabs.enabled {
            return Err(
                "Lead-end feed transitions currently require a profile without holding tabs".into(),
            );
        }
        if p.lead_end_distance > 0. && self.stepover > 0. && !p.finishing {
            return Err(
                "Enable Finishing passes to apply a lead-end feed transition after roughing".into(),
            );
        }
        if p.finishing_depths > 1000 || (p.finishing_depths > 0 && p.finishing_stepdown <= 0.) {
            return Err("Finishing depths need a positive step and at most 1000 levels".into());
        }
        let bottom = -self.depth + p.floor_allowance;
        let span = top - bottom;
        let drop = self.stepdown.min(span);
        let entry_steps = match self.plunge {
            Plunge::Helix { pitch, .. } => drop / pitch * 2.,
            Plunge::ZigZag { length, angle_deg } => drop / (length * angle_deg.to_radians().tan()),
            _ => 0.,
        };
        if !entry_steps.is_finite() || entry_steps > MAX_PROFILE_LEVELS as f64 {
            return Err(
                "Descent requires too many entry moves; increase pitch/length or reduce stepdown"
                    .into(),
            );
        }
        let count = (span / self.stepdown).ceil() + p.finishing_depths as f64;
        if !span.is_finite()
            || span <= 0.
            || !count.is_finite()
            || count > MAX_PROFILE_LEVELS as f64
        {
            return Err("Profile depth must reach below the top and require at most 100000 levels; increase stepdown".into());
        }
        if p.finishing_depths as f64 * p.finishing_stepdown >= span {
            return Err("Finishing levels must leave a positive roughing depth".into());
        }
        if top - self.stepdown == top {
            return Err("Stepdown is too small to change the cutting height".into());
        }
        if p.taper_angle > 0. {
            return Err("Positive taper from the bottom reference creates an undercut; cylindrical profile tools only support zero or negative taper".into());
        }
        if p.taper_angle != 0. && self.contours().any(|c| c.side == Side::On) {
            return Err("Wall taper needs Inside/Outside (or left/right) cutter side; On has no material side".into());
        }
        if p.finishing_depths > 0 {
            let rough_bottom = bottom + p.finishing_depths as f64 * p.finishing_stepdown;
            if rough_bottom - p.finishing_stepdown == rough_bottom {
                return Err("Finishing stepdown is too small to change height".into());
            }
        }
        let caps = self.capabilities(false, true);
        if self.stepover > 0.
            && !p.finishing
            && (self.lead_in != Lead::None || self.lead_out != Lead::None)
        {
            return Err(
                "Enable Finishing passes to apply entry/exit leads after radial roughing".into(),
            );
        }
        if self.stepover > 0.
            && !p.finishing
            && (p.outer_corner != ProfileCorner::Round || p.tolerance != DEFAULT_ARC_FIT_TOLERANCE)
        {
            return Err("Shared roughing uses round corners and 0.01 mm arc fitting; enable Finishing passes for independent finish settings".into());
        }
        if self.stepover > 0. && !caps.roughing {
            return Err("Radial roughing requires one closed outside profile, computed compensation, no tabs and no taper".into());
        }
        if self.tabs.enabled && !caps.tabs {
            return Err("Holding tabs require one closed profile, computed compensation, straight plunge, no radial roughing, overlap or taper".into());
        }
        if self.tabs.enabled
            && (p.finishing
                || p.finishing_depths > 0
                || p.order != ProfileOrder::Contour
                || p.even_stepdowns
                || p.floor_allowance != 0.)
        {
            return Err(
                "Holding tabs currently require the standard depth and contour schedule".into(),
            );
        }
        if self.contours().any(|c| !c.closed)
            && (self.tabs.enabled || self.comp != Comp::Computed || self.lead_overlap != 0.)
        {
            return Err(
                "Open profiles require computed compensation and no tabs or closure overlap".into(),
            );
        }
        if self.comp != Comp::Computed
            && (!caps.control
                || !matches!(self.lead_in, Lead::Linear { .. })
                || !matches!(
                    if p.same_lead_out {
                        self.lead_in
                    } else {
                        self.lead_out
                    },
                    Lead::Linear { .. }
                )
                || self.plunge != Plunge::Straight)
        {
            return Err("Control compensation requires a closed untapered profile with linear entry and exit, straight plunge and no tabs or roughing".into());
        }
        Ok(())
    }
}

/// Geometric identity independent of a closed chain's seam or winding.
pub fn equivalent_profile_contours(a: &crate::ProfileContour, b: &crate::ProfileContour) -> bool {
    if a.closed != b.closed || a.side != b.side || a.chain.len() != b.chain.len() {
        return false;
    }
    let closed = a.closed;
    let a = a.chain.points();
    let b = b.chain.points();
    let n = a.len();
    if n == 0 {
        return true;
    }
    let near = |i: usize, j: usize| a[i].distance(b[j]) < 1e-8;
    if !closed {
        return (0..n).all(|i| near(i, i));
    }
    (0..n).any(|start| {
        (0..n).all(|i| near(i, (start + i) % n)) || (0..n).all(|i| near(i, (start + n - i) % n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_control_names_load_as_one_method_and_old_options_keep_defaults() {
        for name in ["ControlLeft", "ControlRight", "Control"] {
            let comp: Comp = serde_json::from_str(&format!("\"{name}\"")).unwrap();
            assert_eq!(comp, Comp::Control);
            assert_eq!(serde_json::to_string(&comp).unwrap(), "\"Control\"");
        }
        assert_eq!(
            serde_json::from_str::<ProfilePasses>("{}").unwrap(),
            ProfilePasses::default()
        );
        assert_eq!(
            serde_json::from_str::<crate::ProfileTabs>("{}").unwrap(),
            crate::ProfileTabs::default()
        );
    }
}
