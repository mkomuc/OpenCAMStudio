//! Run explicitly with `--features occt --test native_step -- --ignored` and a
//! real OCCT SDK. These never count a missing native backend as a successful import.
#![cfg(feature = "occt")]
use cam_app::{default_machine, AppController, OpKind};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cam-kernel/fixtures")
        .join(name)
}
fn reference(state: &GeometryState, kind: EntityKind, id: u64) -> EntityRef {
    let asset = &state.assets[0];
    let instance = &state.instances[0];
    EntityRef {
        asset_id: asset.id,
        revision: asset.revision,
        instance_id: instance.id,
        body_id: instance.body_id,
        kind,
        id,
    }
}
fn cutting_z_range(program: &cam_cldata::Program) -> (f64, f64) {
    let mut z: Vec<f64> = Vec::new();
    for step in program.steps() {
        match step {
            cam_cldata::Step::Linear { to, tag, .. }
                if matches!(
                    tag.kind,
                    cam_cldata::MoveKind::Cutting | cam_cldata::MoveKind::Plunge
                ) =>
            {
                z.push(to.z)
            }
            cam_cldata::Step::Arc { end, tag, .. }
                if matches!(
                    tag.kind,
                    cam_cldata::MoveKind::Cutting | cam_cldata::MoveKind::Plunge
                ) =>
            {
                z.push(end.z)
            }
            _ => {}
        }
    }
    (
        *z.iter()
            .min_by(|a, b| a.total_cmp(b))
            .expect("cutting moves"),
        *z.iter()
            .max_by(|a, b| a.total_cmp(b))
            .expect("cutting moves"),
    )
}
fn posted_cutting_z_range(nc: &str) -> (f64, f64) {
    let mut mode = 0;
    let mut z: Vec<f64> = Vec::new();
    for line in nc.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if let Some(g) = words
            .iter()
            .find_map(|word| word.strip_prefix('G').and_then(|n| n.parse::<u8>().ok()))
        {
            mode = g;
        }
        if (1..=3).contains(&mode) {
            for value in words.iter().filter_map(|word| word.strip_prefix('Z')) {
                let number: String = value
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
                    .collect();
                if let Ok(value) = number.parse() {
                    z.push(value);
                }
            }
        }
    }
    (
        *z.iter()
            .min_by(|a, b| a.total_cmp(b))
            .expect("posted Z words"),
        *z.iter()
            .max_by(|a, b| a.total_cmp(b))
            .expect("posted Z words"),
    )
}
fn new_app() -> AppController {
    assert!(
        cam_kernel::backend_available(),
        "This qualification test requires a real OCCT SDK"
    );
    let mut app = AppController::new(default_machine());
    app.edit_tool(0, |t| {
        t.diameter = 3.;
        t.flute_length = 12.;
    });
    app.edit_stock(|s| {
        *s = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 0.,
            thickness: 10.,
        }
    });
    app
}
#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn step_plate_floor_island_four_holes_profile_generates_and_reopens_without_source() {
    let mut app = new_app();
    let source = std::env::temp_dir().join(format!("native-source-{}.step", std::process::id()));
    std::fs::copy(fixture("pocket-island-holes-mm.step"), &source).unwrap();
    app.import_cad(&source).unwrap();
    std::fs::remove_file(&source).unwrap();
    app.geometry_state().validate().unwrap();
    let topology = &app.geometry_state().assets[0].topology;
    assert_eq!(topology.bodies.len(), 1);
    assert_eq!(topology.bodies[0].bounds, [[0., 0., -10.], [60., 40., 0.]]);
    let floor=topology.faces.iter().find(|f|matches!(f.surface,CadSurface::Plane{origin,normal} if (origin[2]+6.).abs()<1e-6&&normal[2]>0.9)).expect("pocket floor");
    let floor_ref = reference(app.geometry_state(), EntityKind::Face, floor.id);
    let holes: Vec<_> = topology
        .faces
        .iter()
        .filter(|f| matches!(f.surface, CadSurface::Cylinder { inward: true, .. }))
        .map(|f| reference(app.geometry_state(), EntityKind::Face, f.id))
        .collect();
    assert_eq!(holes.len(), 4);
    let top=topology.faces.iter().find(|f|matches!(f.surface,CadSurface::Plane{origin,normal} if origin[2].abs()<1e-6&&normal[2]>0.9)&&f.loop_ids.len()>2).unwrap();
    let outer = topology
        .loops
        .iter()
        .find(|w| w.face_id == top.id && w.is_outer)
        .unwrap();
    let profile_ref = reference(app.geometry_state(), EntityKind::Loop, outer.id);
    let pocket = app
        .create_cad_operation(OpKind::Pocket, vec![floor_ref])
        .unwrap();
    let Operation::Pocket(p) = app.operation(pocket).unwrap() else {
        panic!()
    };
    assert_eq!(p.islands.len(), 1);
    assert_eq!(p.depth, 6.);
    let drill = app.use_tool(Tool {
        number: 7,
        diameter: 5.,
        length: 30.,
        flute_length: 12.,
        kind: ToolKind::Drill {
            point_angle_deg: 118.,
        },
        ..Default::default()
    });
    app.create_cad_operation_with_tool(OpKind::Drill, holes, drill)
        .unwrap();
    app.create_cad_operation_with_tool(OpKind::Profile, vec![profile_ref], 1)
        .unwrap();
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    let program = result.program.clone();
    let nc = app.export_nc().unwrap().to_string();
    assert!(nc.contains("G"));
    let path = std::env::temp_dir().join(format!("native-step-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = new_app();
    reopened.open_project(&path).unwrap();
    assert_eq!(reopened.run(&CancelToken::new()).program, program);
    assert_eq!(reopened.export_nc().unwrap(), nc);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn box_step_machines_top_and_bottom_faces_in_independent_frames_and_round_trips() {
    let mut app = new_app();
    app.import_cad(fixture("box-mm.step")).unwrap();
    app.geometry_state().validate().unwrap();
    let asset = &app.geometry_state().assets[0];
    let instance_id = app.geometry_state().instances[0].id;
    let original_placement = app.geometry_state().instances[0].placement;
    let topology = &asset.topology;
    assert_eq!(
        topology.bodies[0].bounds,
        [[0.0, 0.0, -10.0], [60.0, 40.0, 0.0]]
    );
    let top = topology
        .faces
        .iter()
        .find(|face| {
            matches!(face.surface,
                CadSurface::Plane { origin, normal } if origin[2].abs() < 1e-7 && normal[2] > 0.99
            )
        })
        .expect("top planar face at maximum Z");
    let bottom = topology.faces.iter().find(|face| matches!(face.surface,
        CadSurface::Plane { origin, normal } if (origin[2] + 10.0).abs() < 1e-7 && normal[2] < -0.99
    )).expect("bottom planar face at minimum Z");
    let top_ref = reference(app.geometry_state(), EntityKind::Face, top.id);
    let bottom_ref = reference(app.geometry_state(), EntityKind::Face, bottom.id);

    let mut top_setup = app.document().setup().clone();
    top_setup.name = "Top".into();
    top_setup.program_name = "TOP".into();
    top_setup.stock = Stock::FixedBox {
        size: [62.0, 42.0, 12.0],
        model_position: [0.0; 3],
    };
    top_setup.heights = Heights::new(15.0, 5.0, 2.0);
    app.apply_setup_settings(top_setup).unwrap();
    assert_eq!(app.stock_box(), ([0.0, 0.0, -10.0], [62.0, 42.0, 2.0]));
    let top_operation = app
        .create_cad_operation(OpKind::Face, vec![top_ref])
        .unwrap();
    let top_heights = app.geometry_state().bindings[&top_operation].heights;
    assert!((top_heights.top - 2.0).abs() < 1e-6 && (top_heights.bottom - 0.0).abs() < 1e-6);
    let top_run = app.run(&CancelToken::new());
    assert_eq!(
        top_run.completion,
        JobCompletion::Complete,
        "{:?}",
        top_run.diagnostics
    );
    let top_program = top_run.program.clone();
    let (top_cut_min, top_cut_max) = cutting_z_range(&top_program);
    assert!(
        (top_cut_min - 0.0).abs() < 1e-6 && (top_cut_max - 0.0).abs() < 1e-6,
        "top face must cut at the model's maximum Z0: {top_cut_min}..{top_cut_max}"
    );
    let top_nc = app.export_nc().unwrap().to_owned();
    assert_eq!(posted_cutting_z_range(&top_nc), (0.0, 0.0));

    let top_setup_id = app.active_setup_id();
    let bottom_setup_id = app.add_setup();
    let mut bottom_setup = app.document().setup().clone();
    bottom_setup.name = "Bottom".into();
    bottom_setup.program_name = "BOTTOM".into();
    bottom_setup.frame =
        SetupFrame::from_axes(Some([1.0, 0.0, 0.0]), Some([0.0, -1.0, 0.0]), None).unwrap();
    bottom_setup.stock = Stock::FixedBox {
        size: [62.0, 42.0, 12.0],
        model_position: [0.0; 3],
    };
    bottom_setup.heights = Heights::new(20.0, 15.0, 12.0);
    // In the flipped frame the bottom face is local Z10; the operator touches off
    // there, so posted cutting coordinates should be 0..2 after one datum shift.
    bottom_setup.origin = [0.0, 0.0, 10.0];
    app.apply_setup_settings(bottom_setup).unwrap();
    assert_eq!(app.stock_box(), ([0.0, -40.0, 0.0], [62.0, 2.0, 12.0]));
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        original_placement,
        "editing a setup frame must leave shared source placement unchanged"
    );
    let bottom_operation = app
        .create_cad_operation(OpKind::Face, vec![bottom_ref])
        .unwrap();
    let bottom_heights = app.geometry_state().bindings[&bottom_operation].heights;
    assert!(
        (bottom_heights.top - 12.0).abs() < 1e-6 && (bottom_heights.bottom - 10.0).abs() < 1e-6
    );
    let bottom_run = app.run(&CancelToken::new());
    assert_eq!(
        bottom_run.completion,
        JobCompletion::Complete,
        "{:?}",
        bottom_run.diagnostics
    );
    let bottom_program = bottom_run.program.clone();
    let (bottom_cut_min, bottom_cut_max) = cutting_z_range(&bottom_program);
    assert!(
        (bottom_cut_min - 10.0).abs() < 1e-6 && (bottom_cut_max - 10.0).abs() < 1e-6,
        "world minimum-Z face must resolve to local Z10: {bottom_cut_min}..{bottom_cut_max}"
    );
    let bottom_nc = app.export_nc().unwrap().to_owned();
    assert_eq!(
        posted_cutting_z_range(&bottom_nc),
        (0.0, 0.0),
        "the local Z10 datum must be subtracted exactly once"
    );
    assert_ne!(
        top_program, bottom_program,
        "setup-local cut coordinates must reflect the bottom fixture"
    );
    assert_ne!(
        top_nc, bottom_nc,
        "each setup must export its own named NC program"
    );
    assert!(top_nc.contains("TOP"));
    assert!(bottom_nc.contains("BOTTOM"));
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        original_placement
    );

    let path = std::env::temp_dir().join(format!("native-two-side-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = new_app();
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(reopened.setups().len(), 2);
    assert_eq!(
        reopened
            .geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        original_placement
    );
    reopened.activate_setup(top_setup_id).unwrap();
    assert_eq!(reopened.run(&CancelToken::new()).program, top_program);
    assert_eq!(reopened.export_nc().unwrap(), top_nc);
    reopened.activate_setup(bottom_setup_id).unwrap();
    assert_eq!(reopened.run(&CancelToken::new()).program, bottom_program);
    assert_eq!(reopened.export_nc().unwrap(), bottom_nc);
}
#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn inch_fixture_normalizes_to_the_same_millimetre_geometry() {
    let mm =
        cam_kernel::read_step_file(fixture("pocket-island-holes-mm.step"), &Default::default())
            .unwrap();
    let inch = cam_kernel::read_step_file(
        fixture("pocket-island-holes-inch.step"),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(mm.source_units, SourceUnits::Millimetres);
    assert_eq!(inch.source_units, SourceUnits::Inches);
    for i in 0..2 {
        for j in 0..3 {
            assert!(
                (mm.topology.bodies[0].bounds[i][j] - inch.topology.bodies[0].bounds[i][j]).abs()
                    < 1e-5
            );
        }
    }
    assert_eq!(mm.topology.faces.len(), inch.topology.faces.len());
    assert_eq!(mm.topology.edges.len(), inch.topology.edges.len());
}
#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn display_tessellation_quality_does_not_change_pocket_nc() {
    let mut snapshots = vec![];
    for tolerance in [0.5, 0.02] {
        let import = cam_kernel::read_step_file(
            fixture("pocket-island-holes-mm.step"),
            &cam_kernel::SolidImportOptions {
                display_tolerance: tolerance,
                ..Default::default()
            },
        )
        .unwrap();
        let asset = GeometryAsset {
            import_options: None,
            id: 1,
            revision: 1,
            name: "plate".into(),
            source_format: CadFormat::Step,
            source_data: vec![].into(),
            source_units: import.source_units,
            backend: import.backend,
            source_fingerprint: String::new(),
            import_diagnostics: import.warnings,
            topology: import.topology.into(),
        };
        let instances = import
            .placements
            .into_iter()
            .map(|p| GeometryInstance {
                id: 1,
                asset_id: 1,
                body_id: p.body_id,
                name: "plate".into(),
                placement: p.placement,
                visible: true,
            })
            .collect();
        let mut app = new_app();
        app.add_cad_asset(asset, instances).unwrap();
        let f=app.geometry_state().assets[0].topology.faces.iter().find(|f|matches!(f.surface,CadSurface::Plane{origin,normal} if (origin[2]+6.).abs()<1e-6&&normal[2]>0.9)).unwrap();
        let r = reference(app.geometry_state(), EntityKind::Face, f.id);
        app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
        assert_eq!(
            app.run(&CancelToken::new()).completion,
            JobCompletion::Complete
        );
        snapshots.push(app.export_nc().unwrap().to_string());
    }
    assert_eq!(snapshots[0], snapshots[1]);
}
#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn rotated_step_can_be_aligned_to_one_fixed_machining_frame() {
    let mut app = new_app();
    app.import_cad(fixture("pocket-island-holes-rotated.step"))
        .unwrap();
    let topology = &app.geometry_state().assets[0].topology;
    let top = topology
        .faces
        .iter()
        .find(|f| {
            f.loop_ids.len() > 2 && matches!(f.surface,CadSurface::Plane{normal,..}if normal[2]>0.5)
        })
        .unwrap();
    let floor = topology
        .faces
        .iter()
        .find(|f| {
            f.loop_ids.len() == 2
                && matches!(f.surface,CadSurface::Plane{normal,..}if normal[2]>0.5)
        })
        .unwrap();
    let top_ref = reference(app.geometry_state(), EntityKind::Face, top.id);
    let floor_ref = reference(app.geometry_state(), EntityKind::Face, floor.id);
    assert!(app
        .create_cad_operation(OpKind::Pocket, vec![floor_ref.clone()])
        .is_err());
    app.align_cad_top_face(top_ref).unwrap();
    let preview = app
        .preview_cad_operation(OpKind::Pocket, std::slice::from_ref(&floor_ref))
        .unwrap();
    assert!(preview.heights.top.abs() < 1e-6);
    assert!((preview.heights.bottom + 6.).abs() < 1e-6);
    app.create_cad_operation(OpKind::Pocket, vec![floor_ref])
        .unwrap();
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
}
#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn repeated_step_components_keep_separate_placements_and_selection_identity() {
    let mut app = new_app();
    app.import_cad(fixture("repeated-boxes.step")).unwrap();
    assert_eq!(app.geometry_state().instances.len(), 2);
    assert_eq!(app.geometry_state().assets[0].topology.bodies.len(), 1);
    let first = &app.geometry_state().instances[0];
    let second = &app.geometry_state().instances[1];
    assert_ne!(first.id, second.id);
    assert_eq!(first.body_id, second.body_id);
    assert_ne!(first.placement, second.placement);
}

