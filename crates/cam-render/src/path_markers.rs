//! Main approach/retract markers. Local leads and stay-down links are not entries.
use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};

use crate::{PathCone, PATH_IN, PATH_OUT};

fn point(p: Point3) -> [f32; 3] {
    [p.x as f32, p.y as f32, p.z as f32]
}

fn direction(v: [f64; 3]) -> Option<[f32; 3]> {
    let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    (length.is_finite() && length > 1e-9).then(|| v.map(|x| (x / length) as f32))
}

fn along(a: Point3, b: Point3) -> Option<[f32; 3]> {
    direction([b.x - a.x, b.y - a.y, b.z - a.z])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[derive(Clone, Copy)]
struct Motion {
    start: Point3,
    end: Point3,
    first: Option<[f32; 3]>,
    last: Option<[f32; 3]>,
    helical: bool,
}

impl Motion {
    fn line(start: Point3, end: Point3) -> Self {
        Self {
            start,
            end,
            first: along(start, end),
            last: along(start, end),
            helical: false,
        }
    }

    fn arc(start: Point3, end: Point3, center: Point3, dir: ArcDir) -> Self {
        let sign = if dir == ArcDir::Ccw { 1. } else { -1. };
        let a0 = (start.y - center.y).atan2(start.x - center.x);
        let a1 = (end.y - center.y).atan2(end.x - center.x);
        let mut sweep = (sign * (a1 - a0)).rem_euclid(std::f64::consts::TAU);
        if sweep < 1e-12 {
            sweep = std::f64::consts::TAU;
        }
        let dz = (end.z - start.z) / sweep;
        let tangent =
            |p: Point3| direction([-sign * (p.y - center.y), sign * (p.x - center.x), dz]);
        Self {
            start,
            end,
            first: tangent(start),
            last: tangent(end),
            helical: (end.z - start.z).abs() > 1e-9,
        }
    }
}

#[derive(Clone, Copy)]
struct Approach {
    start: Point3,
    end: Point3,
    direction: [f32; 3],
}

struct Entry {
    start: Point3,
    end: Point3,
    first: [f32; 3],
    plunge: bool,
    helical: bool,
    approach: Option<Approach>,
}

#[derive(Default)]
struct Markers {
    cones: Vec<PathCone>,
    owner: Option<(u32, Option<u64>, Option<u32>)>,
    approach: Option<Approach>,
    entry: Option<Entry>,
    entered: bool,
    material_seen: bool,
    exit: Option<usize>,
    departure: Option<Motion>,
}

impl Markers {
    fn finish_entry(&mut self) {
        let Some(entry) = self.entry.take() else {
            return;
        };
        // A helix's direction into the stock is its axis, not a tangent halfway
        // around one revolution. Flattened (e.g. rotary) plunges use the full run.
        let axis = if entry.plunge && entry.helical {
            direction([0., 0., entry.end.z - entry.start.z])
        } else if entry.plunge {
            along(entry.start, entry.end)
        } else {
            Some(entry.first)
        };
        let axis = axis.unwrap_or(entry.first);
        let (at, direction) = match entry.approach {
            Some(approach)
                if dot(approach.direction, axis) > 0.8
                    || (approach.end.z < approach.start.z && (!entry.plunge || axis[2] < 0.)) =>
            {
                (approach.start, approach.direction)
            }
            _ => (entry.start, axis),
        };
        self.cones.push(PathCone {
            at: point(at),
            direction,
            color: PATH_IN,
            op: self.owner.unwrap().0,
        });
        self.entered = true;
    }

    fn finish_group(&mut self) {
        self.finish_entry();
        // A standalone final lead-out is still an exit when no retract follows.
        if self.material_seen {
            if let Some(motion) = self.departure.take() {
                if let Some(direction) = motion.last {
                    self.cones.push(PathCone {
                        at: point(motion.end),
                        direction,
                        color: PATH_OUT,
                        op: self.owner.unwrap().0,
                    });
                }
            }
        }
        self.owner = None;
        self.approach = None;
        self.entered = false;
        self.material_seen = false;
        self.exit = None;
    }

    fn motion(&mut self, motion: Motion, tag: Tag, previous: Option<Tag>) {
        let owner = (tag.op_id, tag.pattern_id, tag.instance_index);
        if self.owner != Some(owner) {
            self.finish_group();
            self.owner = Some(owner);
        }
        match tag.kind {
            MoveKind::Plunge | MoveKind::LeadIn => {
                self.exit = None;
                self.departure = None;
                if !self.entered {
                    if let Some(first) = motion.first {
                        let entry = self.entry.get_or_insert_with(|| Entry {
                            start: motion.start,
                            end: motion.end,
                            first,
                            plunge: tag.kind == MoveKind::Plunge,
                            helical: motion.helical,
                            approach: self.approach.take(),
                        });
                        if tag.kind == MoveKind::Plunge || !entry.plunge {
                            entry.end = motion.end;
                            entry.helical |= motion.helical;
                        }
                        self.material_seen = true;
                    }
                }
            }
            MoveKind::Retract => {
                self.finish_entry();
                if self.material_seen || self.exit.is_some() {
                    let direction = motion.last.or_else(|| self.departure.and_then(|m| m.last));
                    if let Some(direction) = direction {
                        let cone = PathCone {
                            at: point(motion.end),
                            direction,
                            color: PATH_OUT,
                            op: tag.op_id,
                        };
                        if let Some(index) = self.exit {
                            self.cones[index] = cone;
                        } else {
                            self.exit = Some(self.cones.len());
                            self.cones.push(cone);
                        }
                    }
                }
                self.departure = None;
                self.approach = None;
                self.material_seen = false;
                self.entered = false;
            }
            MoveKind::Link => {
                self.finish_entry();
                self.exit = None;
                if !self.entered {
                    if let Some(dir) = along(motion.start, motion.end) {
                        let own_origin = previous.is_some_and(|p| {
                            (p.op_id, p.pattern_id, p.instance_index) == owner
                                && p.kind != MoveKind::Traverse
                        });
                        if own_origin {
                            if let Some(approach) = self
                                .approach
                                .as_mut()
                                .filter(|a| dot(a.direction, dir) > 0.99999)
                            {
                                approach.end = motion.end;
                            } else {
                                self.approach = Some(Approach {
                                    start: motion.start,
                                    end: motion.end,
                                    direction: dir,
                                });
                            }
                        } else {
                            self.approach = None;
                        }
                    }
                }
            }
            MoveKind::LeadOut => {
                self.finish_entry();
                self.departure = Some(motion);
                self.exit = None;
            }
            MoveKind::Cutting => {
                if !self.entered && self.entry.is_none() {
                    if let Some(first) = motion.first {
                        self.entry = Some(Entry {
                            start: motion.start,
                            end: motion.end,
                            first,
                            plunge: false,
                            helical: false,
                            approach: self.approach.take(),
                        });
                    }
                }
                self.finish_entry();
                self.material_seen = true;
                self.departure = None;
                self.exit = None;
            }
            MoveKind::Traverse => {
                self.finish_entry();
                self.approach = None;
                self.exit = None;
            }
        }
    }
}

pub(crate) fn main_path_cones(
    program: &Program,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<PathCone>> {
    let mut markers = Markers::default();
    let mut current = None;
    let mut previous = None;
    for step in program.steps() {
        if cancelled() {
            return None;
        }
        match step {
            Step::Rapid { to, tag } | Step::Linear { to, tag, .. } => {
                if let Some(start) = current {
                    markers.motion(Motion::line(start, *to), *tag, previous);
                }
                current = Some(*to);
                previous = Some(*tag);
            }
            Step::Arc {
                end,
                center,
                dir,
                tag,
                ..
            } => {
                if let Some(start) = current {
                    markers.motion(Motion::arc(start, *end, *center, *dir), *tag, previous);
                }
                current = Some(*end);
                previous = Some(*tag);
            }
            Step::Drill(cycle) => {
                markers.finish_group();
                for &[x, y] in &cycle.points {
                    for (a, b, color) in [
                        (cycle.z_top, cycle.depth, PATH_IN),
                        (cycle.depth, cycle.retract, PATH_OUT),
                    ] {
                        if let Some(direction) = direction([0., 0., b - a]) {
                            markers.cones.push(PathCone {
                                at: [
                                    x as f32,
                                    y as f32,
                                    if color == PATH_IN { a } else { b } as f32,
                                ],
                                direction,
                                color,
                                op: cycle.tag.op_id,
                            });
                        }
                    }
                }
                current = None;
                previous = None;
            }
            Step::Datum(_) | Step::Stop => {
                markers.finish_group();
                current = None;
                previous = None;
            }
            Step::ToolChange { .. } => {
                markers.finish_group();
                previous = None;
            }
            _ => {}
        }
    }
    markers.finish_group();
    Some(markers.cones)
}
