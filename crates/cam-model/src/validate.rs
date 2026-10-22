//! Numeric validation of physical machine definitions, independent of posts.

use crate::Machine;
use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct MachineValidationError {
    pub field: &'static str,
    pub value: f64,
    pub positive: bool,
    pub nonnegative: bool,
}

impl fmt::Display for MachineValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "machine {} must be finite{}{}, got {}",
            self.field,
            if self.positive { " and positive" } else { "" },
            if self.nonnegative {
                " and nonnegative"
            } else {
                ""
            },
            self.value
        )
    }
}

impl std::error::Error for MachineValidationError {}

fn check(field: &'static str, value: f64, positive: bool) -> Result<(), MachineValidationError> {
    check_bounds(field, value, positive, false)
}

fn check_bounds(
    field: &'static str,
    value: f64,
    positive: bool,
    nonnegative: bool,
) -> Result<(), MachineValidationError> {
    if value.is_finite() && (!positive || value > 0.0) && (!nonnegative || value >= 0.0) {
        Ok(())
    } else {
        Err(MachineValidationError {
            field,
            value,
            positive,
            nonnegative,
        })
    }
}

impl Machine {
    /// Check finite coordinates, positive rates and finite positive travel spans.
    /// Placement and physical clearance require a resolved machine/setup model.
    pub fn validate(&self) -> Result<(), MachineValidationError> {
        check_bounds("tool-change time", self.tool_change_seconds, false, true)?;
        for (field, value) in [
            ("rapid rate", self.rapid_rate),
            ("maximum spindle speed", self.max_spindle_rpm),
            ("maximum feed", self.max_feed),
        ] {
            check(field, value, true)?;
        }
        for (lo_field, hi_field, span_field, lo, hi) in [
            (
                "minimum X",
                "maximum X",
                "X travel",
                self.envelope.min.x,
                self.envelope.max.x,
            ),
            (
                "minimum Y",
                "maximum Y",
                "Y travel",
                self.envelope.min.y,
                self.envelope.max.y,
            ),
            (
                "minimum Z",
                "maximum Z",
                "Z travel",
                self.envelope.min.z,
                self.envelope.max.z,
            ),
        ] {
            check(lo_field, lo, false)?;
            check(hi_field, hi, false)?;
            check(span_field, hi - lo, true)?;
        }
        check("safe Z", self.safe_z, false)?;
        if let Some(p) = self.tool_change_pos {
            for (field, value) in [
                ("tool-change X", p.x),
                ("tool-change Y", p.y),
                ("tool-change Z", p.z),
            ] {
                check(field, value, false)?;
            }
        }
        Ok(())
    }
}
