use cam_cldata::{
    ArcDir, DrillCycle, MoveKind, Point3, Program, ProgramBuilder, SpindleDir, Step, Tag,
};
use cam_model::{Envelope, Machine};
use cam_post::{
    EstlcamPost, FanucPost, GrblPost, OkumaPost, Post, PostError, PostKind, PostOptions,
};

fn machine() -> Machine {
    Machine {
        name: "test".into(),
        rapid_rate: 1000.0,
        tool_change_seconds: 0.0,
        max_spindle_rpm: 10000.0,
        max_feed: 500.0,
        envelope: Envelope::new(
            Point3::new(0.0, 0.0, -100.0),
            Point3::new(100.0, 100.0, 100.0),
        ),
        safe_z: 5.0,
        tool_change_pos: None,
    }
}
fn arc(start: Point3, end: Point3, center: Point3, dir: ArcDir) -> Program {
    ProgramBuilder::new()
        .op(17)
        .feed(100.0)
        .rapid(start, MoveKind::Link)
        .arc(end, center, dir, MoveKind::Cutting)
        .build()
}
fn cycle() -> DrillCycle {
    DrillCycle {
        points: vec![[1.0, 2.0]],
        z_top: 0.0,
        depth: -5.0,
        retract: 3.0,
        peck: Some(1.0),
        dwell: None,
        feed: 100.0,
        tag: Tag::new(9, MoveKind::Plunge),
    }
}
fn all_reject(program: &Program, machine: &Machine, options: &PostOptions) {
    for post in PostKind::ALL {
        assert!(
            post.post(program, machine, options).is_err(),
            "{post:?} accepted invalid input"
        );
    }
}

#[test]
fn every_post_rejects_nonfinite_coordinates_nonpositive_rates_and_invalid_cycles() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let p = ProgramBuilder::new()
            .feed(bad)
            .linear(Point3::new(1.0, 2.0, 3.0), MoveKind::Cutting)
            .build();
        all_reject(&p, &machine(), &PostOptions::default());
        let p = ProgramBuilder::new()
            .spindle_on(bad, SpindleDir::Cw)
            .build();
        all_reject(&p, &machine(), &PostOptions::default());
        let mut c = cycle();
        c.peck = Some(bad);
        all_reject(
            &Program {
                steps: vec![Step::Drill(c)],
            },
            &machine(),
            &PostOptions::default(),
        );
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let p = ProgramBuilder::new()
            .rapid(Point3::new(1.0, bad, 3.0), MoveKind::Link)
            .build();
        all_reject(&p, &machine(), &PostOptions::default());
    }
    let p = Program {
        steps: vec![Step::Dwell { seconds: -0.1 }],
    };
    all_reject(&p, &machine(), &PostOptions::default());
}

#[test]
fn direct_post_implementations_use_the_same_preflight() {
    let p = ProgramBuilder::new()
        .feed(-1.0)
        .linear(Point3::new(0.0, 0.0, 0.0), MoveKind::Cutting)
        .build();
    for post in [&GrblPost as &dyn Post, &FanucPost, &OkumaPost, &EstlcamPost] {
        assert!(matches!(
            post.post(&p, &machine(), &PostOptions::default()),
            Err(PostError::InvalidProgram(_))
        ));
    }
}

#[test]
fn full_circle_exceeds_travel_even_with_identical_endpoints() {
    let p = arc(
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        ArcDir::Cw,
    );
    let mut m = machine();
    m.envelope.max.x = 10.0;
    for post in PostKind::ALL {
        assert!(matches!(
            post.post(&p, &m, &PostOptions::default()),
            Err(PostError::TravelExceeded {
                axis: 'X',
                span: 40.0,
                travel: 10.0
            })
        ));
    }
}

#[test]
fn partial_arcs_use_commanded_sweep_and_negative_work_coordinates_are_allowed() {
    let mut m = machine();
    m.envelope.max.x = 10.0;
    m.envelope.max.y = 10.0;
    for post in PostKind::ALL {
        let quarter = arc(
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, -10.0),
            Point3::new(0.0, 0.0, 0.0),
            ArcDir::Ccw,
        );
        post.post(
            &quarter.translated([-123.0, -123.0, 0.0]),
            &m,
            &PostOptions::default(),
        )
        .unwrap();
        let major = arc(
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
            ArcDir::Cw,
        );
        assert!(matches!(
            post.post(&major, &m, &PostOptions::default()),
            Err(PostError::TravelExceeded { .. })
        ));
    }
}

#[test]
fn malformed_machine_limits_and_excessive_precision_fail_even_for_empty_programs() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut m = machine();
        m.max_feed = bad;
        all_reject(&Program::new(), &m, &PostOptions::default());
    }
    let opts = PostOptions {
        precision: usize::MAX,
        ..Default::default()
    };
    all_reject(&Program::new(), &machine(), &opts);
    for precision in 0..=9 {
        PostKind::Grbl
            .post(
                &Program::new(),
                &machine(),
                &PostOptions {
                    precision,
                    ..Default::default()
                },
            )
            .unwrap();
    }
}

