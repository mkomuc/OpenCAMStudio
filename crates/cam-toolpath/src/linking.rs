//! Replace a clearance round trip only when its at-depth link is certified.
use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_geo::{Point, Polygon};

use crate::CancelToken;

/// Preserve all cutting/entry motions and the final retract. Only the intervening
/// approach is removed; the replacement is always fed, never a rapid in stock.
/// An entry starting above the current floor can only be shortened when it is a
/// vertical straight plunge. Sloping ramps and helices retain their original start.
pub(crate) fn keep_down(
    program: &Program,
    feed: f64,
    max_distance: f64,
    safe: impl Fn(Point, Point) -> bool,
    cancel: &CancelToken,
) -> Program {
    let steps = program.steps();
    let mut result = Program::new();
    let mut current: Option<Point3> = None;
    let mut i = 0;
    while i < steps.len() {
        if cancel.is_cancelled() {
            return Program::new();
        }
        if let (Some(from), Step::Rapid { tag, .. }) = (current, &steps[i]) {
            if tag.kind == MoveKind::Retract {
                let mut j = i + 1;
                let mut arrival = None;
                while let Some(step) = steps.get(j) {
                    match step {
                        Step::Rapid { to, tag } if tag.kind == MoveKind::Link => {
                            arrival = Some(*to);
                        }
                        Step::Linear { to, tag, .. }
                            if tag.kind == MoveKind::Link
                                && arrival.is_some_and(|p: Point3| same_xy(p, *to)) =>
                        {
                            arrival = Some(*to);
                        }
                        _ => break,
                    }
                    j += 1;
                }
                if let Some(to) = arrival {
                    // Do not splice across state changes, compensation, or another
                    // operation. The first retained motion must be a genuine entry.
                    let entry = matches!(steps.get(j), Some(Step::Linear { tag: next, .. } | Step::Arc { tag: next, .. })
                        if next.kind == MoveKind::Plunge && next.op_id == tag.op_id);
                    let straight = matches!(steps.get(j), Some(Step::Linear { to: end, .. })
                        if same_xy(to, *end) && end.z <= from.z + 1e-9);
                    let distance = (from.x - to.x).hypot(from.y - to.y);
                    if entry && ((to.z - from.z).abs() <= 1e-9 || to.z > from.z && straight) {
                        let link_tag = Tag {
                            kind: MoveKind::Link,
                            ..*tag
                        };
                        let at = Point3::new(to.x, to.y, from.z);
                        let path = if distance <= max_distance
                            && safe(Point::new(from.x, from.y), Point::new(to.x, to.y))
                        {
                            Some(if distance > 1e-9 {
                                vec![Step::Linear {
                                    to: at,
                                    feed,
                                    // Containment protects the part, but does not
                                    // certify that this shortcut is already cleared.
                                    // It must remove any remaining stock it crosses.
                                    tag: Tag {
                                        kind: MoveKind::Cutting,
                                        ..*tag
                                    },
                                }]
                            } else {
                                Vec::new()
                            })
                        } else {
                            retrace(&steps[..i], from, at, feed, link_tag, cancel)
                        };
                        if let Some(path) = path {
                            for step in path {
                                result.push(step);
                            }
                            current = Some(at);
                            i = j;
                            continue;
                        }
                    }
                }
            }
        }
        match &steps[i] {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => current = Some(*to),
            Step::Arc { end, .. } => current = Some(*end),
            _ => current = None,
        }
        result.push(steps[i].clone());
        i += 1;
    }
    result
}

/// A long or obstructed shortcut may still return along the preceding pass.
/// Reverse exact feed geometry, including arcs; stop at any lift or change of Z.
fn retrace(
    steps: &[Step],
    from: Point3,
    to: Point3,
    feed: f64,
    tag: Tag,
    cancel: &CancelToken,
) -> Option<Vec<Step>> {
    let endpoint = |s: &Step| match s {
        Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
        Step::Arc { end, .. } => Some(*end),
        _ => None,
    };
    let mut at = from;
    let mut path = Vec::new();
    for i in (1..steps.len()).rev() {
        if cancel.is_cancelled() {
            return None;
        }
        if same_xy(at, to) {
            return Some(path);
        }
        let start = endpoint(&steps[i - 1])?;
        if (start.z - from.z).abs() > 1e-9 {
            return None;
        }
        let step = match &steps[i] {
            Step::Linear { .. } => Step::Linear {
                to: start,
                feed,
                tag,
            },
            Step::Arc { center, dir, .. } => Step::Arc {
                end: start,
                center: *center,
                dir: match dir {
                    ArcDir::Cw => ArcDir::Ccw,
                    ArcDir::Ccw => ArcDir::Cw,
                },
                feed,
                tag,
            },
            _ => return None,
        };
        path.push(step);
        at = start;
    }
    same_xy(at, to).then_some(path)
}

