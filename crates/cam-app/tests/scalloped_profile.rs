use cam_app::{default_machine, AppController, OpKind};
use cam_geometry3d::{profile_entry_contours, profile_side};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
#[ignore = "requires local testfiles/leiste.ocam"]
fn saved_scalloped_lower_perimeter_is_an_outside_profile() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/leiste.ocam");
    let mut app = AppController::new(default_machine());
    app.open_project(&path).unwrap();
    let project: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let geometry: GeometryState =
        serde_json::from_value(project["document"]["geometry"].clone()).unwrap();
    let frame: SetupFrame =
        serde_json::from_value(project["document"]["setups"][0]["frame"].clone()).unwrap();
    let state = geometry.geometry_in_setup(&frame).unwrap();
    let selected = EntityRef {
        asset_id: 1,
        revision: 1,
        instance_id: 1,
        body_id: 1,
        kind: EntityKind::Loop,
        id: 127,
    };
    assert_eq!(
        profile_side(&state, &[selected.clone()]).unwrap(),
        Side::Outside
    );
    let mut entry = ProfileGeometryEntry::new(ProfileGeometryKind::Chain, "Chain 1".into());
    entry.source = ProfileGeometrySource::Cad(vec![selected.clone()]);
    let (contours, heights) = profile_entry_contours(&state, &entry).unwrap();
    assert_eq!(contours.len(), 1);
    assert!(contours[0].closed);
    assert_eq!(contours[0].side, Side::Outside);
    assert!(contours[0].chain.len() > 100);
    let heights = heights.unwrap();
    assert!((heights.top - 73.).abs() < 1e-5);
    assert!((heights.bottom - 55.).abs() < 1e-5);
    let id = app
        .create_cad_operation(OpKind::Profile, vec![selected])
        .unwrap();
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("below the clearance plane")),
        "{:?}",
        result.diagnostics
    );
    assert!(!result.program.is_empty());
    assert!(matches!(app.operation(id), Some(Operation::Profile(p)) if p.side == Side::Outside));
}

#[test]
#[ignore = "requires local testfiles/leiste.ocam"]
fn saved_project_heights_match_the_workpiece_datum_in_nc() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/leiste.ocam");
    let mut app = AppController::new(default_machine());
    app.open_project(path).unwrap();
    let doc = app.document();
    let setup = doc.setup();
    let op = &setup.operations[0];
    let datum = setup.origin_position(op.work_offset())[2];
    assert!((datum - 54.).abs() < 1e-5);
    let heights = setup.height_settings[&op.id()].resolve(doc, op).unwrap();
    for (actual, expected) in [
        (heights.clearance - datum, 35.),
        (heights.retract - datum, 25.),
        (heights.feed.unwrap() - datum, 25.),
        (heights.top - datum, 20.),
        (heights.bottom - datum, 1.),
    ] {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("below the clearance plane")),
        "{:?}",
        result.diagnostics
    );
    // The saved spindle/feed and part width exceed the default test machine;
    // use compatible limits to verify coordinate output.
    let mut machine = app.machine().clone();
    machine.max_spindle_rpm = 24_000.;
    machine.max_feed = 2_000.;
    machine.envelope.max.x = 600.;
    app.set_machine(machine);
    app.run(&CancelToken::new());
    let nc = app.export_nc().unwrap();
    let zs: Vec<f64> = nc
        .lines()
        .flat_map(str::split_whitespace)
        .filter_map(|word| word.strip_prefix('Z')?.parse().ok())
        .collect();
    assert!(!zs.is_empty());
    assert!(
        (zs.iter().copied().reduce(f64::min).unwrap() - 1.).abs() < 1e-5,
        "{zs:?}"
    );
    assert!(
        (zs.iter().copied().reduce(f64::max).unwrap() - 50.).abs() < 1e-5,
        "{zs:?}"
    );
}
