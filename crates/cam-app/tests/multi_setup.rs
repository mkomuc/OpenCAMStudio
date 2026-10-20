use cam_app::{default_machine, AppController, ExportError};
use cam_geo::{Contour, Point};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};

fn profile(id: u32, depth: f64) -> Operation {
    Operation::Profile(ProfileOp {
        selections: None,
        closed: true,
        additional_chains: Vec::new(),
        tabs: Default::default(),
        spindle_rpm: 0.0,
        tool_preset: None,
        cooling: Default::default(),
        entry_feeds: Default::default(),
        work_offset: 1,
        clearing: Clearing::default(),
        id,
        tool: 1,
        chain: Contour::new(vec![
            Point::new(10.0, 10.0),
            Point::new(30.0, 10.0),
            Point::new(30.0, 30.0),
            Point::new(10.0, 30.0),
        ]),
        side: Side::Outside,
        comp: Comp::Computed,
        offset: 0.0,
        depth,
        stepdown: depth,
        stepover: 0.0,
        feed: 300.0,
        plunge_feed: 100.0,
        start: None,
        lead_in: Lead::None,
        lead_out: Lead::None,
        lead_overlap: 0.0,
        plunge: Plunge::Straight,
    })
}

fn new_app() -> AppController {
    let mut app = AppController::new(default_machine());
    app.edit_tool(0, |tool| {
        tool.diameter = 2.0;
        tool.flute_length = 12.0;
    });
    app.edit_stock(|stock| {
        *stock = Stock::FixedBox {
            size: [50.0, 50.0, 20.0],
            model_position: [0.0; 3],
        }
    });
    app
}

#[test]
fn setups_keep_independent_settings_and_operations_across_switch_save_and_history() {
    let mut app = new_app();
    let first = app.active_setup_id();
    app.edit_setup(|setup| setup.operations.push(profile(1, 2.0)));
    let mut first_settings = app.document().setup().clone();
    first_settings.name = "Top".into();
    first_settings.material = "Aluminium".into();
    first_settings.program_name = "TOP".into();
    app.apply_setup_settings(first_settings).unwrap();

    let second = app.add_setup();
    assert_ne!(first, second);
    let mut second_settings = app.document().setup().clone();
    second_settings.name = "Bottom".into();
    second_settings.material = "Steel".into();
    second_settings.program_name = "BOTTOM".into();
    second_settings.frame =
        SetupFrame::from_axes(Some([1.0, 0.0, 0.0]), Some([0.0, -1.0, 0.0]), None).unwrap();
    app.apply_setup_settings(second_settings).unwrap();
    app.edit_setup(|setup| setup.operations.push(profile(2, 4.0)));
    app.activate_setup(first).unwrap();
    assert_eq!(app.document().setup().material, "Aluminium");
    assert_eq!(app.document().setup().operations[0].id(), 1);
    app.activate_setup(second).unwrap();
    assert_eq!(app.document().setup().material, "Steel");
    assert_eq!(app.document().setup().operations[0].id(), 2);

    let project_path =
        std::env::temp_dir().join(format!("multi-setup-{}.ocam", std::process::id()));
    app.save_project(&project_path).unwrap();
    let mut reopened = new_app();
    reopened.open_project(&project_path).unwrap();
    std::fs::remove_file(project_path).unwrap();
    assert_eq!(reopened.setups().len(), 2);
    let ids: Vec<_> = reopened.setups().iter().map(|setup| setup.id).collect();
    assert_eq!(ids, vec![first, second]);
    reopened.activate_setup(second).unwrap();
    assert_eq!(reopened.document().setup().program_name, "BOTTOM");
    assert_eq!(
        reopened.document().setup().frame.world_from_setup.rotation[1][1],
        -1.0
    );
    reopened.activate_setup(first).unwrap();
    assert_eq!(reopened.document().setup().operations[0].id(), 1);

    let mut history = new_app();
    history.add_setup();
    assert_eq!(history.setups().len(), 2);
    assert!(
        history.undo(),
        "adding a setup is one document history command"
    );
    assert_eq!(history.setups().len(), 1);
    assert!(history.redo());
    assert_eq!(history.setups().len(), 2);
}

