use cam_app::{default_machine, mcp::Session, AppController, OcamFile, OpKind, ToolLibrary};
use cam_model::{CuttingEngagement, Material, Operation, OperationHeightSettings, Tool, ToolKind};
use serde_json::json;

fn tool() -> Tool {
    Tool {
        diameter: 10.,
        number: 1,
        ..Default::default()
    }
}

fn controller() -> AppController {
    let mut c = AppController::new(default_machine());
    c.edit_setup(|s| {
        s.tools = vec![tool()];
        s.material_id = Some(7);
    });
    c
}

#[test]
fn nominal_engagement_uses_strategy_semantics_and_resolved_depth() {
    let c = controller();
    let tool = tool();
    let mut profile = c.operation_template(OpKind::Profile, 1);
    if let Operation::Profile(o) = &mut profile {
        o.stepdown = 2.;
        o.stepover = 0.;
    }
    let (e, basis) = profile.nominal_cutting_engagement(&tool, 0.75).unwrap();
    assert_eq!(e.axial_depth_mm, 0.75);
    assert_eq!(e.radial_width_mm, 10.);
    assert!(basis.contains("slotting"));
    if let Operation::Profile(o) = &mut profile {
        o.stepover = 1.5;
    }
    let (e, _) = profile.nominal_cutting_engagement(&tool, 9.).unwrap();
    assert_eq!(e.axial_depth_mm, 2.);
    assert_eq!(e.radial_width_mm, 1.5);

    let mut pocket = c.operation_template(OpKind::Pocket, 1);
    if let Operation::Pocket(o) = &mut pocket {
        o.clear.stepdown = 3.;
        o.clear.overlap = 0.6;
        o.clear.clearing.engagement = 0.;
    }
    let (e, _) = pocket.nominal_cutting_engagement(&tool, 8.).unwrap();
    assert_eq!(e.axial_depth_mm, 3.);
    assert_eq!(e.radial_width_mm, 4.);
    if let Operation::Pocket(o) = &mut pocket {
        o.clear.clearing.engagement = 1.2;
    }
    assert_eq!(
        pocket
            .nominal_cutting_engagement(&tool, 8.)
            .unwrap()
            .0
            .radial_width_mm,
        1.2
    );

    let mut face = c.operation_template(OpKind::Face, 1);
    if let Operation::Face(o) = &mut face {
        o.stepdown = 2.;
        o.overlap = 0.75;
    }
    let (e, _) = face.nominal_cutting_engagement(&tool, 0.25).unwrap();
    assert_eq!(e.axial_depth_mm, 0.25);
    assert_eq!(e.radial_width_mm, 2.5);

    let mut adaptive = c.operation_template(OpKind::Adaptive, 1);
    if let Operation::Adaptive(o) = &mut adaptive {
        o.params.stepdown = 4.;
        o.params.optimal_load = 1.;
    }
    let (e, _) = adaptive.nominal_cutting_engagement(&tool, 0.5).unwrap();
    assert_eq!(e.axial_depth_mm, 0.5);
    assert_eq!(e.radial_width_mm, 1.);
}

#[test]
fn unknown_contact_and_invalid_parameters_never_produce_automatic_loads() {
    let c = controller();
    let tool = tool();
    for kind in [
        OpKind::Parallel,
        OpKind::Bore,
        OpKind::Thread,
        OpKind::Chamfer,
        OpKind::Engrave,
        OpKind::Carve,
        OpKind::Drill,
    ] {
        assert!(
            c.operation_template(kind, 1)
                .nominal_cutting_engagement(&tool, 2.)
                .is_err(),
            "{kind:?}"
        );
    }
    let mut face = c.operation_template(OpKind::Face, 1);
    if let Operation::Face(o) = &mut face {
        o.stepdown = 1.;
        o.overlap = f64::NAN;
    }
    assert!(face.nominal_cutting_engagement(&tool, 2.).is_err());
    if let Operation::Face(o) = &mut face {
        o.overlap = 0.5;
        o.stepdown = 0.;
    }
    assert!(face.nominal_cutting_engagement(&tool, 2.).is_err());
    if let Operation::Face(o) = &mut face {
        o.stepdown = 1.;
    }
    assert!(face.nominal_cutting_engagement(&tool, 0.).is_err());
    let shaped = Tool {
        kind: ToolKind::BallMill,
        ..tool
    };
    assert!(face
        .nominal_cutting_engagement(&shaped, 2.)
        .unwrap_err()
        .contains("shaped cutter"));
}

