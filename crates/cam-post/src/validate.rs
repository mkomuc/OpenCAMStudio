//! Common preflight for every emission entry point, including formatted NC words.

use cam_cldata::{arc_sweep, MotionBounds, Point3, Program, Step, ValidationKind};
use cam_model::Machine;

use crate::words::{compact, num};
use crate::{PostError, PostOptions};

fn travel(bounds: Option<MotionBounds>, machine: &Machine) -> Result<(), PostError> {
    let Some(b) = bounds else {
        return Ok(());
    };
    let (ex, ey, ez) = machine.envelope.extent();
    for (axis, span, travel) in [
        ('X', b.max.x - b.min.x, ex),
        ('Y', b.max.y - b.min.y, ey),
        ('Z', b.max.z - b.min.z, ez),
    ] {
        if !span.is_finite() || span > travel + 1e-6 {
            return Err(PostError::TravelExceeded { axis, span, travel });
        }
    }
    Ok(())
}

fn rates(program: &Program, machine: &Machine) -> Result<(), PostError> {
    for step in program.steps() {
        let feed = match step {
            Step::Linear { feed, .. } | Step::Arc { feed, .. } => Some(*feed),
            Step::Drill(c) => Some(c.feed),
            _ => None,
        };
        if let Some(feed) = feed {
            if feed > machine.max_feed {
                return Err(PostError::FeedOutOfRange(feed));
            }
        }
        if let Step::Spindle { rpm, .. } = step {
            if *rpm > machine.max_spindle_rpm {
                return Err(PostError::SpindleOutOfRange(*rpm));
            }
        }
    }
    Ok(())
}

pub(crate) fn preflight(
    program: &Program,
    machine: &Machine,
    options: &PostOptions,
) -> Result<(), PostError> {
    machine.validate().map_err(PostError::InvalidMachine)?;
    if options.precision > 9 {
        return Err(PostError::InvalidPrecision(options.precision));
    }
    let bounds = program.validate().map_err(|err| match err.kind {
        ValidationKind::ArcWithoutStart => PostError::ArcWithoutStart,
        ValidationKind::InvalidDatum(_) => PostError::Unsupported(err.to_string()),
        _ => PostError::InvalidProgram(err),
    })?;
    rates(program, machine)?;
    travel(bounds, machine)?;

    // Use the exact word formatters used by the emitters, rather than an
    // arithmetic rounding approximation. Writer stores the original position,
    // so I/J are rounded offsets from that position, not rounded absolute centres.
    let coordinate = |value| {
        num(value, options.precision)
            .parse::<f64>()
            .expect("formatted finite coordinate")
    };
    let rate = |value| {
        compact(value, options.precision)
            .parse::<f64>()
            .expect("formatted finite rate")
    };
    let point = |p: Point3| Point3::new(coordinate(p.x), coordinate(p.y), coordinate(p.z));
    let mut rounded = Program::new();
    let mut current = None;
    for (index, step) in program.steps().iter().enumerate() {
        let mut output = step.clone();
        match &mut output {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => {
                current = Some(*to);
                *to = point(*to);
                if let Step::Linear { feed, .. } = &mut output {
                    *feed = rate(*feed);
                }
            }
            Step::Arc {
                end,
                center,
                dir,
                feed,
                tag,
            } => {
                let start = current.expect("validated arc anchor");
                let rounded_start = point(start);
                let rounded_end = point(*end);
                let rounded_center = Point3::new(
                    rounded_start.x + coordinate(center.x - start.x),
                    rounded_start.y + coordinate(center.y - start.y),
                    coordinate(center.z),
                );
                let original_sweep = arc_sweep(start, *end, *center, *dir);
                let output_sweep = arc_sweep(rounded_start, rounded_end, rounded_center, *dir);
                if (original_sweep - output_sweep).abs() > core::f64::consts::PI {
                    return Err(PostError::UnsafeRounding {
                        step: index,
                        op: tag.op_id,
                    });
                }
                current = Some(*end);
                *end = rounded_end;
                *center = rounded_center;
                *feed = rate(*feed);
            }
            Step::Spindle { rpm, .. } => {
                *rpm = compact(*rpm, 0).parse().expect("formatted spindle speed")
            }
            Step::Dwell { seconds } => {
                *seconds = compact(*seconds, 3).parse().expect("formatted dwell")
            }
            Step::Datum(_) => current = None,
            Step::Drill(c) => {
                if let Some(&[x, y]) = c.points.last() {
                    current = Some(Point3::new(x, y, c.retract));
                }
                c.z_top = coordinate(c.z_top);
                c.depth = coordinate(c.depth);
                c.retract = coordinate(c.retract);
                c.feed = rate(c.feed);
                c.peck = c.peck.map(rate);
                c.dwell = c
                    .dwell
                    .map(|v| compact(v, 3).parse().expect("formatted drill dwell"));
                for p in &mut c.points {
                    *p = [coordinate(p[0]), coordinate(p[1])];
                }
            }
            _ => {}
        }
        rounded.push(output);
    }
    let bounds = rounded.validate().map_err(PostError::InvalidOutput)?;
    rates(&rounded, machine)?;
    travel(bounds, machine)
}
