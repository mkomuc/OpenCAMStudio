use cam_app::{default_machine, AppController, ExportError, OpKind};
use cam_geo::{Contour, Point};
use cam_model::*;
use cam_toolpath::{CancelToken, JobCompletion};
struct Fixture {
    t: TopologySnapshot,
    next: u64,
}
impl Fixture {
    fn new() -> Self {
        Self {
            t: TopologySnapshot {
                bodies: vec![CadBody {
                    id: 1,
                    name: "Analytic plate".into(),
                    valid_solid: true,
                    bounds: [[0., 0., -5.], [20., 20., 0.]],
                }],
                ..Default::default()
            },
            next: 2,
        }
    }
    fn id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
    fn vertex(&mut self, p: [f64; 3]) -> u64 {
        if let Some(v) = self.t.vertices.iter().find(|v| v.point == p) {
            return v.id;
        }
        let id = self.id();
        self.t.vertices.push(CadVertex {
            id,
            body_id: 1,
            point: p,
        });
        id
    }
    fn face(&mut self, rings: Vec<Vec<[f64; 3]>>, normal: [f64; 3]) -> u64 {
        let id = self.id();
        let mut loop_ids = vec![];
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for (i, ring) in rings.iter().enumerate() {
            let wid = self.id();
            loop_ids.push(wid);
            let mut edge_uses = vec![];
            for j in 0..ring.len() {
                let a = ring[j];
                let b = ring[(j + 1) % ring.len()];
                for k in 0..3 {
                    bounds[0][k] = bounds[0][k].min(a[k]);
                    bounds[1][k] = bounds[1][k].max(a[k]);
                }
                let va = self.vertex(a);
                let vb = self.vertex(b);
                let ei = self
                    .t
                    .edges
                    .iter()
                    .position(|e| e.vertices == [va, vb] || e.vertices == [vb, va]);
                let (edge_id, reversed) = if let Some(ei) = ei {
                    let e = &mut self.t.edges[ei];
                    e.face_ids.push(id);
                    (e.id, e.vertices != [va, vb])
                } else {
                    let edge_id = self.id();
                    self.t.edges.push(CadEdge {
                        id: edge_id,
                        body_id: 1,
                        vertices: [va, vb],
                        face_ids: vec![id],
                        curve: CadCurve::Line { start: a, end: b },
                    });
                    (edge_id, false)
                };
                edge_uses.push(EdgeUse { edge_id, reversed });
            }
            self.t.loops.push(CadLoop {
                id: wid,
                body_id: 1,
                face_id: id,
                edge_uses,
                is_outer: i == 0,
            });
        }
        self.t.faces.push(CadFace {
            id,
            body_id: 1,
            surface: CadSurface::Plane {
                origin: rings[0][0],
                normal,
            },
            loop_ids,
            bounds,
        });
        id
    }
    fn walls(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, lo: f64, hi: f64) {
        self.face(
            vec![vec![[x0, y0, lo], [x1, y0, lo], [x1, y0, hi], [x0, y0, hi]]],
            [0., -1., 0.],
        );
        self.face(
            vec![vec![[x1, y0, lo], [x1, y1, lo], [x1, y1, hi], [x1, y0, hi]]],
            [1., 0., 0.],
        );
        self.face(
            vec![vec![[x1, y1, lo], [x0, y1, lo], [x0, y1, hi], [x1, y1, hi]]],
            [0., 1., 0.],
        );
        self.face(
            vec![vec![[x0, y1, lo], [x0, y0, lo], [x0, y0, hi], [x0, y1, hi]]],
            [-1., 0., 0.],
        );
    }
    fn state(self) -> GeometryState {
        GeometryState {
            assets: vec![GeometryAsset {
                import_options: None,
                import_diagnostics: vec![],
                source_fingerprint: String::new(),
                id: 1,
                revision: 1,
                name: "analytic".into(),
                source_format: CadFormat::Step,
                source_data: vec![].into(),
                source_units: SourceUnits::Millimetres,
                backend: "analytic test fixture (not imported STEP)".into(),
                topology: self.t.into(),
            }],
            instances: vec![GeometryInstance {
                id: 1,
                asset_id: 1,
                body_id: 1,
                name: "part".into(),
                placement: Default::default(),
                visible: true,
            }],
            ..Default::default()
        }
    }
}
fn square(x0: f64, x1: f64, y0: f64, y1: f64, z: f64) -> Vec<[f64; 3]> {
    vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]
}
fn reference(id: u64, kind: EntityKind) -> EntityRef {
    EntityRef {
        asset_id: 1,
        revision: 1,
        instance_id: 1,
        body_id: 1,
        kind,
        id,
    }
}
fn pocket() -> (GeometryState, EntityRef) {
    let mut f = Fixture::new();
    let floor = f.face(
        vec![square(2., 18., 2., 18., -3.), square(8., 12., 8., 12., -3.)],
        [0., 0., 1.],
    );
    f.walls(2., 18., 2., 18., -3., 0.);
    f.walls(8., 12., 8., 12., -3., 0.);
    f.face(
        vec![square(0., 20., 0., 20., 0.), square(2., 18., 2., 18., 0.)],
        [0., 0., 1.],
    );
    f.face(vec![square(8., 12., 8., 12., 0.)], [0., 0., 1.]);
    f.walls(0., 20., 0., 20., -5., 0.);
    f.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
    (f.state(), reference(floor, EntityKind::Face))
}
fn controller() -> AppController {
    let mut app = AppController::new(default_machine());
    app.edit_tool(0, |t| {
        t.diameter = 3.;
        t.flute_length = 4.;
    });
    app
}
fn install_pocket(app: &mut AppController) -> EntityRef {
    let (state, r) = pocket();
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    r
}
fn cuts(program: &cam_cldata::Program) -> Vec<cam_cldata::Step> {
    program
        .steps()
        .iter()
        .filter(|s| {
            matches!(s,
                cam_cldata::Step::Linear{tag,..} | cam_cldata::Step::Arc{tag,..}
                if matches!(tag.kind,cam_cldata::MoveKind::Cutting | cam_cldata::MoveKind::Plunge)
            )
        })
        .cloned()
        .collect()
}
#[test]
fn positive_world_floor_uses_physical_depth_and_matches_an_independent_planar_cut() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    app.place_cad_instance(
        1,
        RigidTransform {
            translation: [0., 0., 40.],
            ..Default::default()
        },
    )
    .unwrap();
    app.edit_heights(|h| {
        h.top_of_stock = 40.;
        h.retract = 42.;
        h.clearance = 45.;
    });
    app.edit_stock(|s| {
        *s = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 40.,
            thickness: 5.,
        }
    });
    let id = app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    let actual = cuts(&outcome.program);
    assert!(!actual.is_empty());
    assert!(actual
        .iter()
        .any(|s| matches!(s,cam_cldata::Step::Linear{to,..} if (to.z-37.).abs()<1e-8)));
    // Build independent explicit 2D geometry; do not call the CAD resolver.
    let mut planar = controller();
    let contour = |x0, x1, y0, y1| {
        Contour::new(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ])
    };
    planar.add_operation(Operation::Pocket(PocketOp {
        id,
        tool: 1,
        work_offset: 1,
        spindle_rpm: 1000.,
        boundary: contour(2., 18., 2., 18.),
        islands: vec![contour(8., 12., 8., 12.)],
        depth: 3.,
        start: None,
        clear: ClearParams {
            stepdown: 2.,
            overlap: 0.5,
            offset: 0.,
            feed: 300.,
            plunge_feed: 100.,
            plunge: Plunge::Straight,
            lead_overlap: 0.,
            lead_in: Lead::None,
            lead_out: Lead::None,
            clearing: Clearing::default(),
        },
    }));
    let expected = cuts(
        &planar
            .run(&CancelToken::new())
            .program
            .translated([0., 0., 40.]),
    );
    assert_eq!(actual, expected);
}
#[test]
fn embedded_assets_bindings_and_overrides_reopen_with_identical_output() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    let id = app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    app.override_cad_heights(id, None, Some(-2.)).unwrap();
    let before = app.run(&CancelToken::new()).program.clone();
    let path = std::env::temp_dir().join(format!("cad-roundtrip-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = controller();
    reopened.open_project(&path).unwrap();
    assert_eq!(app.document(), reopened.document());
    assert_eq!(
        reopened.document().geometry.bindings[&id].overrides.bottom,
        Some(-2.)
    );
    assert_eq!(before, reopened.run(&CancelToken::new()).program);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_cad_linked_operation_cannot_reopen_with_its_binding_removed() {
    let mut app = controller();
    let selected = install_pocket(&mut app);
    let operation = app
        .create_cad_operation(OpKind::Pocket, vec![selected])
        .unwrap();
    let path =
        std::env::temp_dir().join(format!("cad-missing-binding-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    project["document"]["geometry"]["bindings"]
        .as_object_mut()
        .unwrap()
        .remove(&operation.to_string());
    std::fs::write(&path, project.to_string()).unwrap();

    let mut reopened = controller();
    let error = reopened.open_project(&path).unwrap_err().to_string();
    std::fs::remove_file(path).unwrap();
    assert!(error.contains("missing its geometry binding"), "{error}");
}

#[test]
fn a_persisted_binding_cannot_reopen_after_its_linked_marker_is_removed() {
    let mut app = controller();
    let selected = install_pocket(&mut app);
    let operation = app
        .create_cad_operation(OpKind::Pocket, vec![selected])
        .unwrap();
    let path = std::env::temp_dir().join(format!("cad-missing-marker-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(project["document"]["geometry"]["bindings"][operation.to_string()].is_object());
    project["document"]["cad_linked_operations"] = serde_json::json!([]);
    std::fs::write(&path, project.to_string()).unwrap();

    let mut reopened = controller();
    let error = reopened.open_project(&path).unwrap_err().to_string();
    std::fs::remove_file(path).unwrap();
    assert!(error.contains("not marked as CAD-linked"), "{error}");
}

#[test]
fn a_rotated_setup_resolves_linked_cad_geometry_in_its_own_cutting_frame() {
    let mut app = controller();
    let floor = install_pocket(&mut app);
    let first_id = app
        .create_cad_operation(OpKind::Pocket, vec![floor])
        .unwrap();
    let first = app.run(&CancelToken::new());
    assert_eq!(
        first.completion,
        JobCompletion::Complete,
        "{:?}",
        first.diagnostics
    );
    let first_cut_x: Vec<_> = cuts(&first.program)
        .iter()
        .filter_map(|step| match step {
            cam_cldata::Step::Linear { to, .. } => Some(to.x),
            cam_cldata::Step::Arc { end, .. } => Some(end.x),
            _ => None,
        })
        .collect();
    assert!(!first_cut_x.is_empty());
    assert!(first_cut_x.iter().any(|x| *x > 0.0));

    let first_setup = app.active_setup_id();
    let rotated_id = app.duplicate_setup(first_setup).unwrap();
    let mut rotated = app
        .setups()
        .iter()
        .find(|s| s.id == rotated_id)
        .unwrap()
        .clone();
    rotated.frame =
        SetupFrame::from_axes(Some([-1.0, 0.0, 0.0]), Some([0.0, -1.0, 0.0]), None).unwrap();
    rotated.stock = Stock::BoundingBox {
        x_offset: 0.0,
        y_offset: 0.0,
        top: 0.0,
        thickness: 6.0,
    };
    app.apply_setup_settings(rotated).unwrap();
    let duplicated_op = app.document().setup().operations[0].id();
    assert_ne!(first_id, duplicated_op);
    assert!(app
        .document()
        .geometry
        .bindings
        .contains_key(&duplicated_op));

    let second = app.run(&CancelToken::new());
    assert_eq!(
        second.completion,
        JobCompletion::Complete,
        "{:?}",
        second.diagnostics
    );
    let rotated_cut_xy: Vec<_> = cuts(&second.program)
        .iter()
        .filter_map(|step| match step {
            cam_cldata::Step::Linear { to, .. } => Some([to.x, to.y]),
            cam_cldata::Step::Arc { end, .. } => Some([end.x, end.y]),
            _ => None,
        })
        .collect();
    assert!(!rotated_cut_xy.is_empty());
    assert!(
        rotated_cut_xy.iter().any(|p| p[0] < 0.0 && p[1] < 0.0),
        "the linked pocket should be resolved in the rotated setup frame"
    );
}

#[test]
fn model_coordinate_system_tracks_instance_placement_and_undo_restores_both() {
    let mut app = controller();
    let _ = install_pocket(&mut app);
    let instance_id = app.geometry_state().instances[0].id;

    let (frame, origin) = app.pending_frame_from_model(instance_id).unwrap();
    let mut settings = app.document().setup().clone();
    settings.name = "Model WCS".into();
    settings.frame = frame;
    settings.origin = origin;
    settings.stock = Stock::RelativeBox {
        offsets: [[1.0, 1.0, 2.0], [1.0, 1.0, 2.0]],
    };
    settings.heights = Heights::new(12.0, 5.0, 2.0);
    app.apply_setup_settings(settings).unwrap();
    assert_eq!(
        app.document().setup().frame.references.model_instance,
        Some(instance_id)
    );
    assert!(matches!(
        app.document().setup().frame.references.origin,
        FrameOrigin::ModelOrigin
    ));

    let rotated_x = RigidTransform {
        rotation: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
        translation: [10.0, 20.0, 30.0],
    };
    app.place_cad_instance(instance_id, rotated_x).unwrap();
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        rotated_x
    );
    assert_eq!(
        app.document().setup().frame.world_from_setup.rotation,
        rotated_x.rotation
    );
    assert_eq!(app.document().setup().origin, [10.0, 30.0, -20.0]);

    let restored_instance = app
        .geometry_state()
        .instance(instance_id)
        .unwrap()
        .placement;
    let restored_frame = app.document().setup().frame.clone();
    let restored_origin = app.document().setup().origin;
    let rotated_z = RigidTransform {
        rotation: [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [15.0, 25.0, 35.0],
    };
    app.place_cad_instance(instance_id, rotated_z).unwrap();
    assert_eq!(
        app.document().setup().frame.world_from_setup.rotation,
        rotated_z.rotation
    );
    assert_eq!(app.document().setup().origin, [-15.0, -25.0, 35.0]);
    assert!(
        app.undo(),
        "instance placement and associated model WCS share one history edit"
    );
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        restored_instance
    );
    assert_eq!(app.document().setup().frame, restored_frame);
    assert_eq!(app.document().setup().origin, restored_origin);

    let mut explicit = app.document().setup().clone();
    explicit.frame.references.origin = FrameOrigin::Explicit([4.0, 5.0, 6.0]);
    explicit.origin = [4.0, 5.0, 6.0];
    app.apply_setup_settings(explicit).unwrap();
    let explicit_before_move = app.document().setup().origin;
    let rotated_y = RigidTransform {
        rotation: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        translation: [11.0, 12.0, 13.0],
    };
    app.place_cad_instance(instance_id, rotated_y).unwrap();
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        rotated_y
    );
    assert_eq!(
        app.document().setup().frame.world_from_setup.rotation,
        rotated_y.rotation
    );
    assert_eq!(
        app.document().setup().origin,
        explicit_before_move,
        "an explicit datum must remain fixed when the shared model moves"
    );
    assert!(matches!(
        app.document().setup().frame.references.origin,
        FrameOrigin::Explicit([4.0, 5.0, 6.0])
    ));
    assert!(app.undo());
    assert_eq!(
        app.geometry_state()
            .instance(instance_id)
            .unwrap()
            .placement,
        restored_instance
    );
}

#[test]
fn replacement_requires_review_and_undo_restores_links_and_export_eligibility() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    let id = app
        .create_cad_operation(OpKind::Pocket, vec![r.clone()])
        .unwrap();
    app.override_cad_heights(id, None, Some(-2.)).unwrap();
    let replacement = app.geometry_state().assets[0].clone();
    app.replace_cad_asset(1, replacement).unwrap();
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Failed
    );
    assert!(matches!(
        app.export_nc(),
        Err(ExportError::HasErrors) | Err(ExportError::Incomplete)
    ));
    app.set_operation_excluded(id, true);
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    app.set_operation_excluded(id, false);
    let mut new_ref = r.clone();
    new_ref.revision = 2;
    app.edit_cad_geometry(id, vec![new_ref]).unwrap();
    assert_eq!(
        app.document().geometry.bindings[&id].overrides.bottom,
        Some(-2.)
    );
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    assert!(app.undo()); // undo reselection
    assert!(matches!(
        app.document().geometry.bindings[&id].status,
        ResolutionStatus::NeedsReselection { .. }
    ));
    assert!(app.undo()); // undo replacement (suppression is session state)
    assert_eq!(app.document().geometry.assets[0].revision, 1);
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
}
#[test]
fn asset_and_binding_duplicate_delete_undo_together() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    assert!(app.undo());
    assert!(app.geometry_state().assets.is_empty());
    assert!(app.redo());
    let id = app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    app.duplicate_selected_operation();
    assert_eq!(app.geometry_state().bindings.len(), 2);
    app.delete_selected_operation();
    assert_eq!(app.geometry_state().bindings.len(), 1);
    assert!(app.geometry_state().bindings.contains_key(&id));
    assert!(app.undo());
    assert_eq!(app.geometry_state().bindings.len(), 2);
    assert!(app.remove_cad_asset(1));
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Failed
    );
    assert!(app.undo());
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
}
#[test]
fn direct_planner_rejects_linked_geometry_without_a_matching_resolution() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    let job = cam_toolpath::build_job(
        app.document(),
        1000.,
        cam_cldata::SpindleDir::Cw,
        None,
        50.,
        &CancelToken::new(),
    );
    assert_eq!(job.completion, JobCompletion::Failed);
    assert!(job
        .diagnostics
        .iter()
        .any(|d| d.message.contains("resolved snapshot")));
}
#[test]
fn editing_translated_linked_depth_keeps_a_physical_magnitude() {
    let mut app = controller();
    let r = install_pocket(&mut app);
    app.place_cad_instance(
        1,
        RigidTransform {
            translation: [0., 0., 40.],
            ..Default::default()
        },
    )
    .unwrap();
    app.edit_heights(|h| {
        h.top_of_stock = 40.;
        h.retract = 42.;
        h.clearance = 45.;
    });
    let id = app.create_cad_operation(OpKind::Pocket, vec![r]).unwrap();
    app.edit_operation(id, |op| {
        if let Operation::Pocket(p) = op {
            p.depth = 2.;
        }
    });
    assert_eq!(
        app.document().geometry.bindings[&id].overrides.bottom,
        Some(38.)
    );
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
}

