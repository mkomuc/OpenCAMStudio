use cam_app::{default_machine, AppController};
use cam_cldata::{MoveKind, Step};
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
fn test2_face_selection_avoids_excessive_adaptive_retracts() {
    let mut app = AppController::new(default_machine());
    // test2 uses the same CAD asset as the tracked fixture. Recreate its
    // adaptive operation without depending on its large local toolpath cache.
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    )
    .unwrap();
    let mut face = app.geometry_state().bindings[&0].selected[0].clone();
    face.kind = cam_model::EntityKind::Face;
    let faces = [2, 10].map(|id| cam_model::EntityRef { id, ..face.clone() });
    app.edit_cad_geometry(0, faces.to_vec()).unwrap();
    app.edit_operation(0, |op| {
        let cam_model::Operation::Adaptive(op) = op else {
            panic!("adaptive fixture")
        };
        // The fixture already contains test2's 6 mm end mill as tool 37.
        op.tool = 37;
        op.params.containment = cam_model::AdaptiveContainment::Outside;
    });
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    assert!(result.program.validate().is_ok());
    assert!(result.collisions.is_empty(), "{:?}", result.collisions);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("concentric clearing")));
    let mut previous_z = None;
    let mut lifts = 0;
    for step in result.program.steps() {
        match step {
            Step::Rapid { to, .. } => {
                if previous_z.is_some_and(|z| to.z > z + 1e-6) {
                    lifts += 1;
                }
                previous_z = Some(to.z);
            }
            Step::Linear { to, .. } => previous_z = Some(to.z),
            Step::Arc { end, .. } => previous_z = Some(end.z),
            _ => {}
        }
    }
    // Count upward rapid motion regardless of its tag: ring-to-ring lifts use
    // Link, while frontier restarts use Retract. This operation formerly had 294.
    assert!(
        (1..=45).contains(&lifts),
        "excessive adaptive clearance lifts: {lifts}"
    );
}

#[test]
fn adaptive_tolerance_drives_simplification_and_survives_project_reopening() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    )
    .unwrap();
    let id = app.document().setup().operations[0].id();
    assert!(
        matches!(app.operation(id), Some(cam_model::Operation::Adaptive(op))
        if op.params.simplification_tolerance() == 0.025)
    );
    let original = app.run(&CancelToken::new());
    assert_eq!(
        original.completion,
        JobCompletion::Complete,
        "{:?}",
        original.diagnostics
    );
    let original_steps = original.program.steps().to_vec();
    app.edit_operation(id, |op| {
        let cam_model::Operation::Adaptive(op) = op else {
            panic!("adaptive fixture")
        };
        op.params.tolerance = 0.025;
    });
    let edited = app.run(&CancelToken::new());
    assert_eq!(
        edited.completion,
        JobCompletion::Complete,
        "{:?}",
        edited.diagnostics
    );
    assert_ne!(
        edited.program.steps(),
        original_steps,
        "editing tolerance must invalidate the cached path"
    );
    let path = std::env::temp_dir().join(format!(
        "opencam-simplification-{}.ocam",
        std::process::id()
    ));
    app.save_project(&path).unwrap();
    let mut reopened = AppController::new(default_machine());
    let result = reopened.open_project(&path);
    std::fs::remove_file(&path).unwrap();
    result.unwrap();
    assert_eq!(reopened.document(), app.document());
    assert!(
        matches!(reopened.operation(id), Some(cam_model::Operation::Adaptive(op))
        if op.params.simplification_tolerance() == 0.00625)
    );
}

#[test]
fn supplied_3d_project_stays_down_at_002_simplification() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    )
    .unwrap();
    // Keep this path-shape regression at its original 0.02 mm simplification.
    // The test above separately calculates the supplied default tolerance and
    // verifies that changing tolerance invalidates the resulting path.
    let id = app.document().setup().operations[0].id();
    app.edit_operation(id, |op| {
        let cam_model::Operation::Adaptive(op) = op else {
            panic!("adaptive fixture")
        };
        op.params.tolerance = 0.08;
        assert_eq!(op.params.simplification_tolerance(), 0.02);
    });
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
            Step::Linear { tag, .. } | Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting => {
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
    for depth in (1000..=19000).step_by(2000) {
        assert!(
            round_pocket_retracts.contains_key(&depth),
            "primary roughing level Z{} missing: {round_pocket_retracts:?}",
            depth as f64 / 1000.
        );
    }
    assert!(
        round_pocket_retracts.len() < 20,
        "unchanged bore walls must not repeat all fine levels: {round_pocket_retracts:?}"
    );
    for depth in round_pocket_retracts.keys().copied().filter(|z| *z < 16000) {
        // Keep primary passes and the final reachable floor refinement. Fine
        // passes for the exterior slope must not repeat the vertical bore.
        assert!(
            depth == 0 || depth == 500 || (depth - 1000) % 2000 == 0,
            "redundant fine bore pass at Z{}",
            depth as f64 / 1000.
        );
    }
    for (depth, retracts) in round_pocket_retracts {
        assert_eq!(
            retracts,
            1,
            "round pocket must stay down within the pass at Z{}",
            depth as f64 / 1000.
        );
    }
}
