use cam_app::{default_machine, AppController, OpKind};
use cam_cldata::{MoveKind, Step};
use cam_model::{EntityKind, EntityRef, Operation, ToolKind};
use cam_toolpath::{CancelToken, JobCompletion};

/// The supplied local CAD fixture is deliberately optional in ordinary CI.
/// Run with: cargo test -p cam-app --test parallel_project -- --ignored --nocapture
#[test]
#[ignore = "uses the supplied local testfiles/tray.ocam CAD project"]
fn tray_parallel_finishing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut app = AppController::new(default_machine());
    app.open_project(root.join("testfiles/tray.ocam")).unwrap();
    app.edit_tool(0, |t| {
        t.kind = ToolKind::BallMill;
        t.description = "6 mm ball nose - Parallel finishing".into();
    });
    let state = app.geometry_state();
    let instance = &state.instances[0];
    let asset = state.asset(instance.asset_id).unwrap();
    let reference = EntityRef {
        asset_id: asset.id,
        revision: asset.revision,
        instance_id: instance.id,
        body_id: instance.body_id,
        kind: EntityKind::Body,
        id: instance.body_id,
    };
    let id = app
        .create_cad_operation_with_tool(OpKind::Parallel, vec![reference], 1)
        .unwrap();
    app.edit_operation(id, |op| {
        let Operation::Parallel(p) = op else { panic!() };
        p.params.stepover = 1.;
        p.params.tolerance = 0.02;
        p.params.angle = 0.;
    });
    let (_, hi) = app.stock_box();
    app.edit_heights(|h| {
        h.top_of_stock = hi[2];
        h.retract = hi[2] + 3.;
        h.clearance = hi[2] + 8.;
    });
    let started = std::time::Instant::now();
    let run = app.run(&CancelToken::new());
    assert_eq!(
        run.completion,
        JobCompletion::Complete,
        "{:?}",
        run.diagnostics
    );
    assert!(run.program.validate().is_ok());
    assert!(run.collisions.is_empty(), "{:?}", run.collisions);
    let cuts: Vec<_> = run
        .program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
            _ => None,
        })
        .collect();
    assert!(cuts.len() > 100);
    let min = cuts.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let max = cuts.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
    assert!(max - min > 5., "surface following Z range: {min}..{max}");
    eprintln!(
        "tray: {} cutting moves, Z {min:.3}..{max:.3}, {:?}, diagnostics {:?}",
        cuts.len(),
        started.elapsed(),
        run.diagnostics
    );
    // A separate review project preserves the user's source fixture.
    app.save_project(root.join("testfiles/tray-parallel.ocam"))
        .unwrap();
    let mut reopened = AppController::new(default_machine());
    reopened
        .open_project(root.join("testfiles/tray-parallel.ocam"))
        .unwrap();
    assert!(matches!(reopened.operation(id),Some(Operation::Parallel(p))if p.params.stepover==1.));
}

#[test]
fn parallel_creation_uses_setup_cad_and_survives_reopening() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut app = AppController::new(default_machine());
    app.open_project(root.join("testfiles/3d_toolpath_calculation.ocam"))
        .unwrap();
    let selected = app.geometry_state().bindings[&0].selected.clone();
    let tool = app.use_tool(cam_model::Tool {
        number: 901,
        diameter: 4.,
        length: 40.,
        kind: ToolKind::BallMill,
        ..Default::default()
    });
    let id = app
        .create_cad_operation_with_tool(OpKind::Parallel, selected, tool)
        .unwrap();
    app.edit_operation(id, |op| {
        let Operation::Parallel(p) = op else { panic!() };
        p.params.angle = 37.;
        p.params.stepover = 0.7;
        p.params.both_ways = false;
    });
    assert_eq!(
        app.geometry_state().bindings[&id].recipe,
        cam_model::ExtractionRecipe::ParallelFinishing
    );
    let path = std::env::temp_dir().join(format!("parallel-roundtrip-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = AppController::new(default_machine());
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        matches!(reopened.operation(id),Some(Operation::Parallel(p))if p.params.angle==37. && p.params.stepover==0.7 && !p.params.both_ways)
    );
    assert_eq!(app.document(), reopened.document());
}
