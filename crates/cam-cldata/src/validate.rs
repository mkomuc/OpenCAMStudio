//! Controller-neutral motion validation and analytic XY-arc bounds.

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use core::fmt;

use crate::{ArcDir, Point3, Program, Step};

/// Maximum admitted difference between an arc's start/end radii, in mm.
/// Bounds use the larger radius so this small geometric tolerance cannot hide travel.
pub const ARC_RADIUS_TOLERANCE_MM: f64 = 0.005;

#[derive(Clone, Debug, PartialEq)]
pub enum ValidationKind {
    NonFinite { field: &'static str, value: f64 },
    NonPositive { field: &'static str, value: f64 },
    Negative { field: &'static str, value: f64 },
    ArcWithoutStart,
    ArcRadiusMismatch { start: f64, end: f64 },
    InvalidDrill(&'static str),
    InvalidDatum(u32),
}

/// The first invalid step, with its zero-based index and operation provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationError {
    pub step: usize,
    pub op: Option<u32>,
    pub kind: ValidationKind,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "step {}", self.step + 1)?;
        if let Some(op) = self.op {
            write!(f, " (operation {op})")?;
        }
        write!(f, ": ")?;
        match &self.kind {
            ValidationKind::NonFinite { field, value } => write!(f, "{field} must be finite, got {value}"),
            ValidationKind::NonPositive { field, value } => write!(f, "{field} must be positive, got {value}"),
            ValidationKind::Negative { field, value } => write!(f, "{field} must not be negative, got {value}"),
            ValidationKind::ArcWithoutStart => write!(f, "arc has no preceding position in this datum"),
            ValidationKind::ArcRadiusMismatch { start, end } => write!(f,
                "arc radii differ by more than {ARC_RADIUS_TOLERANCE_MM} mm ({start:.6} vs {end:.6})"),
            ValidationKind::InvalidDrill(reason) => write!(f, "invalid drilling cycle: {reason}"),
            ValidationKind::InvalidDatum(index) => write!(f, "work datum {index} is invalid; datum indices start at 1"),
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionBounds {
    pub min: Point3,
    pub max: Point3,
}

impl MotionBounds {
    fn include(bounds: &mut Option<Self>, point: Point3) {
        match bounds {
            None => {
                *bounds = Some(Self {
                    min: point,
                    max: point,
                })
            }
            Some(b) => {
                b.min.x = b.min.x.min(point.x);
                b.min.y = b.min.y.min(point.y);
                b.min.z = b.min.z.min(point.z);
                b.max.x = b.max.x.max(point.x);
                b.max.y = b.max.y.max(point.y);
                b.max.z = b.max.z.max(point.z);
            }
        }
    }
}

fn finite(field: &'static str, value: f64) -> Result<(), ValidationKind> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ValidationKind::NonFinite { field, value })
    }
}

fn positive(field: &'static str, value: f64) -> Result<(), ValidationKind> {
    finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(ValidationKind::NonPositive { field, value })
    }
}

fn nonnegative(field: &'static str, value: f64) -> Result<(), ValidationKind> {
    finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(ValidationKind::Negative { field, value })
    }
}

fn point(p: Point3) -> Result<(), ValidationKind> {
    finite("X", p.x)?;
    finite("Y", p.y)?;
    finite("Z", p.z)
}

/// Positive commanded sweep in radians, for a validated XY arc. Equal angular
/// endpoints represent a full circle; Z may change independently for a helix.
pub fn arc_sweep(start: Point3, end: Point3, center: Point3, dir: ArcDir) -> f64 {
    let a = (start.y - center.y).atan2(start.x - center.x);
    let b = (end.y - center.y).atan2(end.x - center.x);
    let sweep = match dir {
        ArcDir::Ccw => b - a,
        ArcDir::Cw => a - b,
    }
    .rem_euclid(TAU);
    if sweep == 0.0 {
        TAU
    } else {
        sweep
    }
}

/// Validate every numeric motion/control/cycle field and calculate swept bounds.
/// Datum events invalidate the current-position anchor; tool changes do not add
/// an unmodeled machine trajectory. These are work-coordinate span bounds, not
/// proof of absolute machine placement or physical tool-change clearance.
pub fn validate_program(program: &Program) -> Result<Option<MotionBounds>, ValidationError> {
    let mut bounds = None;
    let mut current: Option<Point3> = None;
    for (index, step) in program.steps().iter().enumerate() {
        let op = match step {
            Step::Rapid { tag, .. } | Step::Linear { tag, .. } | Step::Arc { tag, .. } => {
                Some(tag.op_id)
            }
            Step::Drill(cycle) => Some(cycle.tag.op_id),
            _ => None,
        };
        let fail = |kind| ValidationError {
            step: index,
            op,
            kind,
        };
        match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                point(*to).map_err(fail)?;
                if let Step::Linear { feed, .. } = step {
                    positive("feed", *feed).map_err(fail)?;
                }
                MotionBounds::include(&mut bounds, *to);
                current = Some(*to);
            }
            Step::Arc {
                end,
                center,
                dir,
                feed,
                ..
            } => {
                point(*end).map_err(fail)?;
                point(*center).map_err(fail)?;
                positive("feed", *feed).map_err(fail)?;
                let start = current.ok_or_else(|| fail(ValidationKind::ArcWithoutStart))?;
                let r = (start.x - center.x).hypot(start.y - center.y);
                let end_r = (end.x - center.x).hypot(end.y - center.y);
                positive("arc radius", r).map_err(fail)?;
                positive("arc end radius", end_r).map_err(fail)?;
                if (r - end_r).abs() > ARC_RADIUS_TOLERANCE_MM {
                    return Err(fail(ValidationKind::ArcRadiusMismatch {
                        start: r,
                        end: end_r,
                    }));
                }
                MotionBounds::include(&mut bounds, start);
                MotionBounds::include(&mut bounds, *end);
                let sweep = arc_sweep(start, *end, *center, *dir);
                let angle = (start.y - center.y).atan2(start.x - center.x);
                let radius = r.max(end_r);
                for (theta, dx, dy) in [
                    (0.0, radius, 0.0),
                    (FRAC_PI_2, 0.0, radius),
                    (PI, -radius, 0.0),
                    (3.0 * FRAC_PI_2, 0.0, -radius),
                ] {
                    let progress = match dir {
                        ArcDir::Ccw => theta - angle,
                        ArcDir::Cw => angle - theta,
                    }
                    .rem_euclid(TAU);
                    if progress <= sweep + 1e-12 {
                        let extreme = Point3::new(center.x + dx, center.y + dy, start.z);
                        point(extreme).map_err(fail)?;
                        MotionBounds::include(&mut bounds, extreme);
                    }
                }
                current = Some(*end);
            }
            Step::Spindle { rpm, .. } => positive("spindle speed", *rpm).map_err(fail)?,
            Step::Dwell { seconds } => nonnegative("dwell", *seconds).map_err(fail)?,
            Step::Datum(datum) => {
                if *datum == 0 {
                    return Err(fail(ValidationKind::InvalidDatum(*datum)));
                }
                current = None;
            }
            Step::Drill(c) => {
                finite("drill top", c.z_top).map_err(fail)?;
                finite("drill depth", c.depth).map_err(fail)?;
                finite("drill retract", c.retract).map_err(fail)?;
                positive("drill feed", c.feed).map_err(fail)?;
                if c.retract < c.z_top || c.z_top <= c.depth {
                    return Err(fail(ValidationKind::InvalidDrill(
                        "require retract >= top > depth",
                    )));
                }
                positive("drill span", c.z_top - c.depth).map_err(fail)?;
                if let Some(peck) = c.peck {
                    positive("peck", peck).map_err(fail)?;
                    if c.z_top - peck == c.z_top || c.depth + peck == c.depth {
                        return Err(fail(ValidationKind::InvalidDrill(
                            "peck cannot advance at these Z coordinates",
                        )));
                    }
                }
                if let Some(dwell) = c.dwell {
                    nonnegative("drill dwell", dwell).map_err(fail)?;
                }
                for &[x, y] in &c.points {
                    let bottom = Point3::new(x, y, c.depth);
                    point(bottom).map_err(fail)?;
                    MotionBounds::include(&mut bounds, bottom);
                    MotionBounds::include(&mut bounds, Point3::new(x, y, c.retract));
                }
                if let Some(&[x, y]) = c.points.last() {
                    current = Some(Point3::new(x, y, c.retract));
                }
            }
            Step::SpindleOff
            | Step::Coolant(_)
            | Step::ToolChange { .. }
            | Step::Stop
            | Step::Comment(_)
            | Step::CutterComp(_) => {}
        }
    }
    Ok(bounds)
}