fn same_xy(a: Point3, b: Point3) -> bool {
    (a.x - b.x).abs() <= 1e-9 && (a.y - b.y).abs() <= 1e-9
}

/// Test the entire segment, including narrow holes and concave boundaries. Split
/// at every boundary crossing and test each interval instead of sampling at a
/// fixed spacing that could miss a thin island.
pub(crate) fn contained(regions: &[Polygon], a: Point, b: Point) -> bool {
    regions.iter().any(|region| {
        if !region.contains(a) || !region.contains(b) {
            return false;
        }
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let mut splits = vec![0., 1.];
        for ring in std::iter::once(region.outer()).chain(region.holes()) {
            for (p, q) in ring
                .points()
                .iter()
                .zip(ring.points().iter().cycle().skip(1))
            {
                let ex = q.x - p.x;
                let ey = q.y - p.y;
                let cross = dx * ey - dy * ex;
                if cross.abs() > 1e-12 {
                    let t = ((p.x - a.x) * ey - (p.y - a.y) * ex) / cross;
                    let u = ((p.x - a.x) * dy - (p.y - a.y) * dx) / cross;
                    if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&u) {
                        splits.push(t);
                    }
                } else {
                    // Collinear boundary vertices also delimit containment intervals.
                    let length2 = dx * dx + dy * dy;
                    if length2 > 1e-24 {
                        for v in [p, q] {
                            let t = ((v.x - a.x) * dx + (v.y - a.y) * dy) / length2;
                            if (0. ..=1.).contains(&t) {
                                splits.push(t);
                            }
                        }
                    }
                }
            }
        }
        splits.sort_by(f64::total_cmp);
        splits.windows(2).all(|s| {
            let t = (s[0] + s[1]) * 0.5;
            region.contains(Point::new(a.x + t * dx, a.y + t * dy))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_geo::Contour;

    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Contour {
        Contour::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    }

    #[test]
    fn links_cannot_cross_even_a_thin_island_or_a_gap() {
        let region = Polygon::with_holes(
            rectangle(0., 0., 20., 20.),
            vec![rectangle(9.999, 2., 10.001, 18.)],
        )
        .unwrap();
        assert!(!contained(
            std::slice::from_ref(&region),
            Point::new(1., 10.),
            Point::new(19., 10.)
        ));
        assert!(contained(
            &[region],
            Point::new(1., 1.),
            Point::new(19., 1.)
        ));
        let separated = vec![
            Polygon::new(rectangle(0., 0., 4., 4.)).unwrap(),
            Polygon::new(rectangle(5., 0., 9., 4.)).unwrap(),
        ];
        assert!(!contained(
            &separated,
            Point::new(2., 2.),
            Point::new(7., 2.)
        ));
    }

    #[test]
    fn direct_keep_down_bridges_remove_stock_at_positive_heights() {
        use cam_sim::{simulate, SimOptions};
        // Each case starts with a new synthetic blank. The bridge crosses material
        // not swept by either adjacent pass, including a same-depth finish pass.
        for (top, floor, next_floor) in [
            (21., 15., 9.),
            (15., 9., 3.),
            (9., 3., 0.),
            (3., 0., -1.),
            (21., 15., 15.),
        ] {
            let mut p = Program::new();
            let at = |x, z| Point3::new(x, 2., z);
            let tag = |kind| Tag::new(5, kind);
            p.push(Step::Rapid {
                to: at(2., top + 5.),
                tag: tag(MoveKind::Link),
            });
            p.push(Step::Linear {
                to: at(2., floor),
                feed: 100.,
                tag: tag(MoveKind::Plunge),
            });
            p.push(Step::Linear {
                to: at(3., floor),
                feed: 300.,
                tag: tag(MoveKind::Cutting),
            });
            p.push(Step::Rapid {
                to: at(3., top + 5.),
                tag: tag(MoveKind::Retract),
            });
            p.push(Step::Rapid {
                to: at(7., top + 5.),
                tag: tag(MoveKind::Link),
            });
            p.push(Step::Linear {
                to: at(7., floor),
                feed: 300.,
                tag: tag(MoveKind::Link),
            });
            p.push(Step::Linear {
                to: at(7., next_floor),
                feed: 100.,
                tag: tag(MoveKind::Plunge),
            });
            p.push(Step::Linear {
                to: at(8., next_floor),
                feed: 300.,
                tag: tag(MoveKind::Cutting),
            });
            p.push(Step::Rapid {
                to: at(8., top + 5.),
                tag: tag(MoveKind::Retract),
            });
            let down = keep_down(&p, 300., 10., |_, _| true, &CancelToken::new());
            down.validate().unwrap();
            let sim = simulate(
                &down,
                [0., 0., -5.],
                [10., 4., top],
                &SimOptions {
                    resolution: 0.25,
                    tool_radius: 0.5,
                },
                &[],
            );
            assert!(
                sim.is_clean(),
                "floor {floor}, next {next_floor}: {:?}",
                sim.collisions
            );
            assert_eq!(
                sim.field.sample(5., 2.),
                floor as f32,
                "The bridge must remove material, not just silence verification"
            );
            assert!(matches!(down.steps().last(), Some(Step::Rapid { tag, .. })
                if tag.kind == MoveKind::Retract));
        }
    }

    #[test]
    fn retracing_cleared_lines_and_arcs_remains_noncutting() {
        use cam_sim::{simulate, SimOptions};
        let mut p = Program::new();
        let tag = |kind| Tag::new(5, kind);
        p.push(Step::Rapid {
            to: Point3::new(2., 2., 26.),
            tag: tag(MoveKind::Link),
        });
        p.push(Step::Linear {
            to: Point3::new(2., 2., 15.),
            feed: 100.,
            tag: tag(MoveKind::Plunge),
        });
        p.push(Step::Arc {
            end: Point3::new(4., 4., 15.),
            center: Point3::new(2., 4., 15.),
            dir: ArcDir::Ccw,
            feed: 300.,
            tag: tag(MoveKind::Cutting),
        });
        p.push(Step::Linear {
            to: Point3::new(8., 4., 15.),
            feed: 300.,
            tag: tag(MoveKind::Cutting),
        });
        p.push(Step::Rapid {
            to: Point3::new(8., 4., 26.),
            tag: tag(MoveKind::Retract),
        });
        p.push(Step::Rapid {
            to: Point3::new(2., 2., 26.),
            tag: tag(MoveKind::Link),
        });
        p.push(Step::Linear {
            to: Point3::new(2., 2., 15.),
            feed: 300.,
            tag: tag(MoveKind::Link),
        });
        p.push(Step::Linear {
            to: Point3::new(2., 2., 9.),
            feed: 100.,
            tag: tag(MoveKind::Plunge),
        });
        p.push(Step::Rapid {
            to: Point3::new(2., 2., 26.),
            tag: tag(MoveKind::Retract),
        });
        let down = keep_down(&p, 300., 1., |_, _| false, &CancelToken::new());
        down.validate().unwrap();
        assert!(
            matches!(&down.steps()[4..6], [Step::Linear { tag: line, .. },
            Step::Arc { tag: arc, dir: ArcDir::Cw, .. }]
            if line.kind == MoveKind::Link && arc.kind == MoveKind::Link)
        );
        let sim = simulate(
            &down,
            [0., 0., 0.],
            [10., 6., 21.],
            &SimOptions {
                resolution: 0.25,
                tool_radius: 0.5,
            },
            &[],
        );
        assert!(sim.is_clean(), "{:?}", sim.collisions);
    }

    #[test]
    fn unsafe_and_distant_links_retain_clearance() {
        let mut p = Program::new();
        p.push(Step::Linear {
            to: Point3::new(0., 0., -1.),
            feed: 300.,
            tag: Tag::new(1, MoveKind::Cutting),
        });
        p.push(Step::Rapid {
            to: Point3::new(0., 0., 5.),
            tag: Tag::new(1, MoveKind::Retract),
        });
        p.push(Step::Rapid {
            to: Point3::new(10., 0., 5.),
            tag: Tag::new(1, MoveKind::Link),
        });
        p.push(Step::Linear {
            to: Point3::new(10., 0., -1.),
            feed: 300.,
            tag: Tag::new(1, MoveKind::Link),
        });
        p.push(Step::Linear {
            to: Point3::new(10., 0., -2.),
            feed: 100.,
            tag: Tag::new(1, MoveKind::Plunge),
        });
        let token = CancelToken::new();
        assert_eq!(keep_down(&p, 300., 20., |_, _| false, &token), p);
        assert_eq!(keep_down(&p, 300., 5., |_, _| true, &token), p);
    }
}
