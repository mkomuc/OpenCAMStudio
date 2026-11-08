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
        let mut current = None;
        for step in &self.steps {
            let previous = current;
            match step {
                Step::Rotary { to, .. } | Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                    current = Some(*to)
                }
                Step::Arc { end, .. } => current = Some(*end),
                Step::Datum(_) => current = None,
                _ => {}
            }
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
                Step::Linear { to, feed, tag } if frame.is_some() => {
                    let (axis, center) = frame.unwrap();
                    result.push(Step::Linear {
                        to: axis.material_point(*to, center, angle),
                        feed: *feed,
                        tag: *tag,
                    });
                }
                Step::Arc {
                    end,
                    center,
                    dir,
                    feed,
                    tag,
                } if frame.is_some() && angle.abs() > 1e-12 => {
                    if let Some(start) = previous {
                        let (axis, origin) = frame.unwrap();
                        let sweep = crate::arc_sweep(start, *end, *center, *dir);
                        let n = (sweep / 5_f64.to_radians()).ceil().max(1.) as usize;
                        let r = (start.x - center.x).hypot(start.y - center.y);
                        let a = (start.y - center.y).atan2(start.x - center.x);
                        let sign = if *dir == crate::ArcDir::Cw { -1. } else { 1. };
                        for i in 1..=n {
                            let t = i as f64 / n as f64;
                            let p = if i == n {
                                *end
                            } else {
                                Point3::new(
                                    center.x + r * (a + sign * sweep * t).cos(),
                                    center.y + r * (a + sign * sweep * t).sin(),
                                    start.z + (end.z - start.z) * t,
                                )
                            };
                            result.push(Step::Linear {
                                to: axis.material_point(p, origin, angle),
                                feed: *feed,
                                tag: *tag,
                            });
                        }
                    }
                }
                Step::Datum(_) => {
                    result.push(step.clone());
                }
                _ => result.push(step.clone()),
            }
        }
        result
    }
}
