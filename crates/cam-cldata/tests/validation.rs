use cam_cldata::{
    ArcDir, DrillCycle, MoveKind, Point3, Program, ProgramBuilder, SpindleDir, Step, Tag,
    ValidationKind,
};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn arc(start: Point3, end: Point3, center: Point3, dir: ArcDir) -> Program {
    ProgramBuilder::new()
        .op(42)
        .feed(100.0)
        .rapid(start, MoveKind::Link)
        .arc(end, center, dir, MoveKind::Cutting)
        .build()
}

#[test]
fn bounds_cover_full_circles_and_helical_z() {
    for dir in [ArcDir::Cw, ArcDir::Ccw] {
        let b = arc(
            Point3::new(25.0, -7.0, 2.0),
            Point3::new(25.0, -7.0, -8.0),
            Point3::new(5.0, -7.0, 99.0),
            dir,
        )
        .validate()
        .unwrap()
        .unwrap();
        assert_eq!(b.min, Point3::new(-15.0, -27.0, -8.0));
        assert_eq!(b.max, Point3::new(25.0, 13.0, 2.0));
    }
}

#[test]
fn bounds_follow_direction_and_do_not_include_unswept_quadrants() {
    let start = Point3::new(10.0, 0.0, 0.0);
    let end = Point3::new(0.0, 10.0, 0.0);
    let center = Point3::new(0.0, 0.0, 0.0);
    let quarter = arc(start, end, center, ArcDir::Ccw)
        .validate()
        .unwrap()
        .unwrap();
    assert_eq!(quarter.min, center);
    assert_eq!(quarter.max, Point3::new(10.0, 10.0, 0.0));
    let major = arc(start, end, center, ArcDir::Cw)
        .validate()
        .unwrap()
        .unwrap();
    assert_eq!(major.min, Point3::new(-10.0, -10.0, 0.0));
    assert_eq!(major.max, Point3::new(10.0, 10.0, 0.0));
}

#[test]
fn analytic_bounds_enclose_independently_sampled_arcs() {
    for dir in [ArcDir::Cw, ArcDir::Ccw] {
        for angle in [-PI, -0.7, 0.0, FRAC_PI_2, 3.0] {
            for sweep in [0.03, FRAC_PI_2, PI, 4.7, TAU] {
                let center = Point3::new(-123.0, 17.0, 0.0);
                let sign = if dir == ArcDir::Ccw { 1.0 } else { -1.0 };
                let sample = |t: f64| {
                    Point3::new(
                        center.x + 7.0 * (angle + sign * sweep * t).cos(),
                        center.y + 7.0 * (angle + sign * sweep * t).sin(),
                        5.0 - 13.0 * t,
                    )
                };
                let start = sample(0.0);
                let end = if sweep == TAU {
                    Point3::new(start.x, start.y, -8.0)
                } else {
                    sample(1.0)
                };
                let b = arc(start, end, center, dir).validate().unwrap().unwrap();
                for i in 0..=2000 {
                    let p = sample(i as f64 / 2000.0);
                    for (v, lo, hi) in [
                        (p.x, b.min.x, b.max.x),
                        (p.y, b.min.y, b.max.y),
                        (p.z, b.min.z, b.max.z),
                    ] {
                        assert!(
                            v >= lo - 1e-10 && v <= hi + 1e-10,
                            "{dir:?} {angle} {sweep}: {v} outside {lo}..{hi}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_numeric_steps_retain_source_provenance() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let p = ProgramBuilder::new()
            .op(42)
            .rapid(Point3::new(value, 0.0, 0.0), MoveKind::Link)
            .build();
        let e = p.validate().unwrap_err();
        assert_eq!((e.step, e.op), (0, Some(42)));
        assert!(matches!(
            e.kind,
            ValidationKind::NonFinite { field: "X", .. }
        ));
    }
    for feed in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let p = ProgramBuilder::new()
            .op(7)
            .feed(feed)
            .linear(Point3::new(1.0, 2.0, 3.0), MoveKind::Cutting)
            .build();
        assert_eq!(p.validate().unwrap_err().op, Some(7));
    }
    for rpm in [0.0, -10.0, f64::INFINITY] {
        assert!(ProgramBuilder::new()
            .spindle_on(rpm, SpindleDir::Cw)
            .build()
            .validate()
            .is_err());
    }
    for seconds in [-1.0, f64::NAN] {
        assert!(Program {
            steps: vec![Step::Dwell { seconds }]
        }
        .validate()
        .is_err());
    }
    assert_eq!(
        Program {
            steps: vec![Step::Dwell { seconds: 0.0 }]
        }
        .validate(),
        Ok(None)
    );
}

#[test]
fn arcs_need_a_valid_anchor_radius_and_datum() {
    let mut p = arc(
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 9.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        ArcDir::Ccw,
    );
    assert!(matches!(
        p.validate().unwrap_err().kind,
        ValidationKind::ArcRadiusMismatch { .. }
    ));
    p.steps.remove(0);
    assert_eq!(
        p.validate().unwrap_err().kind,
        ValidationKind::ArcWithoutStart
    );
    p.steps.insert(0, Step::Datum(0));
    assert_eq!(
        p.validate().unwrap_err().kind,
        ValidationKind::InvalidDatum(0)
    );
    let mut p = arc(
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 10.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        ArcDir::Ccw,
    );
    p.steps.insert(1, Step::Datum(2));
    assert_eq!(
        p.validate().unwrap_err().kind,
        ValidationKind::ArcWithoutStart
    );
    assert!(arc(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        ArcDir::Cw
    )
    .validate()
    .is_err());
}

fn cycle() -> DrillCycle {
    DrillCycle {
        points: vec![[1.0, 2.0], [10.0, 0.0]],
        z_top: 0.0,
        depth: -5.0,
        retract: 3.0,
        peck: Some(1.0),
        dwell: Some(0.0),
        feed: 100.0,
        tag: Tag::new(9, MoveKind::Plunge),
    }
}

#[test]
fn drill_validation_covers_parameters_points_bounds_and_final_anchor() {
    let c = cycle();
    let mut p = Program {
        steps: vec![Step::Drill(c.clone())],
    };
    let b = p.validate().unwrap().unwrap();
    assert_eq!(b.min, Point3::new(1.0, 0.0, -5.0));
    assert_eq!(b.max, Point3::new(10.0, 2.0, 3.0));
    p.push(Step::Arc {
        end: Point3::new(0.0, 10.0, 3.0),
        center: Point3::new(0.0, 0.0, 3.0),
        dir: ArcDir::Ccw,
        feed: 100.0,
        tag: c.tag,
    });
    p.validate().unwrap();
    let cases: [fn(&mut DrillCycle); 10] = [
        |c| c.depth = c.z_top,
        |c| c.retract = -1.0,
        |c| c.peck = Some(0.0),
        |c| c.peck = Some(-1.0),
        |c| c.peck = Some(f64::NAN),
        |c| c.dwell = Some(-1.0),
        |c| c.feed = 0.0,
        |c| c.points[0][1] = f64::INFINITY,
        |c| c.depth = f64::NEG_INFINITY,
        |c| {
            c.z_top = 1e20;
            c.retract = 1e20;
            c.depth = 0.0;
            c.peck = Some(1.0);
        },
    ];
    for mutate in cases {
        let mut c = cycle();
        mutate(&mut c);
        assert_eq!(
            Program {
                steps: vec![Step::Drill(c)]
            }
            .validate()
            .unwrap_err()
            .op,
            Some(9)
        );
    }
}