fn session() -> (Session, u32) {
    let mut session = Session {
        controller: controller(),
        ..Session::default()
    };
    session.library.materials = vec![Material {
        id: 7,
        name: "Test material".into(),
        specific_cutting_force: Some(1500.),
    }];
    let mut op = session.controller.operation_template(OpKind::Face, 1);
    if let Operation::Face(o) = &mut op {
        o.depth = 2.;
        o.stepdown = 2.;
        o.overlap = 0.7;
        o.feed = 1000.;
        o.spindle_rpm = 0.;
    }
    session.controller.add_operation(op);
    let op = session.controller.selected_operation().unwrap().clone();
    let id = op.id();
    let heights = OperationHeightSettings::legacy(session.controller.document(), &op);
    session.controller.edit_setup(|s| {
        s.height_settings.insert(id, heights);
    });
    (session, id)
}

#[test]
fn mcp_uses_setup_material_inherited_rpm_and_read_only_analysis() {
    let (mut session, id) = session();
    let before = session.controller.document().clone();
    let result = session
        .call("get_cutting_forces", json!({"operation":id}))
        .unwrap();
    let estimate: cam_model::CuttingForceEstimate =
        serde_json::from_value(result["estimate"].clone()).unwrap();
    assert_eq!(
        estimate.spindle_rpm,
        session.controller.defaults().spindle_rpm
    );
    assert!((estimate.cutting_power_w - 150.).abs() < 1e-10);
    assert_eq!(session.controller.document(), &before);
    assert!(
        session.controller.outcome().is_none(),
        "analysis must not generate toolpaths"
    );
    session
        .call(
            "set_material_cutting_force",
            json!({"id":7,"specific_cutting_force":3000}),
        )
        .unwrap();
    let doubled = session
        .call("get_cutting_forces", json!({"operation":id}))
        .unwrap();
    assert_eq!(
        doubled["estimate"]["tangential_force_n"].as_f64().unwrap(),
        estimate.tangential_force_n * 2.
    );
    assert_eq!(session.controller.document(), &before);
    assert!(session
        .call(
            "set_material_cutting_force",
            json!({"id":7,"specific_cutting_force":-1})
        )
        .is_err());
    assert_eq!(
        session.library.materials[0].specific_cutting_force,
        Some(3000.)
    );
    session
        .call("set_material_cutting_force", json!({"id":7}))
        .unwrap();
    assert!(session
        .call("get_cutting_forces", json!({"operation":id}))
        .unwrap_err()
        .contains("specific cutting force"));
    session.library.materials.clear();
    assert!(session
        .call("get_cutting_forces", json!({"operation":id}))
        .unwrap_err()
        .contains("managed stock material"));
}

#[test]
fn mcp_custom_engagement_is_validated_and_supports_geometry_dependent_milling() {
    let (mut session, _) = session();
    let mut op = session.controller.operation_template(OpKind::Parallel, 1);
    if let Operation::Parallel(o) = &mut op {
        o.params.feed = 1000.;
        o.spindle_rpm = 10000.;
    }
    session.controller.add_operation(op);
    let id = session.controller.selected_operation().unwrap().id();
    let engagement = CuttingEngagement {
        axial_depth_mm: 2.,
        radial_width_mm: 3.,
        cutting_diameter_mm: 10.,
    };
    let result = session
        .call(
            "get_cutting_forces",
            json!({"operation":id,"engagement":engagement}),
        )
        .unwrap();
    assert_eq!(result["estimate"]["cutting_power_w"], 150.);
    assert!(session.call("get_cutting_forces", json!({"operation":id,"engagement":{"axial_depth_mm":2,"radial_width_mm":11,"cutting_diameter_mm":10}})).is_err());
    let drill = session.controller.operation_template(OpKind::Drill, 1);
    session.controller.add_operation(drill);
    let id = session.controller.selected_operation().unwrap().id();
    assert!(session
        .call(
            "get_cutting_forces",
            json!({"operation":id,"engagement":engagement})
        )
        .unwrap_err()
        .contains("drill-specific"));
}

#[test]
fn library_v3_migrates_without_a_coefficient_and_v4_preserves_it() {
    let mut library = ToolLibrary::default();
    library.materials.push(Material {
        id: 7,
        name: "Steel".into(),
        specific_cutting_force: None,
    });
    let mut old = serde_json::to_value(OcamFile::Library(library.clone())).unwrap();
    old["library_version"] = 3.into();
    let OcamFile::Library(loaded) = OcamFile::from_json(&old.to_string()).unwrap() else {
        panic!()
    };
    assert_eq!(loaded.library_version, cam_app::LIBRARY_VERSION);
    assert_eq!(loaded.materials[0].specific_cutting_force, None);
    library.materials[0].specific_cutting_force = Some(1800.);
    let file = OcamFile::Library(library);
    assert_eq!(OcamFile::from_json(&file.to_json().unwrap()).unwrap(), file);
    old["materials"][0]["specific_cutting_force"] = json!(-1);
    assert!(OcamFile::from_json(&old.to_string()).is_err());
}
