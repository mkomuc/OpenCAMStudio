use cam_app::{default_machine, AppController, OpKind};
use cam_cldata::{MoveKind, SpindleDir, Step};
use cam_model::{EntryFeeds, Heights, Lead, Operation, Plunge, Tool, ToolKind};
use cam_toolpath::{
    CancelToken, ChamferStrategy, JobEnv, PocketStrategy, ProfileStrategy, Strategy, ThreadStrategy,
};

fn tool() -> Tool {
    Tool {
        number: 1,
        diameter: 3.,
        length: 40.,
        flute_length: 20.,
        flutes: 2,
        ..Default::default()
    }
}
fn template(kind: OpKind) -> Operation {
    let mut app = AppController::new(default_machine());
    app.edit_setup(|s| s.tools = vec![tool()]);
    app.operation_template(kind, 1)
}
fn feeds() -> EntryFeeds {
    EntryFeeds {
        ramp: Some(170.),
        lead_in: Some(210.),
        lead_out: Some(220.),
    }
}
fn motion_feeds(program: &cam_cldata::Program) -> Vec<(f64, MoveKind)> {
    program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Linear { feed, tag, .. } | Step::Arc { feed, tag, .. } => Some((*feed, tag.kind)),
            _ => None,
        })
        .collect()
}
fn run(strategy: impl Strategy, tools: &[Tool]) -> Vec<(f64, MoveKind)> {
    let result = strategy.compute(
        &JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools,
            stock: None,
            spindle: SpindleDir::Cw,
        },
        &CancelToken::new(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    motion_feeds(&result.program)
}

#[test]
fn profile_distinguishes_plunge_ramp_and_independent_entry_exit_feeds() {
    for plunge in [
        Plunge::Straight,
        Plunge::Ramp { angle_deg: 10. },
        Plunge::Helix {
            radius: 1.,
            pitch: 0.5,
        },
        Plunge::ZigZag {
            length: 2.,
            angle_deg: 10.,
        },
    ] {
        let Operation::Profile(mut op) = template(OpKind::Profile) else {
            panic!()
        };
        op.feed = 600.;
        op.plunge_feed = 100.;
        op.entry_feeds = feeds();
        op.plunge = plunge;
        op.lead_in = Lead::Linear { length: 1. };
        op.lead_out = Lead::Linear { length: 1. };
        let motions = run(ProfileStrategy::new(op), &[tool()]);
        let entry = if plunge == Plunge::Straight {
            100.
        } else {
            170.
        };
        assert!(
            motions
                .iter()
                .any(|(f, k)| *f == entry && *k == MoveKind::Plunge),
            "{plunge:?}"
        );
        assert!(motions
            .iter()
            .filter(|(_, k)| *k == MoveKind::Plunge)
            .all(|(f, _)| *f == entry));
        assert!(motions
            .iter()
            .any(|(f, k)| *f == 600. && *k == MoveKind::Cutting));
        assert!(motions.iter().any(|(f, _)| *f == 220.));
        if !matches!(plunge, Plunge::Ramp { .. }) {
            assert!(motions.iter().any(|(f, _)| *f == 210.));
        }
    }
}
#[test]
fn pocket_passes_entry_feeds_through_the_shared_clearing_engine() {
    let Operation::Pocket(mut op) = template(OpKind::Pocket) else {
        panic!()
    };
    op.entry_feeds = feeds();
    op.clear.feed = 600.;
    op.clear.plunge_feed = 100.;
    op.clear.plunge = Plunge::Straight;
    op.clear.lead_in = Lead::Linear { length: 0.25 };
    op.clear.lead_out = Lead::Linear { length: 0.25 };
    let motions = run(PocketStrategy::new(op.clone()), &[tool()]);
    for feed in [100., 210., 220., 600.] {
        assert!(
            motions.iter().any(|(f, _)| *f == feed),
            "missing {feed}: {motions:?}"
        );
    }
    op.clear.plunge = Plunge::Helix {
        radius: 0.5,
        pitch: 0.5,
    };
    let motions = run(PocketStrategy::new(op), &[tool()]);
    assert!(motions
        .iter()
        .any(|(f, k)| *f == 170. && *k == MoveKind::Plunge));
}
#[test]
fn chamfer_and_thread_have_independent_lead_feeds() {
    let Operation::Chamfer(mut op) = template(OpKind::Chamfer) else {
        panic!()
    };
    op.entry_feeds = feeds();
    op.feed = 600.;
    op.plunge_feed = 100.;
    op.lead_in = Lead::Linear { length: 1. };
    op.lead_out = Lead::Linear { length: 1. };
    let chamfer_tool = Tool {
        diameter: 10.,
        kind: ToolKind::ChamferMill {
            included_angle_deg: 90.,
            tip_diameter: 0.2,
        },
        ..tool()
    };
    let motions = run(ChamferStrategy::new(op), &[chamfer_tool]);
    for feed in [100., 210., 220., 600.] {
        assert!(motions.iter().any(|(f, _)| *f == feed));
    }
    let Operation::Thread(mut op) = template(OpKind::Thread) else {
        panic!()
    };
    op.entry_feeds = feeds();
    op.feed = 600.;
    op.plunge_feed = 100.;
    let thread_tool = Tool {
        diameter: 3.,
        neck_diameter: 1.5,
        kind: ToolKind::ThreadMill { pitch: None },
        ..tool()
    };
    let motions = run(ThreadStrategy::new(op), &[thread_tool]);
    for feed in [100., 210., 220., 600.] {
        assert!(motions.iter().any(|(f, _)| *f == feed));
    }
    // Thread helices are the cutting motion, not ramp entries.
    assert!(!motions.iter().any(|(f, _)| *f == 170.));
}
#[test]
fn entry_feed_settings_round_trip_and_invalid_rates_are_rejected() {
    let mut op = template(OpKind::Profile);
    *op.entry_feeds_mut() = feeds();
    let value = serde_json::to_value(&op).unwrap();
    assert_eq!(
        serde_json::from_value::<Operation>(value.clone()).unwrap(),
        op
    );
    let mut legacy = value;
    legacy["Profile"]
        .as_object_mut()
        .unwrap()
        .remove("entry_feeds");
    assert_eq!(
        *serde_json::from_value::<Operation>(legacy)
            .unwrap()
            .entry_feeds(),
        EntryFeeds::default()
    );
    let Operation::Profile(mut profile) = op else {
        panic!()
    };
    profile.entry_feeds.ramp = Some(-1.);
    let result = ProfileStrategy::new(profile).compute(
        &JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &[tool()],
            stock: None,
            spindle: SpindleDir::Cw,
        },
        &CancelToken::new(),
    );
    assert!(result.has_errors());
    assert!(result.program.steps().is_empty());
}

