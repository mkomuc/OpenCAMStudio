//! Setup fixtures are separate from the workpiece and never contribute to stock.
use crate::{GeometryState, Setup};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FixtureClearance {
    /// Distance from the fixture in setup XY, in millimetres.
    pub radial: f64,
    /// Distance from the fixture along setup Z, in millimetres.
    pub axial: f64,
}
impl Default for FixtureClearance {
    fn default() -> Self {
        Self {
            radial: 5.0,
            axial: 5.0,
        }
    }
}

/// Conservative solid keep-out envelope in setup axes, including clearance.
#[derive(Clone, Debug, PartialEq)]
pub struct FixtureEnvelope {
    pub instance_id: u64,
    pub name: String,
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Setup {
    pub fn validate_fixtures(&self, geometry: &GeometryState) -> Result<(), String> {
        let c = self.fixture_clearance;
        if [c.radial, c.axial]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("Fixture clearances must be finite and nonnegative.".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.fixture_instances {
            if !seen.insert(*id) {
                return Err("A fixture body is selected more than once.".into());
            }
            if self.model_instances.contains(id) {
                return Err("A body cannot be both a machining model and a fixture.".into());
            }
            let instance = geometry.instance(*id).ok_or_else(|| {
                format!("Fixture body {id} no longer exists; reselect the fixture.")
            })?;
            let body = geometry
                .asset(instance.asset_id)
                .and_then(|asset| {
                    asset
                        .topology
                        .bodies
                        .iter()
                        .find(|body| body.id == instance.body_id)
                })
                .ok_or_else(|| {
                    format!(
                        "Fixture {} has missing source geometry; reselect it.",
                        instance.name
                    )
                })?;
            if !instance.placement.is_rigid() {
                return Err("Fixture placement must be a finite proper rigid transform.".into());
            }
            if !body.valid_solid
                || (0..3).any(|i| {
                    !body.bounds[0][i].is_finite()
                        || !body.bounds[1][i].is_finite()
                        || body.bounds[0][i] > body.bounds[1][i]
                })
            {
                return Err(format!(
                    "Fixture {} must have valid solid bounds.",
                    instance.name
                ));
            }
        }
        Ok(())
    }

    /// `geometry` must already be rotated into setup axes, just like strategy input.
    /// Source body boxes are transformed at their eight corners. This deliberately
    /// protects voids/concavities too; display tessellation cannot weaken protection.
    pub fn fixture_envelopes(
        &self,
        geometry: &GeometryState,
    ) -> Result<Vec<FixtureEnvelope>, String> {
        self.validate_fixtures(geometry)?;
        self.fixture_instances
            .iter()
            .map(|id| {
                let instance = geometry.instance(*id).unwrap();
                let asset = geometry.asset(instance.asset_id).unwrap();
                let body = asset
                    .topology
                    .bodies
                    .iter()
                    .find(|b| b.id == instance.body_id)
                    .unwrap();
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for x in [body.bounds[0][0], body.bounds[1][0]] {
                    for y in [body.bounds[0][1], body.bounds[1][1]] {
                        for z in [body.bounds[0][2], body.bounds[1][2]] {
                            let p = instance.placement.transform_point([x, y, z]);
                            for i in 0..3 {
                                min[i] = min[i].min(p[i]);
                                max[i] = max[i].max(p[i]);
                            }
                        }
                    }
                }
                for i in 0..3 {
                    let margin = if i == 2 {
                        self.fixture_clearance.axial
                    } else {
                        self.fixture_clearance.radial
                    };
                    min[i] -= margin;
                    max[i] += margin;
                }
                if min.iter().chain(&max).any(|v| !v.is_finite()) {
                    return Err("Fixture clearance envelope exceeds finite coordinates.".into());
                }
                Ok(FixtureEnvelope {
                    instance_id: *id,
                    name: instance.name.clone(),
                    min,
                    max,
                })
            })
            .collect()
    }
}
