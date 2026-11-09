//! Analytic cylindrical blanks and general dexel subtraction. Material stays in
//! one stationary frame throughout indexing; no resampling between angles.
use super::{Heightfield, RemovedSpan};

/// A cylinder (or tube) whose axis is X, Y or Z in the material frame. The
/// backing dexel solid retains multiple intervals, including internal cavities.
#[derive(Clone, Debug)]
pub struct CylindricalStock {
    pub field: Heightfield,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
impl CylindricalStock {
    /// `axial` contains absolute end coordinates along `axis` (0=X, 1=Y, 2=Z).
    pub fn new(
        axis: usize,
        center: [f64; 3],
        axial: [f64; 2],
        outer_radius: f64,
        inner_radius: f64,
        resolution: f64,
    ) -> Result<Self, String> {
        Self::new_controlled(
            axis,
            center,
            axial,
            outer_radius,
            inner_radius,
            resolution,
            &|| false,
        )
    }
    pub fn new_controlled(
        axis: usize,
        center: [f64; 3],
        axial: [f64; 2],
        outer_radius: f64,
        inner_radius: f64,
        resolution: f64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        if cancelled() {
            return Err("Cylinder stock calculation cancelled".into());
        }
        if axis > 2
            || !center.into_iter().chain(axial).all(f64::is_finite)
            || !outer_radius.is_finite()
            || !inner_radius.is_finite()
            || outer_radius <= 0.
            || inner_radius < 0.
            || inner_radius >= outer_radius
            || axial[0] >= axial[1]
            || !resolution.is_finite()
            || resolution <= 0.
        {
            return Err("Cylinder requires finite dimensions, ordered ends, positive resolution, and 0 <= inner radius < outer radius".into());
        }
        let mut min = center.map(|v| v - outer_radius);
        let mut max = center.map(|v| v + outer_radius);
        min[axis] = axial[0];
        max[axis] = axial[1];
        let cells =
            ((max[0] - min[0]) / resolution).ceil() * ((max[1] - min[1]) / resolution).ceil();
        if cells > 4_000_000. {
            return Err(
                "Cylinder exceeds 4,000,000 stock columns; lower simulation accuracy".into(),
            );
        }
        let mut field = Heightfield::new([min[0], min[1]], [max[0], max[1]], resolution, max[2]);
        if axis != 2 {
            field.horizontal_cylinder = Some(super::HorizontalCylinder {
                axis,
                center,
                outer_radius,
                inner_radius,
            });
        }
        for y in 0..field.ny {
            if cancelled() {
                return Err("Cylinder stock calculation cancelled".into());
            }
            for x in 0..field.nx {
                let (cx, cy) = field.center(x, y);
                let index = y * field.nx + x;
                if axis == 2 {
                    let d = (cx - center[0]).hypot(cy - center[1]);
                    if d > outer_radius || d < inner_radius {
                        field.z[index] = min[2] as f32;
                    }
                } else {
                    let d = if axis == 0 {
                        cy - center[1]
                    } else {
                        cx - center[0]
                    };
                    if d.abs() >= outer_radius {
                        field.z[index] = min[2] as f32;
                        continue;
                    }
                    let h = (outer_radius * outer_radius - d * d).sqrt();
                    field.z[index] = (center[2] + h) as f32;
                    field.subtract_interval(index, min[2], center[2] - h, None, u32::MAX);
                    if d.abs() < inner_radius {
                        let h = (inner_radius * inner_radius - d * d).sqrt();
                        field.subtract_interval(
                            index,
                            center[2] - h,
                            center[2] + h,
                            None,
                            u32::MAX,
                        );
                    }
                }
            }
        }
        field.uncut_z = field.z.clone();
        Ok(Self { field, min, max })
    }
}

impl Heightfield {
    /// Whether a finite interval overlaps material rather than an inherited void.
    pub(crate) fn interval_contact(
        &self,
        index: usize,
        lower: f64,
        upper: f64,
        floor: f64,
    ) -> bool {
        self.interval_material(index, lower, upper, floor, 0.001)
    }

    /// Collision tolerance must not suppress removal of thin residual films.
    pub(crate) fn interval_material(
        &self,
        index: usize,
        lower: f64,
        upper: f64,
        floor: f64,
        tolerance: f64,
    ) -> bool {
        let mut z = lower.max(floor);
        let top = upper.min(f64::from(self.z[index]));
        if top <= z + tolerance {
            return false;
        }
        if let Some(voids) = self.bounded.get(&index) {
            for span in voids {
                if f64::from(span.upper) <= z {
                    continue;
                }
                if f64::from(span.lower) > z + tolerance {
                    return true;
                }
                z = z.max(f64::from(span.upper));
                if z >= top - tolerance {
                    return false;
                }
            }
        }
        z < top - tolerance
    }

    pub(crate) fn column_top(&self, index: usize) -> f64 {
        f64::from(self.z[index])
    }

