use cam_app::{default_machine, AppController};
use cam_cldata::{MoveKind, Step};
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
fn adaptive_simplification_survives_project_reopening() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    ).unwrap();
    let id = app.document().setup().operations[0].id();
    assert!(matches!(app.operation(id), Some(cam_model::Operation::Adaptive(op))
        if op.params.simplification_tolerance == 0.02));
    let original = app.run(&CancelToken::new());
    assert_eq!(original.completion, JobCompletion::Complete, "{:?}", original.diagnostics);
    let original_steps = original.program.steps().to_vec();
    app.edit_operation(id, |op| {
        let cam_model::Operation::Adaptive(op) = op else { panic!("adaptive fixture") };
        op.params.simplification_tolerance = 0.005;
    });
    let edited = app.run(&CancelToken::new());
    assert_eq!(edited.completion, JobCompletion::Complete, "{:?}", edited.diagnostics);
    assert_ne!(edited.program.steps(), original_steps, "editing simplification must invalidate the cached path");
    let path = std::env::temp_dir().join(format!("opencam-simplification-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = AppController::new(default_machine());
    let result = reopened.open_project(&path);
    std::fs::remove_file(&path).unwrap();
    result.unwrap();
    assert_eq!(reopened.document(), app.document());
    assert!(matches!(reopened.operation(id), Some(cam_model::Operation::Adaptive(op))
        if op.params.simplification_tolerance == 0.005));
}

#[test]
fn supplied_3d_toolpath_project_calculates() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    )
    .unwrap();
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    assert!(result.program.steps().iter().any(|step| matches!(
        step,
        Step::Linear { tag, .. } | Step::Arc { tag, .. }
            if tag.kind == MoveKind::Cutting
    )));

    // Rejected frontier segments must not turn into repeated down/up cycles
    // with no cut between them (the supplied project used to contain 3,272).
    let mut cut_since_retract = false;
    let mut previous_z: Option<f64> = None;
    let mut round_pocket_retracts = std::collections::BTreeMap::new();
    for step in result.program.steps() {
        match step {
            Step::Linear { tag, .. } | Step::Arc { tag, .. }
                if tag.kind == MoveKind::Cutting =>
            {
                cut_since_retract = true;
            }
            Step::Rapid { to, tag } if tag.kind == MoveKind::Retract => {
                assert!(cut_since_retract, "empty plunge/retract cycle: {step:?}");
                cut_since_retract = false;
                // This fixture's round pocket is centred at (-30, 10), with
                // a 15 mm bore and a rounded entrance reaching radius 19 mm.
                if (to.x + 30.).hypot(to.y - 10.) < 20. {
                    let depth = (previous_z.unwrap() * 1000.).round() as i64;
                    *round_pocket_retracts.entry(depth).or_insert(0usize) += 1;
                }
            }
            _ => {}
        }
        match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => previous_z = Some(to.z),
            Step::Arc { end, .. } => previous_z = Some(end.z),
            _ => {}
        }
    }
    assert!(round_pocket_retracts.len() > 20, "the bore must still be machined through its depth");
    for (depth, retracts) in round_pocket_retracts {
        assert_eq!(retracts, 1, "round pocket must stay down within the pass at Z{}", depth as f64 / 1000.);
    }
}
