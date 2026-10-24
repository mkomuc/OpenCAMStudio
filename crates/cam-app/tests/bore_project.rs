//! Regression coverage for the supplied CAD project, including its rounded bore entrance.
use cam_app::{default_machine, AppController, OpKind};
use cam_cldata::{MoveKind, Step};
use cam_model::{CadSurface, EntityKind, EntityRef, Operation};
use cam_toolpath::{CancelToken, JobCompletion};

fn project() -> (AppController, EntityRef) {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/3d_toolpath_calculation.ocam"),
    )
    .unwrap();
    let geometry = app.geometry_state();
    let asset = &geometry.assets[0];
    let face = asset
        .topology
        .faces
        .iter()
        .find(|face| {
            matches!(face.surface,
                CadSurface::Cylinder { radius, inward: true, .. } if (radius - 15.).abs() < 1e-8
            )
        })
        .unwrap();
    let reference = EntityRef {
        asset_id: asset.id,
        revision: asset.revision,
        instance_id: geometry.instances[0].id,
        body_id: face.body_id,
        kind: EntityKind::Face,
        id: face.id,
    };
    (app, reference)
}

#[test]
fn supplied_project_bore_with_rounded_entrance_generates_and_posts() {
    let (mut app, face) = project();
    let original = app.document().clone();
    app.set_operation_excluded(original.setup().operations[0].id(), true);
    let id = app
        .create_cad_operation_with_tool(OpKind::Bore, vec![face], 42)
        .unwrap();
    assert!(matches!(app.operation(id), Some(Operation::Bore(b))
        if b.points == vec![[-30., 10.]] && b.diameter == 30. && b.z_top == 16. && b.z_bottom == 0.));
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    let mut cut_count = 0;
    for step in result.program.steps() {
        if let Step::Arc { end, tag, .. } = step {
            if tag.op_id == id && tag.kind == MoveKind::Cutting {
                assert!(((end.x + 30.).hypot(end.y - 10.) - 11.).abs() < 1e-8);
                assert!((0. ..=16.).contains(&end.z));
                cut_count += 1;
            }
        }
    }
    assert!(cut_count >= 34, "full 16 mm helix and bottom circle");
    assert!(app.export_nc().unwrap().contains("Bore"));
}

#[test]
fn rounded_bore_access_is_analytic_and_rejects_caps_incomplete_rims_and_obstructions() {
    use cam_model::{CadCurve, CadFace, CadMesh, ExtractionRecipe};
    let (app, reference) = project();
    let original = app.geometry_state().clone();
    let resolve = |geometry: &cam_model::GeometryState| {
        cam_geometry3d::resolve_selection(
            geometry,
            std::slice::from_ref(&reference),
            ExtractionRecipe::BoreCylinders { internal: true },
            20.,
        )
    };
    let expected = resolve(&original).unwrap();
    let mut without_mesh = original.clone();
    std::sync::Arc::make_mut(&mut without_mesh.assets[0].topology).mesh = CadMesh::default();
    assert_eq!(resolve(&without_mesh).unwrap(), expected);

    // Drill and Thread retain their existing stricter termination requirements.
    for recipe in [
        ExtractionRecipe::DrillHoles,
        ExtractionRecipe::ThreadCylinders { internal: true },
    ] {
        assert!(cam_geometry3d::resolve_selection(
            &original,
            std::slice::from_ref(&reference),
            recipe,
            20.
        )
        .is_err());
    }
    for case in [
        "noncoaxial",
        "narrowing",
        "partial",
        "capped",
        "roof",
        "rounded_bottom",
    ] {
        let mut geometry = original.clone();
        let topology = std::sync::Arc::make_mut(&mut geometry.assets[0].topology);
        match case {
            "noncoaxial" => {
                let CadSurface::Torus { origin, .. } = &mut topology
                    .faces
                    .iter_mut()
                    .find(|f| f.id == 2)
                    .unwrap()
                    .surface
                else {
                    unreachable!()
                };
                origin[0] += 1.;
            }
            "narrowing" => {
                let CadSurface::Torus { major_radius, .. } = &mut topology
                    .faces
                    .iter_mut()
                    .find(|f| f.id == 2)
                    .unwrap()
                    .surface
                else {
                    unreachable!()
                };
                *major_radius -= 1.;
            }
            "partial" => {
                let CadCurve::CircleArc {
                    start_angle,
                    end_angle,
                    ..
                } = &mut topology
                    .edges
                    .iter_mut()
                    .find(|e| e.id == 16)
                    .unwrap()
                    .curve
                else {
                    unreachable!()
                };
                *end_angle = *start_angle + std::f64::consts::PI;
            }
            "capped" => {
                for wire in &mut topology.loops {
                    if wire.face_id == 4 && wire.edge_uses.iter().any(|u| u.edge_id == 16) {
                        wire.is_outer = true;
                    }
                }
            }
            "roof" => topology.faces.push(CadFace {
                id: 1000,
                body_id: reference.body_id,
                loop_ids: Vec::new(),
                surface: CadSurface::Unsupported {
                    description: "obstruction over bore".into(),
                },
                bounds: [[-31., 9., 18.], [-29., 11., 19.]],
            }),
            "rounded_bottom" => {
                topology
                    .faces
                    .iter_mut()
                    .find(|f| f.id == 15)
                    .unwrap()
                    .surface = CadSurface::Torus {
                    origin: [-30., 10., 0.],
                    axis: [0., 0., 1.],
                    major_radius: 19.,
                    minor_radius: 4.,
                };
            }
            _ => unreachable!(),
        }
        let error = resolve(&geometry).expect_err(case);
        assert!(
            error.contains("rounded entrance")
                || error.contains("blocks vertical access")
                || error.contains("terminations need explicit qualification"),
            "{case}: {error}"
        );
    }
}
