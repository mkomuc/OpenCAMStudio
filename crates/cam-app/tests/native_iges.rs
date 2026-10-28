//! These tests require the optional OCCT SDK and run with `--features occt`.
#![cfg(feature = "occt")]
use cam_app::{default_machine, AppController, OpKind};
use cam_model::{CadFormat, CadSurface, EntityKind, EntityRef, SourceUnits, Stock};
use cam_toolpath::{CancelToken, JobCompletion};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cam-kernel/fixtures")
        .join(name)
}

fn new_app() -> AppController {
    let mut app = AppController::new(default_machine());
    app.edit_tool(0, |tool| {
        tool.diameter = 3.;
        tool.flute_length = 12.;
    });
    app.edit_stock(|stock| {
        *stock = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 0.,
            thickness: 10.,
        }
    });
    app
}

#[test]
fn iges_aliases_machine_and_reopen_without_source() {
    for (extension, name, units) in [
        ("iges", "box-inch.igs", SourceUnits::Inches),
        ("igs", "box-surfaces-mm.igs", SourceUnits::Millimetres),
        ("IGES", "box-m.iges", SourceUnits::Metres),
        ("IGS", "box-hole-mm.iges", SourceUnits::Millimetres),
    ] {
        let source =
            std::env::temp_dir().join(format!("native-iges-{}.{extension}", std::process::id()));
        let bytes = std::fs::read(fixture(name)).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        let mut app = new_app();
        assert_eq!(app.import_cad(&source).unwrap(), 1);
        std::fs::remove_file(&source).unwrap();
        let asset = &app.geometry_state().assets[0];
        let instance = &app.geometry_state().instances[0];
        assert_eq!(asset.source_format, CadFormat::Iges);
        assert_eq!(asset.source_units, units);
        assert_eq!(&*asset.source_data, &bytes);
        let top = asset.topology.faces.iter().find(|face| matches!(face.surface,
            CadSurface::Plane { origin, normal } if origin[2].abs() < 1e-6 && normal[2] > 0.99
        )).unwrap();
        let outer = asset
            .topology
            .loops
            .iter()
            .find(|wire| wire.face_id == top.id && wire.is_outer)
            .unwrap();
        let selected = EntityRef {
            asset_id: asset.id,
            revision: asset.revision,
            instance_id: instance.id,
            body_id: instance.body_id,
            kind: EntityKind::Loop,
            id: outer.id,
        };
        let instance_id = instance.id;
        app.edit_setup(|setup| setup.model_instances = vec![instance_id]);
        app.create_cad_operation(OpKind::Profile, vec![selected])
            .unwrap();
        let outcome = app.run(&CancelToken::new());
        assert_eq!(
            outcome.completion,
            JobCompletion::Complete,
            "{:?}",
            outcome.diagnostics
        );
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        assert!(!outcome.program.is_empty());
        let program = outcome.program.clone();
        let nc = app.export_nc().unwrap().to_owned();
        let project = source.with_extension("ocam");
        app.save_project(&project).unwrap();
        let mut reopened = new_app();
        reopened.open_project(&project).unwrap();
        std::fs::remove_file(project).unwrap();
        assert_eq!(
            reopened.geometry_state().assets[0].source_format,
            CadFormat::Iges
        );
        assert_eq!(reopened.run(&CancelToken::new()).program, program);
        assert_eq!(reopened.export_nc().unwrap(), nc);
    }
}

#[test]
fn iges_can_replace_an_existing_solid_through_the_background_import() {
    let mut app = new_app();
    app.import_cad(fixture("box-mm.step")).unwrap();
    let id = app.geometry_state().assets[0].id;
    app.start_cad_replacement(id, fixture("box-mm.iges"))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(result) = app.poll_cad_import() {
            assert_eq!(result.unwrap(), 1);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "import timed out");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(app.commit_cad_preview().unwrap(), 1);
    let asset = &app.geometry_state().assets[0];
    assert_eq!(asset.id, id);
    assert_eq!(asset.revision, 2);
    assert_eq!(asset.source_format, CadFormat::Iges);
}

