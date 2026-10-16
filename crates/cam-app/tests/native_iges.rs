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
