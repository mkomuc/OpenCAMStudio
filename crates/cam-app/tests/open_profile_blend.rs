use cam_app::{default_machine, AppController};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
#[ignore = "requires local testfiles/wrong_z.ocam"]
fn saved_rotated_setup_profiles_edge_67_with_explicit_heights_and_reopens() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/wrong_z.ocam"),
    )
    .unwrap();
    let mut operation = app.operation(0).unwrap().clone();
    let Operation::Profile(profile) = &mut operation else {
        panic!("profile")
    };
    let mut entry = profile.selections.as_ref().unwrap()[0].clone();
    entry.name = "Chain 2".into();
    let ProfileGeometrySource::Cad(refs) = &mut entry.source else {
        panic!("CAD")
    };
    refs[0].id = 67;
    entry.chain_mode = ProfileChainMode::Open;
    profile.selections.as_mut().unwrap().push(entry.clone());
    let geometry = app
        .geometry_state()
        .geometry_in_setup(&app.document().setup().frame)
        .unwrap();
    let (curves, certified) = cam_geometry3d::profile_entry_contours(&geometry, &entry).unwrap();
    assert!(certified.is_none());
    assert!(!curves[0].closed);
    assert!(curves[0].chain.len() > 50);
    assert_eq!(curves[0].side, Side::Outside);

    app.apply_operation_settings(operation, None).unwrap();
    assert!(matches!(app.operation(0), Some(Operation::Profile(p)) if p.depth == 2.));
    app.apply_operation_settings(
        app.operation(0).unwrap().clone(),
        Some(GeometryOverrides {
            top: Some(31.),
            bottom: Some(28.),
        }),
    )
    .unwrap();
    // Explicit depth, rather than an inferred extent of the spline blend.
    app.edit_setup(|setup| {
        let settings = setup.height_settings.get_mut(&0).unwrap();
        settings.planes[HeightPlaneKind::Bottom.index()].offset = -2.;
    });
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert!(!outcome.program.is_empty());
    assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
    let cutting_z: Vec<_> = outcome
        .program
        .steps()
        .iter()
        .filter_map(|step| match step {
            cam_cldata::Step::Linear { to, tag, .. }
                if tag.kind == cam_cldata::MoveKind::Cutting =>
            {
                Some(to.z)
            }
            cam_cldata::Step::Arc { end, tag, .. } if tag.kind == cam_cldata::MoveKind::Cutting => {
                Some(end.z)
            }
            _ => None,
        })
        .collect();
    assert!((cutting_z.iter().copied().reduce(f64::min).unwrap() - 28.).abs() < 1e-8);
    assert!(cutting_z.iter().all(|z| *z <= 31. + 1e-8));
    let program = outcome.program.clone();
    let nc = app.export_nc().unwrap().to_owned();
    let path = std::env::temp_dir().join(format!("open-profile-blend-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = AppController::new(default_machine());
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(reopened.operation(0), app.operation(0));
    let outcome = reopened.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert_eq!(outcome.program, program);
    assert_eq!(reopened.export_nc().unwrap(), nc);
}
