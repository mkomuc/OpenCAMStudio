use cam_app::{default_machine, AppController, OpKind, Selection};
use cam_cldata::{MoveKind, Point3, ProgramBuilder, Step};
use cam_geo::{Contour, Point};
use cam_model::{Heights, Operation, RotaryWrap, Side};
use cam_post::{PostKind, PostOptions};
use cam_toolpath::{wrap_program, CancelToken, JobCompletion};

fn new_app() -> AppController {
    let mut app = AppController::new(default_machine());
    app.set_post_kind(PostKind::LinuxCnc);
    app.edit_setup(|s| {
        s.heights = Heights::new(8., 3., 0.);
        s.origin = [0.; 3];
    });
    app.edit_tool(0, |t| {
        t.diameter = 2.;
        t.flute_length = 12.;
    });
    app
}
fn profile(app: &AppController) -> Operation {
    let mut operation = app.operation_template(OpKind::Profile, 1);
    let Operation::Profile(op) = &mut operation else {
        panic!()
    };
    op.chain = Contour::new(vec![
        Point::new(5., 0.),
        Point::new(25., 0.),
        Point::new(25., 20.),
        Point::new(5., 20.),
    ]);
    op.depth = 2.;
    op.stepdown = 1.;
    op.side = Side::On;
    op.wrap = Some(RotaryWrap {
        radius: 20.,
        ..Default::default()
    });
    operation
}
#[test]
fn wrapped_job_exports_and_roundtrips_with_undo_and_duplication() {
    let mut app = new_app();
    app.add_operation(profile(&app));
    let id = app.document().setup().operations[0].id();
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert!(outcome.program.has_rotary_motion());
    assert!(outcome
        .diagnostics
        .iter()
        .any(|d| d.message.contains("not verified")));
    assert!(outcome.stock_vertices.is_empty());
    let nc = app.export_nc().unwrap().to_string();
    assert!(nc.contains("G93\n") && nc.contains(" A") && nc.contains("G94\n"));
    let path = std::env::temp_dir().join(format!("rotary-wrap-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = new_app();
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        reopened.document().setup().operations[0].rotary_wrap(),
        app.document().setup().operations[0].rotary_wrap()
    );
    app.select(Selection::Operation(id));
    app.duplicate_selected_operation();
    assert_eq!(app.document().setup().operations.len(), 2);
    assert_eq!(
        app.document().setup().operations[1].rotary_wrap(),
        app.document().setup().operations[0].rotary_wrap()
    );
    app.undo();
    assert_eq!(app.document().setup().operations.len(), 1);
    app.edit_operation(id, |op| op.set_rotary_wrap(None));
    app.undo();
    assert!(app.document().setup().operations[0].rotary_wrap().is_some());
}
#[test]
fn posts_require_rotary_capability_and_emit_feed_on_every_g93_block() {
    let source = ProgramBuilder::new()
        .feed(300.)
        .rapid(Point3::new(0., 0., 1.), MoveKind::Link)
        .linear(Point3::new(5., 10., -1.), MoveKind::Cutting)
        .build();
    let mut wrapped = wrap_program(&source, &RotaryWrap::default(), &CancelToken::new()).unwrap();
    wrapped.push(Step::Linear {
        to: Point3::new(6., 0., 24.),
        feed: 125.,
        tag: cam_cldata::Tag::new(0, MoveKind::Cutting),
    });
    for kind in PostKind::ALL {
        let result = kind.post(&wrapped, &default_machine(), &PostOptions::default());
        if !kind.supports_rotary_wrap() {
            assert!(result.is_err(), "{kind}");
            continue;
        }
        let nc = result.unwrap();
        let mut inverse = false;
        for line in nc.lines() {
            if line == "G93" {
                inverse = true;
            }
            if line == "G94" {
                inverse = false;
            }
            if inverse && line.starts_with("G1 ") {
                assert!(line.contains(" F"), "{line}");
            }
        }
        assert!(nc.contains("G94\nG1") && nc.contains("F125"), "{nc}");
    }
}
#[test]
fn pocket_supports_rotary_adaptive_clearing_and_invalid_wraps_block_export() {
    let mut app = new_app();
    let mut operation = app.operation_template(OpKind::Pocket, 1);
    let Operation::Pocket(op) = &mut operation else {
        panic!()
    };
    op.depth = 2.;
    op.clear.clearing.engagement = 0.5;
    op.wrap = Some(RotaryWrap::default());
    app.add_operation(operation);
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    assert!(app.export_nc().unwrap().contains("G93"));
    let id = app.document().setup().operations[0].id();
    app.edit_operation(id, |op| {
        let mut w = op.rotary_wrap().unwrap().clone();
        w.offset = -w.radius;
        op.set_rotary_wrap(Some(w));
    });
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Failed
    );
    assert!(app.export_nc().is_err());
}

#[test]
fn wrapped_height_references_use_the_developed_plane_with_nonzero_datum() {
    let mut app = new_app();
    let mut operation = profile(&app);
    let mut wrap = operation.rotary_wrap().unwrap().clone();
    wrap.surface_z = 4.;
    wrap.center = [0., 0., 10.];
    operation.set_rotary_wrap(Some(wrap));
    app.add_operation(operation);
    let id = app.document().setup().operations[0].id();
    app.edit_setup(|setup| {
        setup.origin = [0., 0., 10.];
        setup.height_settings.insert(id, Default::default());
    });
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    let bottom = result
        .program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Rotary {
                to, feed: Some(_), ..
            } => Some(to.z),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    assert!((bottom - 28.).abs() < 1e-9, "{bottom}");
    let nc = app.export_nc().unwrap();
    assert!(nc.contains("Z18.000"), "{nc}");
}
