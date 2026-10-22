//! Analytic, controller-neutral processing-time estimate.

use crate::{arc_sweep, MoveKind, Point3, Program, Step};

/// Motion distances and elapsed-time components for a program estimate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcessingTime {
    pub feed_distance_mm: f64,
    pub rapid_distance_mm: f64,
    pub feed_seconds: f64,
    /// Feed time spent on ramp, helix, and lead-in entry motions.
    pub ramp_helix_seconds: f64,
    /// Feed time spent on straight vertical plunges.
    pub plunge_seconds: f64,
    pub rapid_seconds: f64,
    pub dwell_seconds: f64,
    pub tool_changes: usize,
    pub tool_change_seconds: f64,
}

impl ProcessingTime {
    /// Sum of feed, rapid, dwell, and tool-change time components.
    pub fn total_seconds(&self) -> f64 {
        self.feed_seconds + self.rapid_seconds + self.dwell_seconds + self.tool_change_seconds
    }

    /// Feed time left after ramp/helix entries and straight plunges.
    pub fn cutting_seconds(&self) -> f64 {
        (self.feed_seconds - self.ramp_helix_seconds - self.plunge_seconds).max(0.0)
    }
}

impl Program {
    /// Estimate machining time. `rapid_rate` is mm/min; feed override is percent.
    /// Unknown initial position is ignored, and a datum change clears the position anchor.
    pub fn processing_time(
        &self,
        rapid_rate: f64,
        tool_change_seconds: f64,
        feed_override_percent: f64,
    ) -> Result<ProcessingTime, String> {
        if !rapid_rate.is_finite() || rapid_rate <= 0.0 {
            return Err("rapid rate must be finite and positive".into());
        }
        if !tool_change_seconds.is_finite() || tool_change_seconds < 0.0 {
            return Err("tool change time must be finite and nonnegative".into());
        }
        if !feed_override_percent.is_finite() || feed_override_percent <= 0.0 {
            return Err("feed override percent must be finite and positive".into());
        }
        self.validate().map_err(|e| e.to_string())?;
        let override_scale = feed_override_percent / 100.0;
        let mut out = ProcessingTime::default();
        let mut current: Option<Point3> = None;
        for step in self.steps() {
            match step {
                Step::Rapid { to, .. } => {
                    if let Some(from) = current {
                        out.rapid_distance_mm += dist(from, *to);
                    }
                    current = Some(*to);
                }
                Step::Linear { to, feed, tag } => {
                    if let Some(from) = current {
                        let d = dist(from, *to);
                        out.feed_distance_mm += d;
                        let seconds = d / (feed * override_scale) * 60.0;
                        out.feed_seconds += seconds;
                        match tag.kind {
                            MoveKind::LeadIn => out.ramp_helix_seconds += seconds,
                            MoveKind::Plunge if (to.x - from.x).hypot(to.y - from.y) > 1e-9 => {
                                out.ramp_helix_seconds += seconds
                            }
                            MoveKind::Plunge => out.plunge_seconds += seconds,
                            _ => {}
                        }
                    }
                    current = Some(*to);
                }
                Step::Arc {
                    end,
                    center,
                    dir,
                    feed,
                    tag,
                } => {
                    let from = current.expect("validated arcs have a start");
                    let radius = (from.x - center.x).hypot(from.y - center.y);
                    let sweep = arc_sweep(from, *end, *center, *dir);
                    let planar = radius * sweep;
                    let d = planar.hypot(end.z - from.z);
                    out.feed_distance_mm += d;
                    let seconds = d / (feed * override_scale) * 60.0;
                    out.feed_seconds += seconds;
                    if matches!(tag.kind, MoveKind::LeadIn | MoveKind::Plunge) {
                        out.ramp_helix_seconds += seconds;
                    }
                    current = Some(*end);
                }
                Step::Drill(c) => {
                    let mut total_pecks = 0usize;
                    for &[x, y] in &c.points {
                        let retract = Point3::new(x, y, c.retract);
                        let top = Point3::new(x, y, c.z_top);
                        if let Some(from) = current {
                            out.rapid_distance_mm += dist(from, retract);
                        }
                        out.rapid_distance_mm += dist(retract, top);
                        let mut top_of_peck = c.z_top;
                        let mut first = true;
                        loop {
                            total_pecks += 1;
                            if total_pecks > 100_000 {
                                return Err("drill cycle exceeds 100000 expanded pecks".into());
                            }
                            let target = c
                                .peck
                                .map(|peck| (top_of_peck - peck).max(c.depth))
                                .unwrap_or(c.depth);
                            if target >= top_of_peck {
                                return Err("drill peck cannot advance at this depth".into());
                            }
                            let feed_start = if first {
                                top_of_peck
                            } else {
                                let resume = (top_of_peck + 0.5).min(c.z_top);
                                out.rapid_distance_mm += (c.retract - resume).abs();
                                resume
                            };
                            let fd = feed_start - target;
                            out.feed_distance_mm += fd;
                            let seconds = fd / (c.feed * override_scale) * 60.0;
                            out.feed_seconds += seconds;
                            out.plunge_seconds += seconds;
                            out.rapid_distance_mm += (c.retract - target).abs();
                            let last = target <= c.depth;
                            if last {
                                if let Some(d) = c.dwell {
                                    out.dwell_seconds += d;
                                }
                                break;
                            }
                            top_of_peck = target;
                            first = false;
                        }
                        current = Some(retract);
                    }
                }
                Step::Dwell { seconds } => out.dwell_seconds += seconds,
                Step::ToolChange { .. } => out.tool_changes += 1,
                Step::Datum(_) => current = None,
                _ => {}
            }
        }
        out.rapid_seconds = out.rapid_distance_mm / rapid_rate * 60.0;
        out.tool_change_seconds = out.tool_changes as f64 * tool_change_seconds;
        if [
            out.feed_distance_mm,
            out.rapid_distance_mm,
            out.feed_seconds,
            out.ramp_helix_seconds,
            out.plunge_seconds,
            out.rapid_seconds,
            out.dwell_seconds,
            out.tool_change_seconds,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("processing time estimate overflowed".into());
        }
        if !out.total_seconds().is_finite() {
            return Err("processing time total overflowed".into());
        }
        Ok(out)
    }
}

