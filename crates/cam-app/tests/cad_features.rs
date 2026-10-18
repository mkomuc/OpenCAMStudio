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
    fn bore(&mut self, center: [f64; 2], radius: f64, top: u64, bottom: u64) -> u64 {
        let face = self.id();
        let mut loop_ids = vec![];
        for (z, adjacent) in [(0., top), (-5., bottom)] {
            let vertex = self.vertex([center[0] + radius, center[1], z]);
            let edge = self.id();
            self.t.edges.push(CadEdge {
                id: edge,
                body_id: 1,
                vertices: [vertex, vertex],
                face_ids: vec![face, adjacent],
                curve: CadCurve::CircleArc {
                    center: [center[0], center[1], z],
                    normal: [0., 0., 1.],
                    x_axis: [1., 0., 0.],
                    radius,
                    start_angle: 0.,
                    end_angle: std::f64::consts::TAU,
                },
            });
            for owner in [face, adjacent] {
                let wire = self.id();
                self.t.loops.push(CadLoop {
                    id: wire,
                    body_id: 1,
                    face_id: owner,
                    edge_uses: vec![EdgeUse {
                        edge_id: edge,
                        reversed: owner == adjacent,
                    }],
                    is_outer: owner == face,
                });
                if owner == face {
                    loop_ids.push(wire);
                } else {
                    self.t
                        .faces
                        .iter_mut()
                        .find(|f| f.id == owner)
                        .unwrap()
                        .loop_ids
                        .push(wire);
                }
            }
        }
        self.t.faces.push(CadFace {
            id: face,
            body_id: 1,
            surface: CadSurface::Cylinder {
                origin: [center[0], center[1], 0.],
                axis: [0., 0., 1.],
                radius,
                inward: true,
            },
            loop_ids,
            bounds: [
                [center[0] - radius, center[1] - radius, -5.],
                [center[0] + radius, center[1] + radius, 0.],
            ],
        });
        face
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
fn select_all_models(app: &mut AppController) {
    let ids = app
        .geometry_state()
        .instances
        .iter()
        .map(|i| i.id)
        .collect();
    app.edit_setup(|setup| setup.model_instances = ids);
}
fn install_pocket(app: &mut AppController) -> EntityRef {
    let (state, r) = pocket();
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    select_all_models(app);
    r
}

fn blind_bore_with_unrelated_surface() -> (GeometryState, EntityRef) {
    let mut fixture = Fixture::new();
    let top = fixture.face(vec![square(0., 20., 0., 20., 0.)], [0., 0., 1.]);
    let floor = fixture.face(vec![square(6., 14., 6., 14., -5.)], [0., 0., 1.]);
    fixture.bore([10., 10.], 4., top, floor);
    let floor_wire = fixture
        .t
        .faces
        .iter()
        .find(|f| f.id == floor)
        .unwrap()
        .loop_ids[1];
    fixture
        .t
        .faces
        .iter_mut()
        .find(|f| f.id == floor)
        .unwrap()
        .loop_ids = vec![floor_wire];
    fixture
        .t
        .loops
        .iter_mut()
        .find(|w| w.id == floor_wire)
        .unwrap()
        .is_outer = true;
    let blend = fixture.face(vec![square(18., 20., 18., 20., -1.)], [0., 0., 1.]);
    fixture
        .t
        .faces
        .iter_mut()
        .find(|f| f.id == blend)
        .unwrap()
        .surface = CadSurface::Unsupported {
        description: "unrelated blend away from the bore".into(),
    };
    (fixture.state(), reference(floor_wire, EntityKind::Loop))
}

#[test]
fn face_profile_and_open_chain_replace_each_other_transactionally_and_survive_reopen() {
    let mut fixture = Fixture::new();
    let top = fixture.face(vec![square(0., 20., 0., 20., 0.)], [0., 0., 1.]);
    fixture.walls(0., 20., 0., 20., -5., 0.);
    fixture.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
    let state = fixture.state();
    let top_face = state.assets[0]
        .topology
        .faces
        .iter()
        .find(|face| face.id == top)
        .unwrap();
    let wire = state.assets[0]
        .topology
        .loops
        .iter()
        .find(|wire| wire.id == top_face.loop_ids[0])
        .unwrap();
    let edges: Vec<_> = wire
        .edge_uses
        .iter()
        .take(2)
        .map(|usage| reference(usage.edge_id, EntityKind::Edge))
        .collect();
    let mut app = controller();
    app.edit_tool(0, |tool| tool.flute_length = 10.);
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    select_all_models(&mut app);
    let face = reference(top, EntityKind::Face);
    let id = app
        .create_cad_operation(OpKind::Profile, vec![face.clone()])
        .unwrap();
    assert!(matches!(app.operation(id), Some(Operation::Profile(profile)) if profile.closed));
    let before = app.document().clone();
    let binding = app.preview_operation_binding(id, edges.clone()).unwrap();
    assert_eq!(binding.recipe, ExtractionRecipe::ProfileContours);
    assert_eq!(app.document(), &before);
    let operation = app.operation(id).unwrap().clone();
    app.apply_operation_settings_with_geometry(operation, None, vec![], Some(edges))
        .unwrap();
    assert!(
        matches!(app.operation(id), Some(Operation::Profile(profile)) if !profile.closed && profile.chain.len() == 3 && profile.side != Side::On)
    );
    let result = app.run(&CancelToken::new());
    assert_eq!(
        result.completion,
        JobCompletion::Complete,
        "{:?}",
        result.diagnostics
    );
    let path = std::env::temp_dir().join(format!("profile-contours-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = controller();
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(matches!(reopened.operation(id), Some(Operation::Profile(profile)) if !profile.closed));
    assert_eq!(
        reopened.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    let operation = app.operation(id).unwrap().clone();
    app.apply_operation_settings_with_geometry(operation, None, vec![], Some(vec![face]))
        .unwrap();
    assert!(
        matches!(app.operation(id), Some(Operation::Profile(profile)) if profile.closed && profile.side == Side::Outside)
    );
    assert!(app.undo());
    assert!(matches!(app.operation(id), Some(Operation::Profile(profile)) if !profile.closed));
}

#[test]
fn cad_face_contours_preserve_independent_sides_through_resolution_save_and_undo() {
    let mut fixture = Fixture::new();
    let rings = |z| vec![square(0., 20., 0., 20., z), square(8., 12., 8., 12., z)];
    let top = fixture.face(rings(0.), [0., 0., 1.]);
    fixture.walls(0., 20., 0., 20., -5., 0.);
    fixture.walls(8., 12., 8., 12., -5., 0.);
    fixture.face(rings(-5.), [0., 0., -1.]);
    let state = fixture.state();
    let mut app = controller();
    app.edit_tool(0, |tool| tool.flute_length = 10.);
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    select_all_models(&mut app);
    let id = app
        .create_cad_operation(OpKind::Profile, vec![reference(top, EntityKind::Face)])
        .unwrap();
    let before = app.document().clone();
    let mut operation = app.operation(id).unwrap().clone();
    let Operation::Profile(profile) = &mut operation else {
        unreachable!()
    };
    assert_eq!(profile.additional_chains.len(), 1);
    profile.side = Side::Inside;
    profile.additional_chains[0].side = Side::On;
    app.apply_operation_settings(operation, None).unwrap();
    assert!(
        matches!(app.operation(id),Some(Operation::Profile(p)) if p.side==Side::Inside && p.additional_chains[0].side==Side::On)
    );
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    let path = std::env::temp_dir().join(format!("cad-contour-sides-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut reopened = controller();
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(reopened.operation(id), app.operation(id));
    assert_eq!(
        reopened.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    assert!(app.undo());
    assert_eq!(app.document(), &before);
}

#[test]
fn inside_profile_uses_local_bore_geometry_and_preserves_depth_guards() {
    let (state, selected) = blind_bore_with_unrelated_surface();
    let mut app = controller();
    app.edit_tool(0, |t| t.flute_length = 10.);
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    select_all_models(&mut app);
    app.toggle_cad_selection(selected.clone(), false).unwrap();
    let preview = app.cad_operation_eligibility(OpKind::Profile).unwrap();
    assert_eq!(
        preview.heights,
        CuttingHeights {
            top: 0.,
            bottom: -5.
        }
    );
    let id = app
        .create_cad_operation(OpKind::Profile, vec![selected])
        .unwrap();
    assert!(matches!(app.operation(id), Some(Operation::Profile(p)) if p.side == Side::Inside));
    let before = app.document().clone();
    assert!(app
        .override_cad_heights(id, None, Some(-6.))
        .unwrap_err()
        .contains("certified"));
    assert_eq!(app.document(), &before);
}

#[test]
fn inside_profile_rejects_blocked_entries_invalid_solids_and_incomplete_bores() {
    for defect in [
        "blocked entry",
        "invalid solid",
        "partial rim",
        "protected island",
        "wrong setup",
    ] {
        let (mut state, selected) = blind_bore_with_unrelated_surface();
        let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
        let reason = match defect {
            "blocked entry" => {
                topology.bodies[0].bounds[1][2] = 1.;
                "above the bore entry"
            }
            "invalid solid" => {
                topology.bodies[0].valid_solid = false;
                "invalid/open solid"
            }
            "partial rim" => {
                let wire = topology.loops.iter().find(|w| w.id == selected.id).unwrap();
                let edge = topology
                    .edges
                    .iter_mut()
                    .find(|e| e.id == wire.edge_uses[0].edge_id)
                    .unwrap();
                if let CadCurve::CircleArc { end_angle, .. } = &mut edge.curve {
                    *end_angle = std::f64::consts::PI;
                }
                "complete constant-diameter bore"
            }
            "protected island" => {
                let floor_id = topology
                    .loops
                    .iter()
                    .find(|w| w.id == selected.id)
                    .unwrap()
                    .face_id;
                let floor = topology
                    .faces
                    .iter_mut()
                    .find(|f| f.id == floor_id)
                    .unwrap();
                let other_loop = topology
                    .loops
                    .iter()
                    .find(|w| w.face_id == floor_id && w.id != selected.id)
                    .unwrap();
                floor.loop_ids.push(other_loop.id);
                "circular floor boundary"
            }
            _ => {
                state.instances[0].placement.rotation = [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]];
                "not aligned with setup Z"
            }
        };
        let mut app = controller();
        app.add_cad_asset(state.assets[0].clone(), state.instances)
            .unwrap();
        select_all_models(&mut app);
        let before = app.document().clone();
        let error = app
            .create_cad_operation(OpKind::Profile, vec![selected])
            .unwrap_err();
        assert!(error.contains(reason), "{defect}: {error}");
        assert_eq!(app.document(), &before);
    }
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
    // Match the CAD stock top after translating this reference program by 40 mm.
    planar.edit_stock(|s| {
        *s = Stock::BoundingBox {
            x_offset: 0.,
            y_offset: 0.,
            top: 0.,
            thickness: 5.,
        };
    });
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
    assert!(app.undo()); // undo re-inclusion
    assert!(app.is_operation_excluded(id));
    assert!(app.undo()); // undo suppression
    assert!(!app.is_operation_excluded(id));
    assert!(app.undo()); // undo replacement
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
    assert!(app.undo()); // setup body selection
    assert!(app.document().setup().model_instances.is_empty());
    assert!(app.undo()); // import
    assert!(app.geometry_state().assets.is_empty());
    assert!(app.redo());
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
    assert!(app.document().setup().model_instances.is_empty());
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

#[test]
fn deleted_asset_id_is_never_reused_for_an_unrelated_import() {
    let mut app = controller();
    let reference = install_pocket(&mut app);
    let asset = app.geometry_state().assets[0].clone();
    let instances = app.geometry_state().instances.clone();
    assert!(app.remove_cad_asset(asset.id));
    app.add_cad_asset(asset, instances).unwrap();
    select_all_models(&mut app);
    assert_eq!(app.geometry_state().assets[0].id, 2);
    assert!(!app.geometry_state().entity_exists(&reference));
    assert_eq!(app.geometry_state().instances[0].id, 2);
}

#[test]
fn cad_feature_and_operation_first_resolve_and_create_equivalent_inputs() {
    let mut feature_first = controller();
    let feature = install_pocket(&mut feature_first);
    feature_first
        .toggle_cad_selection(feature.clone(), false)
        .unwrap();
    let kind = OpKind::Pocket;
    let before = feature_first.cad_operation_eligibility(kind).unwrap();
    let first_id = feature_first
        .create_cad_operation_with_tool(kind, feature_first.cad_selection().to_vec(), 1)
        .unwrap();

    let mut operation_first = controller();
    install_pocket(&mut operation_first);
    assert!(operation_first
        .cad_operation_eligibility(kind)
        .unwrap_err()
        .contains("select geometry"));
    operation_first
        .toggle_cad_selection(feature, false)
        .unwrap();
    assert_eq!(
        before,
        operation_first.cad_operation_eligibility(kind).unwrap()
    );
    let second_id = operation_first
        .create_cad_operation_with_tool(kind, operation_first.cad_selection().to_vec(), 1)
        .unwrap();
    assert_eq!(
        feature_first.operation(first_id),
        operation_first.operation(second_id)
    );
    assert_eq!(
        feature_first.geometry_state().bindings[&first_id],
        operation_first.geometry_state().bindings[&second_id]
    );
    assert_eq!(
        cuts(&feature_first.run(&CancelToken::new()).program),
        cuts(&operation_first.run(&CancelToken::new()).program)
    );
}

#[test]
fn cad_multi_selection_is_homogeneous_and_rejections_preserve_every_reference() {
    let mut app = controller();
    let floor = install_pocket(&mut app);
    let topology = &app.geometry_state().assets[0].topology;
    let other = reference(
        topology.faces.iter().find(|f| f.id != floor.id).unwrap().id,
        EntityKind::Face,
    );
    let edge = reference(topology.edges[0].id, EntityKind::Edge);
    app.toggle_cad_selection(floor.clone(), false).unwrap();
    assert!(app
        .toggle_cad_selection(edge, true)
        .unwrap_err()
        .contains("same kind"));
    assert_eq!(app.cad_selection(), std::slice::from_ref(&floor));
    app.toggle_cad_selection(other.clone(), true).unwrap();
    assert_eq!(app.cad_selection(), &[floor.clone(), other]);
    let before = app.document().clone();
    assert!(app
        .cad_operation_eligibility(OpKind::Pocket)
        .unwrap_err()
        .contains("one planar floor"));
    assert!(app
        .create_cad_operation(OpKind::Pocket, app.cad_selection().to_vec())
        .is_err());
    assert_eq!(app.document(), &before);
    let mut stale = floor.clone();
    stale.revision += 1;
    assert!(app
        .toggle_cad_selection(stale.clone(), true)
        .unwrap_err()
        .contains("stale"));
    assert_eq!(app.cad_selection().len(), 2);
    app.set_cad_selection(vec![floor, stale]);
    assert_eq!(
        app.cad_selection().len(),
        2,
        "invalid members are never silently dropped"
    );
    assert!(app
        .cad_operation_eligibility(OpKind::Pocket)
        .unwrap_err()
        .contains("stale"));
    for kind in [
        OpKind::Chamfer,
        OpKind::Thread,
        OpKind::Engrave,
        OpKind::Carve,
    ] {
        assert!(app
            .preview_cad_operation(kind, &app.cad_selection()[..1])
            .unwrap_err()
            .contains("currently support"));
    }
}

#[test]
fn a_different_body_instance_cannot_be_added_to_the_geometry_field() {
    let mut app = controller();
    let floor = install_pocket(&mut app);
    let mut second = app.geometry_state().instances[0].clone();
    second.id = 2;
    let mut asset = app.geometry_state().assets[0].clone();
    asset.id = 2;
    second.asset_id = 2;
    app.add_cad_asset(asset, vec![second]).unwrap();
    select_all_models(&mut app);
    let mut second_floor = floor.clone();
    second_floor.asset_id = 2;
    second_floor.instance_id = 2;
    app.toggle_cad_selection(floor.clone(), false).unwrap();
    assert!(app
        .toggle_cad_selection(second_floor, true)
        .unwrap_err()
        .contains("one body instance"));
    assert_eq!(app.cad_selection(), &[floor]);
}

#[test]
fn persisted_mixed_geometry_cannot_generate_or_export_using_cached_contours() {
    let mut app = controller();
    let floor = install_pocket(&mut app);
    let edge = reference(
        app.geometry_state().assets[0].topology.edges[0].id,
        EntityKind::Edge,
    );
    let id = app
        .create_cad_operation(OpKind::Pocket, vec![floor])
        .unwrap();
    assert_eq!(
        app.run(&CancelToken::new()).completion,
        JobCompletion::Complete
    );
    let path =
        std::env::temp_dir().join(format!("cad-mixed-selection-{}.ocam", std::process::id()));
    app.save_project(&path).unwrap();
    let mut project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    project["document"]["geometry"]["bindings"][id.to_string()]["selected"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(edge).unwrap());
    std::fs::write(&path, project.to_string()).unwrap();
    let mut reopened = controller();
    reopened.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let outcome = reopened.run(&CancelToken::new());
    assert_eq!(outcome.completion, JobCompletion::Failed);
    assert!(outcome
        .diagnostics
        .iter()
        .any(|d| d.message.contains("same kind")));
    assert!(cuts(&outcome.program).is_empty());
    assert!(matches!(reopened.export_nc(), Err(ExportError::HasErrors)));
}

#[test]
fn linked_drill_previews_staged_cutter_and_bore_then_commits_in_one_undo() {
    let mut fixture = Fixture::new();
    let top = fixture.face(vec![square(0., 20., 0., 20., 0.)], [0., 0., 1.]);
    let bottom = fixture.face(vec![square(0., 20., 0., 20., -5.)], [0., 0., -1.]);
    fixture.walls(0., 20., 0., 20., -5., 0.);
    let old_bore = reference(fixture.bore([5., 10.], 3., top, bottom), EntityKind::Face);
    let new_bore = reference(fixture.bore([15., 10.], 4., top, bottom), EntityKind::Face);
    let state = fixture.state();
    let mut app = controller();
    app.add_cad_asset(state.assets[0].clone(), state.instances)
        .unwrap();
    select_all_models(&mut app);
    let old_tool = app.use_tool(Tool {
        number: 10,
        diameter: 6.,
        length: 30.,
        flute_length: 20.,
        kind: ToolKind::Drill {
            point_angle_deg: 118.,
        },
        ..Default::default()
    });
    let id = app
        .create_cad_operation_with_tool(OpKind::Drill, vec![old_bore], old_tool)
        .unwrap();
    let before = app.document().clone();
    let revision = app.revision();
    let mut staged_tool = *app
        .document()
        .setup()
        .tools
        .iter()
        .find(|t| t.number == old_tool)
        .unwrap();
    staged_tool.number = app
        .document()
        .setup()
        .tools
        .iter()
        .map(|t| t.number)
        .max()
        .unwrap()
        + 1;
    staged_tool.diameter = 8.;
    let mut draft = app.operation(id).unwrap().clone();
    if let Operation::Drill(operation) = &mut draft {
        operation.tool = staged_tool.number;
        operation.feed = 173.;
    } else {
        panic!("expected linked drill");
    }
    let overrides = Some(GeometryOverrides {
        top: None,
        bottom: Some(-4.),
    });
    // The committed cutter cannot qualify the replacement bore by itself.
    assert!(app
        .preview_operation_geometry(id, vec![new_bore.clone()])
        .unwrap_err()
        .contains("diameter"));
    let binding = app
        .preview_operation_settings_with_geometry(
            draft.clone(),
            overrides.clone(),
            vec![staged_tool],
            vec![new_bore.clone()],
        )
        .unwrap();
    assert_eq!(binding.selected, vec![new_bore.clone()]);
    assert_eq!(binding.effective_heights().bottom, -4.);
    assert_eq!(
        app.document(),
        &before,
        "preview never publishes draft state"
    );
    assert_eq!(app.revision(), revision);
    assert!(app
        .apply_operation_settings_with_geometry(
            draft.clone(),
            overrides.clone(),
            vec![],
            Some(vec![new_bore.clone()]),
        )
        .unwrap_err()
        .contains("missing tool"));
    assert_eq!(app.document(), &before, "rejected transaction is atomic");
    app.apply_operation_settings_with_geometry(
        draft,
        overrides,
        vec![staged_tool],
        Some(vec![new_bore.clone()]),
    )
    .unwrap();
    let Operation::Drill(committed) = app.operation(id).unwrap() else {
        panic!("expected linked drill");
    };
    assert_eq!(committed.tool, staged_tool.number);
    assert_eq!(committed.points, vec![[15., 10.]]);
    assert_eq!(committed.depth, 4.);
    assert_eq!(committed.feed, 173.);
    assert_eq!(app.geometry_state().bindings[&id].selected, vec![new_bore]);
    assert!(app.document().setup().tools.contains(&staged_tool));
    assert!(app.undo());
    assert_eq!(
        app.document(),
        &before,
        "one undo restores cutter, source and all tabs"
    );
}

#[test]
fn single_model_edge_uses_current_orientation_and_applies_without_other_axis_picks() {
    for axis in 0..3 {
        let mut app = controller();
        install_pocket(&mut app);
        let mut settings = app.document().setup().clone();
        settings.frame = SetupFrame::from_xy([0., 1., 0.], [0., 0., 1.]).unwrap();
        app.apply_setup_settings(settings).unwrap();
        let previous = app.document().setup().clone();
        let geometry = app.geometry_state().clone();
        let edge = geometry.assets[0].topology.edges[0].clone();
        let selected = reference(edge.id, EntityKind::Edge);
        let mut picks = [None, None, None];
        picks[axis] = Some(selected.clone());
        let frame = app.pending_frame_from_edges(picks, [false; 3]).unwrap();
        frame.validate().unwrap();
        let CadCurve::Line { start, end } = edge.curve else {
            panic!("fixture edge must be straight")
        };
        let direction: [f64; 3] = std::array::from_fn(|i| end[i] - start[i]);
        let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
        let actual =
            frame.to_world_vector(std::array::from_fn(|i| if i == axis { 1. } else { 0. }));
        assert!((0..3).all(|i| (actual[i] - direction[i] / length).abs() < 1e-9));
        let mut setup = previous.clone();
        setup.frame = frame;
        app.apply_setup_settings(setup.clone()).unwrap();
        assert_eq!(app.document().setup(), &setup);
        assert_eq!(app.geometry_state(), &geometry);
        let loaded: Document =
            serde_json::from_str(&serde_json::to_string(app.document()).unwrap()).unwrap();
        assert_eq!(loaded.setup().frame, setup.frame);
        assert!(app.undo());
        assert_eq!(app.document().setup(), &previous);
        assert!(app.redo());
        assert_eq!(app.document().setup(), &setup);
    }
}

#[test]
fn pocket_and_drill_ignore_unrelated_blends_but_reject_obstructed_approaches() {
    for kind in [OpKind::Pocket, OpKind::Drill] {
        let (state, floor_loop) = blind_bore_with_unrelated_surface();
        let wire = state.assets[0]
            .topology
            .loops
            .iter()
            .find(|w| w.id == floor_loop.id)
            .unwrap();
        let id = if kind == OpKind::Pocket {
            wire.face_id
        } else {
            state.assets[0]
                .topology
                .faces
                .iter()
                .find(|f| matches!(f.surface, CadSurface::Cylinder { inward: true, .. }))
                .unwrap()
                .id
        };
        let selected = reference(id, EntityKind::Face);
        let mut app = controller();
        app.edit_tool(0, |t| t.flute_length = 10.);
        app.add_cad_asset(state.assets[0].clone(), state.instances.clone())
            .unwrap();
        select_all_models(&mut app);
        let preview = app
            .preview_cad_operation(kind, std::slice::from_ref(&selected))
            .unwrap();
        assert_eq!(
            preview.heights,
            CuttingHeights {
                top: 0.,
                bottom: -5.
            }
        );
        let tool = if kind == OpKind::Drill {
            app.use_tool(Tool {
                number: 7,
                diameter: 8.,
                length: 30.,
                flute_length: 10.,
                kind: ToolKind::Drill {
                    point_angle_deg: 118.,
                },
                ..Default::default()
            })
        } else {
            1
        };
        app.create_cad_operation_with_tool(kind, vec![selected.clone()], tool)
            .unwrap();
        assert_eq!(
            app.run(&CancelToken::new()).completion,
            JobCompletion::Complete
        );
        assert!(app.export_nc().is_ok());

        // An unknown curved roof is ignored only when its exact conservative
        // bounds prove it cannot block this feature, including after rotation.
        for blocked in [false, true] {
            let mut state = state.clone();
            let topology = std::sync::Arc::make_mut(&mut state.assets[0].topology);
            let blend = topology
                .faces
                .iter_mut()
                .find(|f| matches!(f.surface, CadSurface::Unsupported { .. }))
                .unwrap();
            blend.bounds = if blocked {
                [[9., 9., 1.], [11., 11., 2.]]
            } else {
                [[18., 18., 1.], [20., 20., 2.]]
            };
            state.instances[0].placement = RigidTransform {
                rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
                translation: [100., -50., 40.],
            };
            let mut app = controller();
            app.add_cad_asset(state.assets[0].clone(), state.instances)
                .unwrap();
            select_all_models(&mut app);
            let result = app.preview_cad_operation(kind, std::slice::from_ref(&selected));
            if blocked {
                assert!(result.unwrap_err().contains("blocks vertical access"));
            } else {
                assert!(result.is_ok(), "{result:?}");
            }
        }
    }
}
