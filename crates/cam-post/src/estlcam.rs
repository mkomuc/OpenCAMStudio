//! Estlcam CNC control: https://www.estlcam.de/steuerung_cnc_programme.php
//!
//! The control ignores unsupported words, so refuse intent it cannot execute.
//! Units must be configured in Estlcam as mm and mm/min; G20/G21/G94 have no effect.

use cam_cldata::{arc_sweep, ArcDir, Coolant, CutterComp, Point3, Program, SpindleDir, Step};
use cam_model::{Machine, Tool, ToolKind};

use crate::words::{compact, num, sanitize};
use crate::writer::Writer;
use crate::{Capabilities, Post, PostError, PostOptions, WorkOffset};

/// Outputs switched on by coolant intent (flood or mist), and off by coolant-off
/// and at program end. Either output can drive an MMS lubrication system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EstlcamOptions {
    /// Output 1: M8 on, M9 off.
    pub output_1: bool,
    /// Output 2: M10 on, M11 off.
    pub output_2: bool,
}

impl Default for EstlcamOptions {
    fn default() -> Self {
        Self {
            output_1: true,
            output_2: false,
        }
    }
}

/// Estlcam's single-datum, XY-arc CNC dialect.
#[derive(Clone, Copy, Debug, Default)]
pub struct EstlcamPost;

impl Post for EstlcamPost {
    fn name(&self) -> &str {
        "Estlcam"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            arcs: true,
            canned_drill: false,
            cutter_comp: false,
            work_offsets: false,
            coolant: true,
            tool_change: true,
        }
    }

    fn post(
        &self,
        program: &Program,
        machine: &Machine,
        options: &PostOptions,
    ) -> Result<String, PostError> {
        crate::validate::preflight(program, machine, options)?;
        if options.work_offset != WorkOffset::G54 {
            return Err(unsupported("work offsets; use the single Estlcam origin"));
        }
        let program = split_circles(program, options.precision)?;
        // Split endpoints and their new I/J offsets must also survive NC rounding.
        crate::validate::preflight(&program, machine, options)?;
        let mut w = Writer::new(machine, options.precision);
        w.explicit_motion();
        if let Some(name) = &options.program_name {
            w.line(format!("({})", sanitize(name)));
        }
        w.line("(Estlcam: configure units to mm and feed to mm/min)".into());
        w.line("M5".into());
        outputs(&mut w, options.estlcam, false);
        let header = options.program_name.as_deref().map(sanitize);
        let mut dedupe = header.is_some();
        let mut spindle_rpm = None;
        for (index, step) in program.steps().iter().enumerate() {
            match step {
                Step::Comment(text) => {
                    let text = sanitize(text);
                    if dedupe {
                        dedupe = false;
                        if header.as_deref() == Some(&text) {
                            continue;
                        }
                    }
                    w.line(format!("({text})"));
                }
                Step::Spindle {
                    dir: SpindleDir::Cw,
                    rpm,
                } => {
                    spindle_rpm = Some(*rpm);
                    w.line(format!("M3 S{}", compact(*rpm, 0)));
                }
                Step::Spindle {
                    dir: SpindleDir::Ccw,
                    ..
                } => return Err(unsupported("counterclockwise spindle (M4)")),
                Step::SpindleOff => w.line("M5".into()),
                Step::Coolant(c) => outputs(&mut w, options.estlcam, *c != Coolant::Off),
                Step::ToolChange { tool } => {
                    // Estlcam displays the comment attached to M6 during the manual
                    // change. Use the upcoming spindle command, not library defaults:
                    // an operation can override the tool's nominal speed.
                    let rpm = program.steps()[index + 1..]
                        .iter()
                        .take_while(|s| {
                            !matches!(
                                s,
                                Step::ToolChange { .. }
                                    | Step::Rapid { .. }
                                    | Step::Linear { .. }
                                    | Step::Arc { .. }
                                    | Step::Drill(_)
                                    | Step::Stop
                            )
                        })
                        .find_map(|s| match s {
                            Step::Spindle { rpm, .. } => Some(*rpm),
                            _ => None,
                        })
                        .or(spindle_rpm);
                    let details = tool_change_details(
                        *tool,
                        options.tools.iter().find(|t| t.number == *tool),
                        rpm,
                        options.precision,
                    );
                    w.line(format!("T{tool} M6 ({details})"));
                    w.reset_modal();
                }
                Step::Stop => w.line("M0".into()),
                Step::Datum(1) | Step::CutterComp(CutterComp::Off) => {}
                Step::Datum(_) => {
                    return Err(unsupported(
                        "multiple work datums; export each origin separately",
                    ))
                }
                Step::CutterComp(_) => {
                    return Err(unsupported(
                        "controller cutter compensation; use computed compensation",
                    ))
                }
                Step::Dwell { seconds } if *seconds == 0.0 => {}
                Step::Dwell { .. } => return Err(unsupported("timed dwell (G4)")),
                Step::Rapid { to, .. } => w.rapid(*to)?,
                Step::Linear { to, feed, .. } => w.feed_move(*to, *feed)?,
                Step::Arc {
                    end,
                    center,
                    dir,
                    feed,
                    ..
                } => w.arc(*end, *center, *dir, *feed)?,
                Step::Drill(c) => {
                    if c.dwell.is_some_and(|d| d > 0.0) {
                        return Err(unsupported("drilling dwell (G4); disable the dwell"));
                    }
                    let mut c = c.clone();
                    c.dwell = None;
                    crate::dialect::expanded_drill(&mut w, &c)?;
                }
            }
        }
        w.line("M5".into());
        outputs(&mut w, options.estlcam, false);
        // End at EOF: M2/M30 are not supported by Estlcam.
        Ok(w.finish())
    }
}