#[test]
fn nc_rounding_cannot_zero_feed_spindle_peck_or_drill_depth() {
    let p = ProgramBuilder::new()
        .feed(0.0001)
        .linear(Point3::new(1.0, 0.0, 0.0), MoveKind::Cutting)
        .build();
    all_reject(&p, &machine(), &PostOptions::default());
    let p = ProgramBuilder::new()
        .spindle_on(0.4, SpindleDir::Cw)
        .build();
    all_reject(&p, &machine(), &PostOptions::default());
    let mut c = cycle();
    c.peck = Some(0.0001);
    all_reject(
        &Program {
            steps: vec![Step::Drill(c)],
        },
        &machine(),
        &PostOptions::default(),
    );
    let mut c = cycle();
    c.depth = -0.0001;
    all_reject(
        &Program {
            steps: vec![Step::Drill(c)],
        },
        &machine(),
        &PostOptions::default(),
    );
}

#[test]
fn nc_rounding_cannot_convert_a_partial_arc_to_a_full_circle() {
    for angle in [0.000001_f64, 0.00004] {
        let p = arc(
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 0.0),
            Point3::new(0.0, 0.0, 0.0),
            ArcDir::Ccw,
        );
        for post in PostKind::ALL {
            assert!(matches!(
                post.post(&p, &machine(), &PostOptions::default()),
                Err(PostError::UnsafeRounding { step: 1, op: 17 })
            ));
            post.post(
                &p,
                &machine(),
                &PostOptions {
                    precision: 9,
                    ..Default::default()
                },
            )
            .unwrap();
        }
    }
}

#[test]
fn nc_arc_radii_and_zero_radius_are_checked_after_rounding() {
    let p = arc(
        Point3::new(0.01, 0.0, 0.0),
        Point3::new(0.0, 0.01, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        ArcDir::Ccw,
    );
    all_reject(
        &p,
        &machine(),
        &PostOptions {
            precision: 1,
            ..Default::default()
        },
    );
    // Exact circle at raw precision, with rounded incremental I/J.
    let p = arc(
        Point3::new(1.04, 0.04, 0.0),
        Point3::new(0.04, 1.04, 0.0),
        Point3::new(0.04, 0.04, 0.0),
        ArcDir::Ccw,
    );
    // This symmetric case remains coherent and documents offset-centre semantics.
    for post in PostKind::ALL {
        post.post(
            &p,
            &machine(),
            &PostOptions {
                precision: 1,
                ..Default::default()
            },
        )
        .unwrap();
    }
    let angle = 0.5_f64;
    let p = arc(
        Point3::new(1.04, 0.04, 0.0),
        Point3::new(0.04 + angle.cos(), 0.04 + angle.sin(), 0.0),
        Point3::new(0.04, 0.04, 0.0),
        ArcDir::Ccw,
    );
    all_reject(
        &p,
        &machine(),
        &PostOptions {
            precision: 1,
            ..Default::default()
        },
    );
}

#[test]
fn rounded_travel_and_rates_must_still_fit_machine_limits() {
    let p = ProgramBuilder::new()
        .rapid(Point3::new(-0.0006, 0.0, 0.0), MoveKind::Link)
        .rapid(Point3::new(9.9996, 0.0, 0.0), MoveKind::Link)
        .build();
    let mut m = machine();
    m.envelope.max.x = 10.0005;
    all_reject(&p, &m, &PostOptions::default());
    let p = ProgramBuilder::new()
        .feed(100.0006)
        .linear(Point3::new(1.0, 0.0, 0.0), MoveKind::Cutting)
        .build();
    let mut m = machine();
    m.max_feed = 100.0007;
    all_reject(&p, &m, &PostOptions::default());
    let p = ProgramBuilder::new()
        .spindle_on(100.6, SpindleDir::Cw)
        .build();
    let mut m = machine();
    m.max_spindle_rpm = 100.7;
    all_reject(&p, &m, &PostOptions::default());
}

#[test]
fn explicit_peck_expansion_has_a_resource_limit_but_canned_cycles_remain_compact() {
    let mut c = cycle();
    c.peck = Some(0.00001);
    let p = Program {
        steps: vec![Step::Drill(c)],
    };
    let opts = PostOptions {
        precision: 6,
        ..Default::default()
    };
    for post in [PostKind::Grbl, PostKind::GrblHal, PostKind::FluidNc] {
        assert!(matches!(
            post.post(&p, &machine(), &opts),
            Err(PostError::Unsupported(_))
        ));
    }
    for post in [
        PostKind::Fanuc,
        PostKind::Haas,
        PostKind::LinuxCnc,
        PostKind::Okuma,
    ] {
        assert!(post.post(&p, &machine(), &opts).unwrap().len() < 1000);
    }
}