#[test]
fn spline_bore_is_usable_and_survives_save_reopen() {
    for name in [
        "spline-hole-mm.iges",
        "elevated-spline-hole-mm.iges",
        "elevated-spline-hole-mm.step",
    ] {
        let mut app = new_app();
        app.import_cad(fixture(name)).unwrap();
        let state = app.geometry_state();
        let asset = &state.assets[0];
        let instance = &state.instances[0];
        let face = asset
            .topology
            .faces
            .iter()
            .find(|face| matches!(face.surface, CadSurface::Cylinder { inward: true, .. }))
            .unwrap();
        let reference = EntityRef {
            asset_id: asset.id,
            revision: asset.revision,
            instance_id: instance.id,
            body_id: face.body_id,
            kind: EntityKind::Face,
            id: face.id,
        };
        for recipe in [
            cam_model::ExtractionRecipe::BoreCylinders { internal: true },
            cam_model::ExtractionRecipe::ThreadCylinders { internal: true },
            cam_model::ExtractionRecipe::DrillHoles,
        ] {
            assert_eq!(
                cam_geometry3d::verify_selection(state, &[reference.clone()], recipe, 0.),
                None
            );
        }
        let instance_id = instance.id;
        app.edit_setup(|setup| setup.model_instances = vec![instance_id]);
        let id = app
            .create_cad_operation(OpKind::Bore, vec![reference])
            .unwrap();
        let binding = app.geometry_state().bindings[&id].clone();
        assert!(binding.heights.top.abs() < 1e-5);
        assert!((binding.heights.bottom + 10.).abs() < 1e-5);
        let input = cam_geometry3d::resolve_binding(app.geometry_state(), &binding).unwrap();
        assert!((input.holes[0].diameter - 5.).abs() < 1e-9);
        let project = std::env::temp_dir().join(format!("spline-bore-{}.ocam", std::process::id()));
        app.save_project(&project).unwrap();
        let mut reopened = new_app();
        reopened.open_project(&project).unwrap();
        std::fs::remove_file(project).unwrap();
        let reopened_input = cam_geometry3d::resolve_binding(
            reopened.geometry_state(),
            &reopened.geometry_state().bindings[&id],
        )
        .unwrap();
        assert_eq!(input, reopened_input);
    }
}

#[test]
fn exact_conic_boundaries_are_usable_as_profile_geometry() {
    for kind in ["ellipse", "parabola", "hyperbola"] {
        for ext in ["step", "iges"] {
            let name = format!("{kind}-prism-mm.{ext}");
            let mut app = new_app();
            app.import_cad(fixture(&name)).unwrap();
            let state = app.geometry_state();
            let asset = &state.assets[0];
            let instance = &state.instances[0];
            let face = asset
                .topology
                .faces
                .iter()
                .find(|face| {
                    matches!(face.surface,
                CadSurface::Plane { origin, normal } if origin[2].abs() < 1e-5 && normal[2] > 0.99)
                })
                .unwrap();
            let reference = EntityRef {
                asset_id: asset.id,
                revision: asset.revision,
                instance_id: instance.id,
                body_id: face.body_id,
                kind: EntityKind::Face,
                id: face.id,
            };
            let binding = cam_geometry3d::resolve_selection_advisory(
                state,
                &[reference],
                cam_model::ExtractionRecipe::ProfileContours,
                0.,
            )
            .unwrap();
            let input = cam_geometry3d::resolve_binding_advisory(state, &binding).unwrap();
            assert_eq!(input.profiles.len(), 1, "{name}");
            assert!(input.profiles[0].closed, "{name}");
            assert!(input.profiles[0].chain.len() > 3, "{name}");
        }
    }
}

#[test]
#[ignore = "requires the locally supplied testfiles/ColdEnd.iges"]
fn supplied_cold_end_spline_bore_is_recognized() {
    let mut app = new_app();
    app.edit_stock(|stock| {
        *stock = Stock::BoundingBox { x_offset: 0., y_offset: 0., top: 18., thickness: 18. };
    });
    let cutter = app.use_tool(cam_model::Tool {
        number: 3,
        diameter: 3.,
        shank_diameter: 6.,
        flute_length: 8.,
        length: 57.,
        flutes: 3,
        kind: cam_model::ToolKind::EndMill,
        ..Default::default()
    });
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/ColdEnd.iges");
    app.import_cad(source).unwrap();
    let state = app.geometry_state();
    let asset = &state.assets[0];
    let topology = &asset.topology;
    assert_eq!(
        topology
            .faces
            .iter()
            .filter(|face| matches!(face.surface, CadSurface::Cylinder { .. }))
            .count(),
        22
    );
    let face = topology.faces.iter().find(|face| face.id == 343).unwrap();
    assert!(
        matches!(face.surface, CadSurface::Cylinder { radius, inward: true, .. } if (radius - 2.1).abs() < 1e-9),
        "{face:?}"
    );
    let instance = state
        .instances
        .iter()
        .find(|instance| instance.body_id == face.body_id)
        .unwrap();
    let reference = EntityRef {
        asset_id: asset.id,
        revision: asset.revision,
        instance_id: instance.id,
        body_id: face.body_id,
        kind: EntityKind::Face,
        id: face.id,
    };
    let instance_id = instance.id;
    app.edit_setup(|setup| setup.model_instances = vec![instance_id]);
    let id = app
        .create_cad_operation_with_tool(OpKind::Bore, vec![reference], cutter)
        .unwrap();
    let binding = &app.geometry_state().bindings[&id];
    assert!((binding.heights.top - 18.).abs() < 1e-5, "{binding:?}");
    assert!((binding.heights.bottom - 12.).abs() < 1e-5, "{binding:?}");
    let input = cam_geometry3d::resolve_binding(app.geometry_state(), binding).unwrap();
    assert!((input.holes[0].diameter - 4.2).abs() < 1e-9);
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no cylindrical cutting flank")),
        "{:?}",
        result.diagnostics
    );
    assert!(app.export_nc().unwrap().contains("Bore"));
}