    /// Subtract and union a finite interval, retaining stock above and below it.
    pub(crate) fn subtract_interval(
        &mut self,
        index: usize,
        lower: f64,
        upper: f64,
        operation: Option<u32>,
        source: u32,
    ) {
        let upper = upper.min(f64::from(self.z[index])) as f32;
        let lower = lower as f32;
        if upper <= lower + 1e-6 {
            return;
        }
        let removal = self.removal_interval(index, f64::from(lower), f64::from(upper));
        let spans = self.bounded.get_or_default(index);
        if let Some(query) = removal {
            // Account and update through the same column lookup.
            self.removal_tracking.as_mut().unwrap().1 += query.volume(spans);
        }
        spans.push(RemovedSpan {
            lower,
            upper,
            lower_op: operation,
            upper_op: operation,
            lower_source: source,
            upper_source: source,
        });
        spans.sort_by(|a, b| a.lower.total_cmp(&b.lower));
        let mut count = 0;
        for i in 0..spans.len() {
            let span = spans[i];
            if count > 0 && span.lower <= spans[count - 1].upper + 1e-6 {
                let previous = &mut spans[count - 1];
                if span.upper > previous.upper {
                    previous.upper = span.upper;
                    previous.upper_op = span.upper_op;
                    previous.upper_source = span.upper_source;
                }
            } else {
                spans[count] = span;
                count += 1;
            }
        }
        spans.truncate(count);
        let (x, y) = (index % self.nx, index / self.nx);
        self.invalidate_mesh_region([x, y], [x, y]);
    }

    pub(crate) fn rotary_source(
        &mut self,
        point: [f64; 3],
        profile: crate::ToolProfile,
        index: usize,
        fraction: f64,
    ) -> u32 {
        if !self.surface_tracking {
            return u32::MAX;
        }
        let mut cut = crate::sweep::Sweep::new(point, point, profile);
        cut.source = Some(super::CutSource {
            step_index: index,
            range: (fraction, fraction),
        });
        let id = self.cuts.len() as u32;
        self.cuts.push(cut);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horizontal_cylinder_mesh_has_continuous_outward_side_normals() {
        for axis in [0, 1] {
            let stock = CylindricalStock::new(axis, [0.; 3], [-2., 2.], 5., 0., 0.2).unwrap();
            let mesh = stock.field.to_solid_mesh(stock.min[2]);
            let transverse = 1 - axis;
            let mut bad = 0;
            let mut checked = 0;
            for (positions, normals) in mesh
                .positions
                .chunks_exact(4)
                .zip(mesh.normals.chunks_exact(4))
            {
                let middle: [f32; 3] =
                    std::array::from_fn(|i| positions.iter().map(|p| p[i]).sum::<f32>() * 0.25);
                let normal = normals[0];
                let radius = middle[transverse].hypot(middle[2]);
                if middle[axis].abs() < 1.5
                    && (radius - 5.).abs() < 0.15
                    && normal[axis].abs() < 0.1
                {
                    checked += 1;
                    let radial =
                        (middle[transverse] * normal[transverse] + middle[2] * normal[2]) / radius;
                    if radial < 0.8 {
                        bad += 1;
                    }
                }
            }
            assert!(checked > 100);
            assert_eq!(
                bad, 0,
                "axis {axis}: {bad} of {checked} cylindrical side quads have sideways normals"
            );
        }
    }
    #[test]
    fn cylinders_and_tubes_have_correct_volume_and_preserve_internal_air() {
        for axis in 0..3 {
            let cylinder =
                CylindricalStock::new(axis, [2., 3., 4.], [-5., 5.], 5., 2., 0.1).unwrap();
            let field = &cylinder.field;
            assert!(field
                .to_solid_mesh(cylinder.min[2])
                .machined
                .iter()
                .all(|v| !*v));
            let mut volume = 0.;
            for y in 0..field.ny {
                for x in 0..field.nx {
                    let i = y * field.nx + x;
                    let void = field
                        .bounded
                        .get(&i)
                        .map_or(0., |v| v.iter().map(|s| f64::from(s.upper - s.lower)).sum());
                    volume += (f64::from(field.z[i]) - cylinder.min[2] - void).max(0.) * 0.01;
                }
            }
            let expected = std::f64::consts::PI * (25. - 4.) * 10.;
            assert!(
                (volume - expected).abs() / expected < 0.015,
                "axis {axis}: {volume}"
            );
        }
    }
    #[test]
    fn invalid_cylinder_dimensions_are_rejected_before_allocating() {
        for (outer, inner, res) in [
            (0., 0., 0.1),
            (5., 5., 0.1),
            (5., -1., 0.1),
            (5., 0., 0.),
            (5., 0., f64::NAN),
            (5., 0., 1e-9),
        ] {
            assert!(CylindricalStock::new(0, [0.; 3], [-5., 5.], outer, inner, res).is_err());
        }
    }
}