fn dist(a: Point3, b: Point3) -> f64 {
    (b.x - a.x).hypot(b.y - a.y).hypot(b.z - a.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArcDir, DrillCycle, MoveKind, Tag};

    #[test]
    fn line_arc_dwell_toolchange_and_override() {
        let tag = Tag::new(0, MoveKind::Cutting);
        let p = Program {
            steps: vec![
                Step::ToolChange { tool: 1 },
                Step::Rapid {
                    to: Point3::new(0.0, 0.0, 0.0),
                    tag,
                },
                Step::Linear {
                    to: Point3::new(10.0, 0.0, 0.0),
                    feed: 600.0,
                    tag,
                },
                Step::Arc {
                    end: Point3::new(0.0, 0.0, 0.0),
                    center: Point3::new(5.0, 0.0, 0.0),
                    dir: ArcDir::Ccw,
                    feed: 600.0,
                    tag,
                },
                Step::Dwell { seconds: 2.0 },
            ],
        };
        let t = p.processing_time(1200.0, 5.0, 50.0).unwrap();
        assert!((t.feed_distance_mm - (10.0 + 5.0 * core::f64::consts::PI)).abs() < 1e-9);
        assert!((t.feed_seconds - t.feed_distance_mm / 300.0 * 60.0).abs() < 1e-9);
        assert_eq!(t.ramp_helix_seconds, 0.0);
        assert_eq!(t.plunge_seconds, 0.0);
        assert_eq!(t.dwell_seconds, 2.0);
        assert_eq!(t.tool_changes, 1);
        assert_eq!(t.total_seconds(), t.feed_seconds + t.rapid_seconds + 7.0);
    }

    #[test]
    fn peck_cycle_counts_full_retract_clearance_and_dwell() {
        let p = Program {
            steps: vec![Step::Drill(DrillCycle {
                points: vec![[1.0, 2.0]],
                z_top: 0.0,
                depth: -1.2,
                retract: 2.0,
                peck: Some(0.5),
                dwell: Some(1.0),
                feed: 60.0,
                tag: Tag::new(0, MoveKind::Plunge),
            })],
        };
        let t = p.processing_time(60.0, 0.0, 100.0).unwrap();
        assert!((t.feed_distance_mm - 2.2).abs() < 1e-9);
        assert!((t.plunge_seconds - t.feed_seconds).abs() < 1e-9);
        // Explicit-post expansion: rapid to top, feed to -0.5, retract,
        // rapid to 0.0, feed to -1.0, retract, rapid to -0.5, feed to -1.2,
        // then retract. Include the 0.5 mm resume clearance in the feed path.
        assert!((t.rapid_distance_mm - 15.2).abs() < 1e-9);
        assert_eq!(t.dwell_seconds, 1.0);

        let tag = Tag::new(0, MoveKind::Plunge);
        let expanded = Program {
            steps: vec![
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 2.0),
                    tag,
                },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 0.0),
                    tag,
                },
                Step::Linear {
                    to: Point3::new(1.0, 2.0, -0.5),
                    feed: 60.0,
                    tag,
                },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 2.0),
                    tag,
                },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 0.0),
                    tag,
                },
                Step::Linear {
                    to: Point3::new(1.0, 2.0, -1.0),
                    feed: 60.0,
                    tag,
                },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 2.0),
                    tag,
                },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, -0.5),
                    tag,
                },
                Step::Linear {
                    to: Point3::new(1.0, 2.0, -1.2),
                    feed: 60.0,
                    tag,
                },
                Step::Dwell { seconds: 1.0 },
                Step::Rapid {
                    to: Point3::new(1.0, 2.0, 2.0),
                    tag,
                },
            ],
        };
        let e = expanded.processing_time(60.0, 0.0, 100.0).unwrap();
        assert_eq!(t.feed_distance_mm, e.feed_distance_mm);
        assert_eq!(t.rapid_distance_mm, e.rapid_distance_mm);
        assert_eq!(t.feed_seconds, e.feed_seconds);
        assert_eq!(t.dwell_seconds, e.dwell_seconds);
    }

    #[test]
    fn rejects_bad_rates_and_unbounded_pecks() {
        let empty = Program::new();
        assert!(empty.processing_time(0.0, 0.0, 100.0).is_err());
        assert!(empty.processing_time(10.0, f64::INFINITY, 100.0).is_err());
        assert!(empty.processing_time(10.0, 0.0, f64::NAN).is_err());
        assert!(empty.processing_time(10.0, 0.0, f64::INFINITY).is_err());
    }

    #[test]
    fn full_circle_helix_and_datum_reset_are_handled() {
        let tag = Tag::new(0, MoveKind::Cutting);
        let p = Program {
            steps: vec![
                Step::Rapid {
                    to: Point3::new(1.0, 0.0, 0.0),
                    tag,
                },
                Step::Arc {
                    end: Point3::new(1.0, 0.0, 3.0),
                    center: Point3::new(0.0, 0.0, 0.0),
                    dir: ArcDir::Ccw,
                    feed: 60.0,
                    tag,
                },
                Step::Datum(2),
                Step::Rapid {
                    to: Point3::new(100.0, 0.0, 0.0),
                    tag,
                },
            ],
        };
        let t = p.processing_time(60.0, 0.0, 100.0).unwrap();
        assert!((t.feed_distance_mm - core::f64::consts::TAU.hypot(3.0)).abs() < 1e-9);
        assert_eq!(t.rapid_distance_mm, 0.0);
    }

    #[test]
    fn rejects_total_time_overflow() {
        let p = Program {
            steps: vec![
                Step::Dwell { seconds: f64::MAX },
                Step::Dwell { seconds: f64::MAX },
            ],
        };
        assert!(p.processing_time(1.0, 0.0, 100.0).is_err());
    }
}