fn tool_change_details(
    number: u32,
    tool: Option<&Tool>,
    rpm: Option<f64>,
    precision: usize,
) -> String {
    let mut details = format!("T{number}");
    if let Some(tool) = tool {
        let description = sanitize(&tool.description);
        if !description.trim().is_empty() {
            details.push_str(&format!(" - {}", description.trim()));
        }
        let corner_radius = match tool.kind {
            ToolKind::BallMill => tool.diameter / 2.0,
            ToolKind::BullNose { corner_radius } => corner_radius,
            ToolKind::VBit { tip_radius, .. } => tip_radius,
            _ => 0.0,
        };
        details.push_str(&format!(
            " - {} - D{} - FL{} - CR={} - F{}",
            tool.kind,
            num(tool.diameter, precision),
            num(tool.flute_len(), precision),
            num(corner_radius, precision),
            tool.flutes,
        ));
    }
    if let Some(rpm) = rpm {
        details.push_str(&format!(" - RPM {}", compact(rpm, 0)));
    }
    details
}

fn unsupported(feature: &str) -> PostError {
    PostError::Unsupported(format!("Estlcam: {feature}"))
}

fn outputs(w: &mut Writer, options: EstlcamOptions, on: bool) {
    if options.output_1 {
        w.line(if on { "M8" } else { "M9" }.into());
    }
    if options.output_2 {
        w.line(if on { "M10" } else { "M11" }.into());
    }
}

/// Estlcam accepts helices, but not full circles. Split each full revolution
/// into two half arcs, distributing Z travel equally for a helix. Also split
/// near-full arcs whose endpoints become identical at the output precision.
fn split_circles(program: &Program, precision: usize) -> Result<Program, PostError> {
    let mut out = Program::new();
    let mut current = None;
    for step in program.steps() {
        match step {
            Step::Arc {
                end,
                center,
                dir,
                feed,
                tag,
            } => {
                let start: Point3 = current.ok_or(PostError::ArcWithoutStart)?;
                let same_xy = |a: Point3, b: Point3| {
                    num(a.x, precision) == num(b.x, precision)
                        && num(a.y, precision) == num(b.y, precision)
                };
                if same_xy(start, *end) {
                    let half = arc_sweep(start, *end, *center, *dir) / 2.0
                        * if *dir == ArcDir::Cw { -1.0 } else { 1.0 };
                    let (sin, cos) = half.sin_cos();
                    let (dx, dy) = (start.x - center.x, start.y - center.y);
                    let middle = Point3::new(
                        center.x + dx * cos - dy * sin,
                        center.y + dx * sin + dy * cos,
                        start.z + (end.z - start.z) / 2.0,
                    );
                    if same_xy(start, middle) || same_xy(middle, *end) {
                        return Err(unsupported(
                            "circle collapses at output precision; increase precision",
                        ));
                    }
                    out.push(Step::Arc {
                        end: middle,
                        center: *center,
                        dir: *dir,
                        feed: *feed,
                        tag: *tag,
                    });
                }
                current = Some(*end);
            }
            Step::Rapid { to, .. } | Step::Linear { to, .. } => current = Some(*to),
            Step::Datum(_) => current = None,
            Step::Drill(c) => {
                if let Some(&[x, y]) = c.points.last() {
                    current = Some(Point3::new(x, y, c.retract));
                }
            }
            _ => {}
        }
        out.push(step.clone());
    }
    Ok(out)
}
