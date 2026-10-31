//! Qualification against the supplied modeled-chamfer project; no native SDK required.

#[path = "../src/test_fixture.rs"]
mod test_fixture;
use cam_app::{default_machine, AppController, OpKind};
use cam_cldata::{MoveKind, Point3, Step};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};

fn project() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/chamfer_test.ocam")
}
fn setup(diameter: f64, angle: f64) -> (AppController, Vec<EntityRef>, u32) {
    let mut app = AppController::new(default_machine());
    app.open_project(project())
        .expect("supplied chamfer_test.ocam project");
    let instance = &app.geometry_state().instances[0];
    let asset = app.geometry_state().asset(instance.asset_id).unwrap();
    let references = asset
        .topology
        .faces
        .iter()
        .filter(|f| {
            matches!(f.surface,
        CadSurface::Plane { normal, .. } if normal[2] > 1e-5 && normal[2] < 1. - 1e-5)
        })
        .map(|f| EntityRef {
            asset_id: asset.id,
            revision: asset.revision,
            instance_id: instance.id,
            body_id: instance.body_id,
            kind: EntityKind::Face,
            id: f.id,
        })
        .collect();
    let tool = app.use_tool(Tool {
        number: 10,
        diameter,
        length: 30.,
        flutes: 2,
        kind: ToolKind::ChamferMill {
            included_angle_deg: angle,
            tip_diameter: 1.,
        },
        ..Default::default()
    });
    (app, references, tool)
}
fn cuts(program: &cam_cldata::Program) -> Vec<(Point3, Point3)> {
    let mut previous = Point3::new(0., 0., 0.);
    let mut result = Vec::new();
    for step in program.steps() {
        match step {
            Step::Linear { to, tag, .. } => {
                if tag.kind == MoveKind::Cutting {
                    result.push((previous, *to));
                }
                previous = *to;
            }
            Step::Rapid { to, .. } => previous = *to,
            Step::Arc { end, .. } => previous = *end,
            _ => {}
        }
    }
    result
}

#[test]
fn supplied_faces_define_exact_boundaries_and_a_small_cutter_uses_multiple_bands() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, tool) = setup(6., 90.);
    assert_eq!(references.len(), 4);
    let resolved = app
        .preview_cad_operation(OpKind::Chamfer, &references)
        .unwrap();
    assert_eq!(resolved.chamfers.len(), 4);
    assert_eq!(
        resolved.heights,
        CuttingHeights {
            top: 50.,
            bottom: 40.
        }
    );
    for face in &resolved.chamfers {
        assert_eq!(face.top, 50.);
        assert_eq!(face.bottom, 40.);
        assert!((face.width() - 10.).abs() < 1e-8);
    }
    let id = app
        .create_cad_operation_with_tool(OpKind::Chamfer, references, tool)
        .unwrap();
    assert!(
        matches!(app.operation(id), Some(Operation::Chamfer(op)) if op.faces.len() == 4 && op.width == 10. && op.depth == 10.)
    );
    let heights = app.document().setup().height_settings[&id]
        .resolve(app.document(), app.operation(id).unwrap())
        .unwrap();
    assert_eq!(heights.top, 50.);
    assert_eq!(heights.bottom, 40.);
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
    let paths = cuts(&outcome.program);
    // A 6 mm cutter with a 1 mm flat has 2.5 mm of 45° cutting flank.
    assert_eq!(paths.len(), 16);
    for (face, paths) in resolved.chamfers.iter().zip(paths.as_chunks::<4>().0) {
        for (index, (start, end)) in paths.iter().enumerate() {
            let z = 50. - (index + 1) as f64 * 2.5;
            assert!((start.z - z).abs() < 1e-8 && (end.z - z).abs() < 1e-8);
            let boundary = face.at_height(z);
            for (actual, contact) in [(*start, boundary[0]), (*end, boundary[1])] {
                assert!((actual.x - contact[0] - face.outward[0] * 0.5).abs() < 1e-8);
                assert!((actual.y - contact[1] - face.outward[1] * 0.5).abs() < 1e-8);
            }
        }
    }
}