#[test]
fn switching_setups_discards_the_previous_run_and_cannot_export_its_program() {
    let mut app = new_app();
    let first = app.active_setup_id();
    app.edit_setup(|setup| setup.operations.push(profile(1, 2.0)));
    let second = app.add_setup();
    app.edit_setup(|setup| setup.operations.push(profile(2, 4.0)));

    app.activate_setup(first).unwrap();
    let first_run = app.run(&CancelToken::new());
    assert_eq!(
        first_run.completion,
        JobCompletion::Complete,
        "{:?}",
        first_run.diagnostics
    );
    let first_program = first_run.program.clone();
    let _ = app.export_nc();

    app.activate_setup(second).unwrap();
    assert!(matches!(app.export_nc(), Err(ExportError::NothingToExport)));
    let second_run = app.run(&CancelToken::new());
    assert_eq!(
        second_run.completion,
        JobCompletion::Complete,
        "{:?}",
        second_run.diagnostics
    );
    assert_ne!(second_run.program, first_program);
    let _ = app.export_nc();

    app.activate_setup(first).unwrap();
    assert!(matches!(app.export_nc(), Err(ExportError::NothingToExport)));
}

#[test]
fn duplicating_a_cad_linked_operation_assigns_a_global_id_and_copies_its_binding() {
    let mut source = new_app();
    let original_setup = source.active_setup_id();
    source.edit_setup(|setup| setup.operations.push(profile(1, 2.0)));

    // A reselection-needed binding is a valid persisted review state, even without a
    // live CAD asset. Seed it through the project wrapper to exercise the actual loader.
    let path =
        std::env::temp_dir().join(format!("multi-setup-binding-{}.ocam", std::process::id()));
    source.save_project(&path).unwrap();
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["document"]["geometry"]["bindings"] = serde_json::json!({
        "1": {
            "selected": [{
                "asset_id": 1, "revision": 1, "instance_id": 1, "body_id": 1,
                "kind": "Body", "id": 1
            }],
            "recipe": "FaceRegion",
            "heights": {"top": 0.0, "bottom": -2.0},
            "overrides": {},
            "status": {"NeedsReselection": {"reason": "test binding"}},
            "resolved_fingerprint": "old dependency fingerprint"
        }
    });
    json["document"]["cad_linked_operations"] = serde_json::json!([1]);
    std::fs::write(&path, json.to_string()).unwrap();
    let mut loaded = new_app();
    loaded.open_project(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let duplicate_setup = loaded.duplicate_setup(original_setup).unwrap();
    let duplicated = loaded
        .setups()
        .iter()
        .find(|setup| setup.id == duplicate_setup)
        .unwrap();
    let duplicated_op = duplicated.operations[0].id();
    assert_ne!(duplicated_op, 1);
    let snapshot = loaded.document();
    assert!(snapshot.geometry.bindings.contains_key(&1));
    assert!(snapshot.geometry.bindings.contains_key(&duplicated_op));
    assert!(snapshot.cad_linked_operations.contains(&1));
    assert!(snapshot.cad_linked_operations.contains(&duplicated_op));
    assert_ne!(
        snapshot.geometry.bindings[&1].resolved_fingerprint,
        snapshot.geometry.bindings[&duplicated_op].resolved_fingerprint
    );

    let roundtrip = std::env::temp_dir().join(format!(
        "multi-setup-binding-roundtrip-{}.ocam",
        std::process::id()
    ));
    loaded.save_project(&roundtrip).unwrap();
    let mut reopened = new_app();
    reopened.open_project(&roundtrip).unwrap();
    std::fs::remove_file(roundtrip).unwrap();
    assert_eq!(reopened.setups().len(), 2);
    assert!(reopened
        .document()
        .geometry
        .bindings
        .contains_key(&duplicated_op));
}