#[test]
fn committing_operation_depth_with_unchanged_link_fields_retains_the_override() {
    let mut app = controller();
    let reference = install_pocket(&mut app);
    let id = app
        .create_cad_operation(OpKind::Pocket, vec![reference])
        .unwrap();
    let mut draft = app.operation(id).unwrap().clone();
    let unchanged_links = app.document().geometry.bindings[&id].overrides.clone();
    if let Operation::Pocket(operation) = &mut draft {
        operation.depth = 2.;
    }
    app.apply_operation_settings(draft, Some(unchanged_links))
        .unwrap();
    assert_eq!(
        app.document().geometry.bindings[&id].overrides.bottom,
        Some(-2.)
    );
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    let mut draft = app.operation(id).unwrap().clone();
    if let Operation::Pocket(operation) = &mut draft {
        operation.clear.feed = 250.;
    }
    app.apply_operation_settings(draft, Some(GeometryOverrides::default()))
        .unwrap();
    assert_eq!(app.document().geometry.bindings[&id].overrides.bottom, None);
}
#[test]
fn background_import_previews_cancel_and_reject_project_edits_without_losing_work() {
    let path = std::env::temp_dir().join(format!("cad-job-{}.dxf", std::process::id()));
    let drawing =
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n20\n21\n0\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&path, drawing).unwrap();
    let mut app = controller();
    let original = app.document().clone();
    app.start_cad_import(path.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(result) = app.poll_cad_import() {
            result.unwrap();
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(app.document(), &original);
    assert!(app.cad_import_preview().is_some());
    app.cancel_cad_import();
    assert!(app.cad_import_preview().is_none());
    assert_eq!(app.document(), &original);
    app.start_cad_import(path.clone());
    app.edit_heights(|h| h.clearance = 10.);
    assert!(app.poll_cad_import().unwrap().is_err());
    assert!(app.open_paths().is_empty());
    app.start_cad_import(path.clone());
    loop {
        if let Some(result) = app.poll_cad_import() {
            result.unwrap();
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    app.edit_heights(|h| h.retract = 3.);
    assert!(app.commit_cad_preview().is_err());
    assert!(app.open_paths().is_empty());
    std::fs::remove_file(path).unwrap();
}