#[test]
fn requested_stepdown_and_reopened_project_keep_all_face_bounds_and_passes() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, tool) = setup(6., 90.);
    let id = app
        .create_cad_operation_with_tool(OpKind::Chamfer, references.clone(), tool)
        .unwrap();
    let mut draft = app.operation(id).unwrap().clone();
    if let Operation::Chamfer(op) = &mut draft {
        op.stepdown = 1.;
        op.feed = 321.;
    }
    app.apply_operation_settings(draft, None).unwrap();
    let before = app.run(&CancelToken::new()).program.clone();
    assert_eq!(cuts(&before).len(), 40);
    let path = std::env::temp_dir().join(format!("modeled-chamfer-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = AppController::new(default_machine());
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(app.operation(id), reopened.operation(id));
    assert_eq!(reopened.geometry_state().bindings[&id].selected, references);
    assert_eq!(reopened.run(&CancelToken::new()).program, before);
}

#[test]
fn single_face_selection_stays_local_and_angle_mismatch_is_advisory() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, tool) = setup(6., 90.);
    app.create_cad_operation_with_tool(OpKind::Chamfer, vec![references[0].clone()], tool)
        .unwrap();
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert_eq!(cuts(&outcome.program).len(), 4);
    let (mut wrong, references, tool) = setup(6., 60.);
    wrong
        .create_cad_operation_with_tool(OpKind::Chamfer, references, tool)
        .unwrap();
    let outcome = wrong.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
    assert!(!cuts(&outcome.program).is_empty());
    assert!(
        !outcome.stock_vertices.is_empty(),
        "simulation must still run"
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|d| d.severity == cam_toolpath::Severity::Warning
                && d.message.contains("included-angle cutter")),
        "{:?}",
        outcome.diagnostics
    );
}

#[test]
fn horizontal_and_vertical_faces_are_not_mistaken_for_modeled_chamfers() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (app, references, _) = setup(6., 90.);
    for id in [6, 8] {
        let mut face = references[0].clone();
        face.id = id;
        assert!(app
            .preview_cad_operation(OpKind::Chamfer, &[face])
            .unwrap_err()
            .contains("inclined chamfer face"));
    }
}

#[test]
fn moved_model_keeps_the_face_interval_and_each_pass_attached_to_the_source() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, tool) = setup(6., 90.);
    let id = app
        .create_cad_operation_with_tool(OpKind::Chamfer, references, tool)
        .unwrap();
    let before = cuts(&app.run(&CancelToken::new()).program);
    app.place_cad_instance(
        2,
        RigidTransform {
            translation: [0., 0., 7.],
            ..Default::default()
        },
    )
    .unwrap();
    let heights = app.document().setup().height_settings[&id]
        .resolve(app.document(), app.operation(id).unwrap())
        .unwrap();
    assert_eq!((heights.top, heights.bottom), (57., 47.));
    let after = cuts(&app.run(&CancelToken::new()).program);
    assert_eq!(after.len(), before.len());
    for ((a, b), (old_a, old_b)) in after.into_iter().zip(before) {
        assert!((a.z - old_a.z - 7.).abs() < 1e-8);
        assert!((b.z - old_b.z - 7.).abs() < 1e-8);
        assert!((a.x - old_a.x).abs() < 1e-8 && (a.y - old_a.y).abs() < 1e-8);
    }
}

#[test]
fn rounded_vbit_uses_its_straight_flank_and_keeps_the_nose_clear_of_the_lower_edge() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, _) = setup(6., 90.);
    let tool = app.use_tool(Tool {
        number: 20,
        diameter: 6.,
        length: 30.,
        flutes: 2,
        kind: ToolKind::VBit {
            included_angle_deg: 90.,
            tip_radius: 0.5,
        },
        ..Default::default()
    });
    let id = app
        .create_cad_operation_with_tool(OpKind::Chamfer, vec![references[0].clone()], tool)
        .unwrap();
    let Operation::Chamfer(op) = app.operation(id).unwrap() else {
        panic!("chamfer");
    };
    let face = op.faces[0].clone();
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    let nose_height = 0.5 * (1. - (45_f64.to_radians()).sin());
    let radius = 0.5 * (45_f64.to_radians()).cos();
    let paths = cuts(&outcome.program);
    assert_eq!(paths.len(), 4);
    for (start, end) in paths {
        let contacts = face.at_height(start.z + nose_height);
        for (point, contact) in [(start, contacts[0]), (end, contacts[1])] {
            assert!((point.x - contact[0] - face.outward[0] * radius).abs() < 1e-8);
            assert!((point.y - contact[1] - face.outward[1] * radius).abs() < 1e-8);
        }
    }
}