#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn background_replacement_preview_requires_commit_and_marks_operations_for_reselection() {
    let mut app = new_app();
    app.import_cad(fixture("pocket-island-holes-mm.step"))
        .unwrap();
    let f=app.geometry_state().assets[0].topology.faces.iter().find(|f|matches!(f.surface,CadSurface::Plane{origin,normal} if (origin[2]+6.).abs()<1e-6&&normal[2]>0.9)).unwrap();
    let r = reference(app.geometry_state(), EntityKind::Face, f.id);
    let id = app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    app.start_cad_replacement(1, fixture("pocket-island-holes-mm.step"))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(result) = app.poll_cad_import() {
            result.unwrap();
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(app.geometry_state().assets[0].revision, 1);
    assert!(app.cad_import_preview().is_some());
    app.commit_cad_preview().unwrap();
    assert_eq!(app.geometry_state().assets[0].revision, 2);
    assert!(matches!(
        app.geometry_state().bindings[&id].status,
        ResolutionStatus::NeedsReselection { .. }
    ));
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Failed
    );
}

#[test]
#[ignore = "requires OCCT SDK; run explicitly for native qualification"]
fn facing_uses_actual_stock_top_and_rederives_it_after_stock_edits() {
    let mut app = new_app();
    app.import_cad(fixture("box-mm.step")).unwrap();
    let f=app.geometry_state().assets[0].topology.faces.iter().find(|f|matches!(f.surface,CadSurface::Plane{origin,normal}if origin[2].abs()<1e-6&&normal[2]>0.9)).unwrap();
    let r = reference(app.geometry_state(), EntityKind::Face, f.id);
    app.edit_stock(|s| {
        *s = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 1.,
            thickness: 11.,
        }
    });
    let id = app.create_cad_operation(OpKind::Face, vec![r]).unwrap();
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    app.edit_stock(|s| {
        *s = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 1.5,
            thickness: 11.5,
        }
    });
    assert_eq!(app.geometry_state().bindings[&id].heights.top, 1.5);
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    assert!(app.run(&CancelToken::new()).program.steps().iter().any(|s|matches!(s,cam_cldata::Step::Linear{to,tag,..}if tag.kind==cam_cldata::MoveKind::Cutting&&to.z.abs()<1e-9)));
}
