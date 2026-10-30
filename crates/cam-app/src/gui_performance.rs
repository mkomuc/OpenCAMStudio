//! Manual UI CPU benchmarks against the supplied local CAD project.
use super::*;
use std::time::Instant;

fn measure<T>(name: &str, mut action: impl FnMut() -> T) -> std::time::Duration {
    let started = Instant::now();
    for _ in 0..3 {
        std::hint::black_box(action());
    }
    let elapsed = started.elapsed() / 3;
    eprintln!("{name}: {elapsed:?}");
    elapsed
}

#[test]
#[ignore = "UI CPU benchmark using the supplied local basket_1.ocam project"]
fn basket_ui_cpu_benchmark() {
    let (mut app, _) = App::new();
    app.controller = AppController::new(crate::default_machine());
    app.controller
        .open_project(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/basket_1.ocam"))
        .unwrap();
    app.controller.activate_setup(4).unwrap();
    app.setup_draft = None;
    app.controller.select(Selection::Setup);
    app.refresh_fields();
    measure("setup view", || app.view());
    measure("setup viewport", || app.machining_viewport());
    app.controller.select(Selection::Operation(14));
    app.begin_operation_draft(14);
    app.inspector_enabled = true;
    app.inspector_active = true;
    measure("operation validity", || app.any_field_invalid());
    measure("operation heights", || app.resolve_height_editor(14));
    measure("operation dirty", || app.inspector_dirty());
    measure("operation viewport", || app.machining_viewport());
    measure("operation view", || app.view());
    measure("cutting feed edit", || {
        app.update(Message::FieldChanged(Field::Feed, "3001".into()))
    });
    app.operation_tab = OperationTab::Heights;
    measure("heights view", || app.view());
    for id in [13, 15, 16] {
        app.controller.select(Selection::Operation(id));
        app.begin_operation_draft(id);
        app.operation_tab = OperationTab::Tool;
        eprintln!("profile {id}");
        measure("profile viewport", || app.machining_viewport());
        measure("profile view", || app.view());
        measure("profile validity", || app.any_field_invalid());
        measure("profile field edit + view", || {
            let _ = app.update(Message::FieldChanged(Field::Feed, "3001".into()));
            drop(app.view());
        });
        app.operation_tab = OperationTab::Geometry;
        measure("profile geometry view", || app.view());
    }
}

#[test]
#[ignore = "profile creation benchmark using local basket_1.ocam Body 5"]
fn basket_body_5_profile_creation_benchmark() {
    let (mut app, _) = App::new();
    app.controller = AppController::new(crate::default_machine());
    app.controller
        .open_project(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/basket_1.ocam"))
        .unwrap();
    app.controller.activate_setup(1).unwrap();
    app.setup_draft = None;
    app.controller.select(Selection::Setup);
    app.controller.set_cad_selection(vec![]);
    app.refresh_fields();
    let _ = app.update(Message::BeginOp(OpKind::Profile));
    let reference = EntityRef {
        asset_id: 2,
        revision: 1,
        instance_id: 8,
        body_id: 5053,
        kind: EntityKind::Loop,
        id: 6126,
    };
    let start = Instant::now();
    let _ = app.update(Message::CadSelect(reference.clone()));
    eprintln!("Body 5 selection: {:?}", start.elapsed());
    assert!(!app.controller.cad_selection().is_empty(), "{}", app.status);
    app.operation_tab = OperationTab::Geometry;
    measure("Body 5 profile entries", || app.preview_profile_entries());
    measure("Body 5 creation validation", || app.creation_ready());
    assert!(app.creation_ready(), "{:?}", app.creation_operation());
    measure("Body 5 creation view", || app.view());
    let mut feed = 3000;
    measure("Body 5 feed edit + view", || {
        feed += 1;
        let _ = app.update(Message::FieldChanged(Field::Feed, feed.to_string()));
        drop(app.view());
    });
    let Operation::Profile(profile) = app.creation_operation().unwrap().0 else {
        panic!()
    };
    assert_eq!(profile.feed, f64::from(feed));

    // Exercise the named Chain editor too: its picking submenu remains open
    // while a source is selected, before the entry is accepted into the draft.
    let _ = app.update(Message::CancelOp);
    let _ = app.update(Message::BeginOp(OpKind::Profile));
    let _ = app.update(Message::ProfileEntry(
        profile_entries_view::EntryMessage::Add(cam_model::ProfileGeometryKind::Chain),
    ));
    let start = Instant::now();
    let _ = app.update(Message::CadSelect(reference));
    eprintln!("Body 5 named Chain selection: {:?}", start.elapsed());
    measure("Body 5 named Chain picker view", || app.view());
    let _ = app.update(Message::ProfileEntry(
        profile_entries_view::EntryMessage::Confirm,
    ));
    assert!(app.profile_entry_editor.is_none(), "{}", app.status);
    assert!(app.creation_ready(), "{:?}", app.creation_operation());
    measure("Body 5 named Chain creation view", || app.view());

    let _ = app.update(Message::CancelOp);
    let _ = app.update(Message::BeginOp(OpKind::Profile));
    let _ = app.update(Message::ProfileEntry(
        profile_entries_view::EntryMessage::Add(cam_model::ProfileGeometryKind::Silhouette),
    ));
    measure("Body 5 silhouette picker view", || app.view());
}