#[test]
fn replacing_an_edge_by_a_modeled_face_updates_untouched_height_defaults() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    let (mut app, references, tool) = setup(6., 90.);
    let mut rim = references[0].clone();
    rim.kind = EntityKind::Loop;
    rim.id = 50; // Boundary of the supplied model's horizontal top face.
    let id = app
        .create_cad_operation_with_tool(OpKind::Chamfer, vec![rim], tool)
        .unwrap();
    let old = app.operation(id).unwrap().clone();
    assert_eq!(
        app.document().setup().height_settings[&id]
            .definition(HeightPlaneKind::Bottom)
            .offset,
        -1.
    );
    app.apply_operation_settings_with_geometry(old, None, vec![], Some(references))
        .unwrap();
    let heights = app.document().setup().height_settings[&id]
        .resolve(app.document(), app.operation(id).unwrap())
        .unwrap();
    assert_eq!((heights.top, heights.bottom), (50., 40.));
    assert_eq!(cuts(&app.run(&CancelToken::new()).program).len(), 16);
}

#[test]
fn single_upper_and_lower_edges_accept_width_without_a_closing_cut_and_roundtrip() {
    if !test_fixture::available("chamfer_test.ocam") {
        return;
    }
    for (edge_id, top, y, side) in [
        (12, 50., -20.5, Side::Outside),
        (14, 40., -30.5, Side::Inside),
    ] {
        let (mut app, faces, tool) = setup(6., 90.);
        let edge = EntityRef {
            kind: EntityKind::Edge,
            id: edge_id,
            ..faces[0].clone()
        };
        let preview = app
            .preview_cad_operation(OpKind::Chamfer, std::slice::from_ref(&edge))
            .unwrap();
        assert!(!preview.profiles[0].closed);
        assert_eq!(preview.profiles[0].chain.len(), 2);
        assert_eq!(preview.profiles[0].side, side);
        let id = app
            .create_cad_operation_with_tool(OpKind::Chamfer, vec![edge], tool)
            .unwrap();
        let mut operation = app.operation(id).unwrap().clone();
        let Operation::Chamfer(op) = &mut operation else {
            panic!()
        };
        assert!(!op.closed);
        let mut legacy = serde_json::to_value(&*op).unwrap();
        legacy.as_object_mut().unwrap().remove("closed");
        assert!(serde_json::from_value::<ChamferOp>(legacy).unwrap().closed);
        op.width = 1.5;
        op.step = 0.5;
        app.apply_operation_settings(operation, None).unwrap();
        let outcome = app.run(&CancelToken::new());
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        let segments = cuts(&outcome.program);
        assert_eq!(segments.len(), 3, "one forward cut per width increment");
        for (i, (a, b)) in segments.iter().enumerate() {
            assert!(
                (a.y - y).abs() < 1e-8 && (b.y - y).abs() < 1e-8,
                "{segments:?}"
            );
            assert!((a.z - (top - 0.5 * (i + 1) as f64)).abs() < 1e-8);
            assert!((a.z - b.z).abs() < 1e-8);
        }
        let program = outcome.program.clone();
        let path = std::env::temp_dir().join(format!(
            "single-chamfer-edge-{edge_id}-{}.ocam",
            std::process::id()
        ));
        app.save_project(&path).unwrap();
        let mut reopened = AppController::new(default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            matches!(reopened.operation(id), Some(Operation::Chamfer(op)) if !op.closed && op.width == 1.5)
        );
        assert_eq!(reopened.run(&CancelToken::new()).program, program);
    }
}
