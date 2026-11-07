use crate::{Point3, Program, Step};

/// Right-hand rotation of the workpiece around machine X (A) or Y (B).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RotaryAxis {
    #[default]
    A,
    B,
}
impl RotaryAxis {
    pub fn word(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
        }
    }
    /// Tool location in the stationary material frame, for rotary backplots.
    pub fn material_point(self, to: Point3, center: Point3, angle: f64) -> Point3 {
        let (s, c) = angle.to_radians().sin_cos();
        let (x, y, z) = (to.x - center.x, to.y - center.y, to.z - center.z);
        match self {
            Self::A => Point3::new(to.x, center.y + y * c + z * s, center.z - y * s + z * c),
            Self::B => Point3::new(center.x + x * c - z * s, to.y, center.z + x * s + z * c),
        }
    }
}
impl Program {
    pub fn has_rotary_motion(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Rotary { .. }))
    }
    /// Display-only material-frame polyline; never use this for posting or for
    /// fixed-Z stock verification (the cutter direction rotates in this frame).
    pub fn rotary_backplot(&self) -> Self {
        let mut angle = 0.;
        let mut frame = None;
        let mut result = Self::new();
        for step in &self.steps {
            match step {
                Step::Rotary {
                    to,
                    center,
                    axis,
                    angle: a,
                    feed,
                    tag,
                    ..
                } => {
                    angle = *a;
                    frame = Some((*axis, *center));
                    let to = axis.material_point(*to, *center, *a);
                    result.push(match feed {
                        Some(feed) => Step::Linear {
                            to,
                            feed: *feed,
                            tag: *tag,
                        },
                        None => Step::Rapid { to, tag: *tag },
                    });
                }
                Step::Rapid { to, tag } if frame.is_some() => {
                    let (axis, center) = frame.unwrap();
                    result.push(Step::Rapid {
                        to: axis.material_point(*to, center, angle),
                        tag: *tag,
                    });
                }
                Step::Datum(_) => {
                    frame = None;
                    angle = 0.;
                    result.push(step.clone());
                }
                _ => result.push(step.clone()),
            }
        }
        result
    }
}
