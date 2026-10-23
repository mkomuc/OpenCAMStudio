//! Cutter-radius compensation is a capability: Fanuc emits `G41`/`G40`, grbl
//! (which has none) refuses the program.

use cam_cldata::{CutterComp, MoveKind, Point3, Program, ProgramBuilder, SpindleDir, Step, Tag};
use cam_model::{Envelope, Machine};
use cam_post::{FanucPost, GrblPost, Post, PostError, PostKind, PostOptions};

fn machine() -> Machine {
    Machine {
        name: "test".into(),
        rapid_rate: 2000.0,
        max_spindle_rpm: 10_000.0,
        max_feed: 800.0,
        envelope: Envelope::new(
            Point3::new(0.0, 0.0, -50.0),
            Point3::new(300.0, 180.0, 50.0),
        ),
        safe_z: 5.0,
        tool_change_pos: None,
    }
}

/// A short profile that turns cutter comp on (left, register 1), cuts, and off.
fn comp_program() -> Program {
    let tag = Tag::new(0, MoveKind::Cutting);
    let mut prog = ProgramBuilder::new()
        .spindle_on(1000.0, SpindleDir::Cw)
        .op(0)
        .feed(300.0)
        .rapid(Point3::new(10.0, 10.0, 5.0), MoveKind::Link)
        .linear(Point3::new(10.0, 10.0, -1.0), MoveKind::Plunge)
        .build();
    prog.push(Step::CutterComp(CutterComp::Left(1)));
    prog.push(Step::Linear {
        to: Point3::new(50.0, 10.0, -1.0),
        feed: 300.0,
        tag,
    });
    prog.push(Step::Linear {
        to: Point3::new(50.0, 40.0, -1.0),
        feed: 300.0,
        tag,
    });
    prog.push(Step::CutterComp(CutterComp::Off));
    prog.push(Step::SpindleOff);
    prog
}

#[test]
fn fanuc_emits_g41_and_g40() {
    let nc = FanucPost
        .post(&comp_program(), &machine(), &PostOptions::default())
        .unwrap();
    assert!(nc.contains("G41 D1"), "left comp with register 1\n{nc}");
    assert!(nc.contains("G40"), "comp cancelled\n{nc}");
}

#[test]
fn grbl_rejects_control_comp() {
    let err = GrblPost
        .post(&comp_program(), &machine(), &PostOptions::default())
        .unwrap_err();
    assert!(matches!(err, PostError::Unsupported(_)), "got {err:?}");
}

#[test]
fn fanuc_declares_the_capability_grbl_does_not() {
    assert!(FanucPost.capabilities().cutter_comp);
    assert!(!GrblPost.capabilities().cutter_comp);
}

#[test]
fn supported_posts_activate_on_the_linear_engagement_block_and_reject_rapid_engagement() {
    for post in [
        PostKind::Fanuc,
        PostKind::Haas,
        PostKind::LinuxCnc,
        PostKind::Okuma,
    ] {
        let program = comp_program();
        let nc = post
            .post(&program, &machine(), &PostOptions::default())
            .unwrap();
        let activation = nc.lines().find(|line| line.contains("G41 D1")).unwrap();
        assert!(
            activation.contains("G1") && activation.contains("X50."),
            "{post}: {activation}"
        );
        let mut bad = program;
        let index = bad
            .steps
            .iter()
            .position(|s| matches!(s, Step::CutterComp(CutterComp::Left(_))))
            .unwrap();
        bad.steps.insert(
            index + 1,
            Step::Rapid {
                to: Point3::new(11., 11., 5.),
                tag: Tag::new(0, MoveKind::Link),
            },
        );
        assert!(matches!(
            post.post(&bad, &machine(), &PostOptions::default()),
            Err(PostError::Unsupported(_))
        ));
    }
}

#[test]
fn compensation_cannot_engage_on_vertical_or_rounded_away_motion() {
    for post in [
        PostKind::Fanuc,
        PostKind::Haas,
        PostKind::LinuxCnc,
        PostKind::Okuma,
    ] {
        for to in [Point3::new(10., 10., -2.), Point3::new(10.00001, 10., -1.)] {
            let mut program = comp_program();
            let index = program
                .steps
                .iter()
                .position(|step| matches!(step, Step::CutterComp(CutterComp::Left(_))))
                .unwrap();
            program.steps[index + 1] = Step::Linear {
                to,
                feed: 300.,
                tag: Tag::new(0, MoveKind::LeadIn),
            };
            assert!(matches!(
                post.post(&program, &machine(), &PostOptions::default()),
                Err(PostError::Unsupported(_))
            ));
        }
    }
}