#[test]
fn independent_feeds_are_posted_as_gcode_feed_words() {
    let Operation::Profile(mut op) = template(OpKind::Profile) else {
        panic!()
    };
    op.feed = 600.;
    op.plunge_feed = 100.;
    op.entry_feeds = feeds();
    op.lead_in = Lead::Linear { length: 1. };
    op.lead_out = Lead::Linear { length: 1. };
    let result = ProfileStrategy::new(op.clone()).compute(
        &JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &[tool()],
            stock: None,
            spindle: SpindleDir::Cw,
        },
        &CancelToken::new(),
    );
    let nc = cam_post::PostKind::Grbl
        .post(&result.program, &default_machine(), &Default::default())
        .unwrap();
    for feed in ["F100", "F210", "F220", "F600"] {
        assert!(nc.contains(feed), "missing {feed}: {nc}");
    }
    op.plunge = Plunge::Ramp { angle_deg: 10. };
    let result = ProfileStrategy::new(op).compute(
        &JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &[tool()],
            stock: None,
            spindle: SpindleDir::Cw,
        },
        &CancelToken::new(),
    );
    let nc = cam_post::PostKind::Grbl
        .post(&result.program, &default_machine(), &Default::default())
        .unwrap();
    assert!(nc.contains("F170"));
}
