//! Named, independently configured geometry collections for a profile operation.
use crate::{EntityRef, ProfileContour, Side};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileGeometryKind {
    FaceContours,
    Chain,
    Silhouette,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileCurves {
    #[default]
    All,
    Outer,
    Inner,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileSideRule {
    #[default]
    StartOutside,
    StartInside,
    AlwaysOutside,
    AlwaysInside,
    On,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileChainMode {
    #[default]
    Auto,
    Closed,
    Open,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProfileGeometrySource {
    Cad(Vec<EntityRef>),
    Contours(Vec<ProfileContour>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileGeometryEntry {
    pub name: String,
    pub kind: ProfileGeometryKind,
    pub source: ProfileGeometrySource,
    pub coplanar: bool,
    pub curves: ProfileCurves,
    pub side_rule: ProfileSideRule,
    pub chain_mode: ProfileChainMode,
    pub reversed: bool,
    pub use_setup_models: bool,
    /// Maximum permitted deflection of the independent silhouette projection.
    pub tolerance: f64,
    #[serde(default)]
    pub side_overrides: BTreeMap<usize, Side>,
    /// Resolved contour count for mapping per-entry travel direction into the cache.
    #[serde(default)]
    pub contour_count: usize,
}
impl ProfileGeometryEntry {
    pub fn new(kind: ProfileGeometryKind, name: String) -> Self {
        Self {
            name,
            kind,
            source: ProfileGeometrySource::Cad(Vec::new()),
            coplanar: false,
            curves: ProfileCurves::All,
            side_rule: ProfileSideRule::StartOutside,
            chain_mode: ProfileChainMode::Auto,
            reversed: false,
            use_setup_models: kind == ProfileGeometryKind::Silhouette,
            tolerance: 0.010,
            side_overrides: BTreeMap::new(),
            contour_count: 0,
        }
    }
}

impl std::fmt::Display for ProfileSideRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StartOutside => "Start outside",
            Self::StartInside => "Start inside",
            Self::AlwaysOutside => "Always outside",
            Self::AlwaysInside => "Always inside",
            Self::On => "On contour",
        })
    }
}
