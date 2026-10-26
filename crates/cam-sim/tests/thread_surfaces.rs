use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_sim::{simulate, ProfileShape, SimOptions, SimTool, SimulationCache, ToolProfile};

fn tools() -> [SimTool; 2] {
    [
        SimTool {
            number: 1,
            profile: ToolProfile::flat(4.),
            noncutting_tip_radius: None,
        },
        SimTool {
            number: 2,
            profile: ToolProfile {
                radius: 1.,
                shape: ProfileShape::Thread {
                    root_radius: 0.5,
                    tooth_height: 1. / 3f64.sqrt(),
                    pitch: 0.,
                    tooth_count: 1,
                },
            },
            noncutting_tip_radius: Some(0.5),
        },
    ]
}
fn program(turns: usize) -> Program {
    let motion = |to: [f64; 3], kind| Step::Linear {
        to: Point3::new(to[0], to[1], to[2]),
        feed: 60.,
        tag: Tag::new(2, kind),
    };
    let mut steps = vec![
        Step::ToolChange { tool: 1 },
        Step::Rapid {
            to: Point3::new(0., 0., 7.),
            tag: Tag::new(1, MoveKind::Link),
        },
        Step::Linear {
            to: Point3::new(0., 0., 0.),
            feed: 60.,
            tag: Tag::new(1, MoveKind::Plunge),
        },
        Step::Rapid {
            to: Point3::new(0., 0., 7.),
            tag: Tag::new(1, MoveKind::Retract),
        },
        Step::ToolChange { tool: 2 },
        motion([0., 0., 0.], MoveKind::Plunge),
        motion([3.5, 0., 0.], MoveKind::LeadIn),
    ];
    for turn in 0..turns {
        steps.push(Step::Arc {
            end: Point3::new(3.5, 0., (turn + 1) as f64),
            center: Point3::new(0., 0., turn as f64),
            dir: ArcDir::Ccw,
            feed: 60.,
            tag: Tag::new(2, MoveKind::Cutting),
        });
    }
    Program { steps }
}
fn options() -> SimOptions {
    SimOptions {
        resolution: 0.125,
        tool_radius: 4.,
    }
}
fn run(program: &Program, tools: &[SimTool]) -> cam_sim::SimResult {
    simulate(program, [-6., -6., 0.], [6., 6., 5.], &options(), tools)
}

#[test]
fn thread_grooves_have_both_flanks_cut_ownership_and_retained_crests() {
    let result = run(&program(4), &tools());
    assert!(result.collisions.is_empty(), "{:?}", result.collisions);
    assert_eq!(
        result.field.sample(4.25, 0.),
        5.,
        "stock above the teeth must remain"
    );
    let mesh = result.field.to_solid_mesh(0.);
    let mut upper = 0;
    let mut lower = 0;
    for (i, &owner) in mesh.operations.iter().enumerate() {
        if owner != Some(2) {
            continue;
        }
        assert!(mesh.machined[i]);
        upper += usize::from(mesh.normals[i][2] > 0.5);
        lower += usize::from(mesh.normals[i][2] < -0.5);
        assert!((0. ..=5.).contains(&mesh.positions[i][2]));
    }
    assert!(
        upper > 100 && lower > 100,
        "both thread flanks must be present: {upper}, {lower}"
    );
    assert!(
        mesh.normals
            .iter()
            .zip(&mesh.operations)
            .any(|(n, op)| *op == Some(2) && n[2].abs() > 0.6 && n[2].abs() < 0.99),
        "sloping tooth flanks"
    );
    assert!(
        mesh.operations.contains(&Some(1)),
        "the prepared bore retains its own cut faces"
    );
    assert!(mesh
        .operations
        .iter()
        .zip(&mesh.machined)
        .all(|(op, cut)| op.is_none() || *cut));
    // Bounded grooves cannot remove an entire cylinder above the first turn.
    let prepared = run(&program(0), &tools());
    assert!(result.removed_volume > prepared.removed_volume);
    assert!(result.removed_volume - prepared.removed_volume < 50.);
}

#[test]
fn threading_keeps_the_prepared_bore_round_at_the_existing_accuracy() {
    let mesh = run(&program(4), &tools()).field.to_solid_mesh(0.);
    let mut count = 0;
    let mut worst = 0f32;
    for ((p, n), op) in mesh
        .positions
        .iter()
        .zip(&mesh.normals)
        .zip(&mesh.operations)
    {
        if *op == Some(1) && n[2].abs() < 0.01 && p[2] > 4.7 {
            count += 1;
            worst = worst.max((p[0].hypot(p[1]) - 4.).abs());
        }
    }
    assert!(count > 50);
    assert!(
        worst < 0.02,
        "bore wall contour error {worst} at 0.125 mm grid spacing"
    );
}

fn vertical_hits(mesh: &cam_sim::SurfaceMesh, xy: [f32; 2]) -> Vec<f32> {
    let mut hits = Vec::new();
    for triangle in mesh.indices.as_chunks::<3>().0 {
        if mesh.operations[triangle[0] as usize] != Some(2) {
            continue;
        }
        let [a, b, c] = triangle.map(|i| mesh.positions[i as usize]);
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if det.abs() < 1e-8 {
            continue;
        }
        let u = ((xy[0] - a[0]) * (c[1] - a[1]) - (xy[1] - a[1]) * (c[0] - a[0])) / det;
        let v = ((b[0] - a[0]) * (xy[1] - a[1]) - (b[1] - a[1]) * (xy[0] - a[0])) / det;
        if u >= -1e-5 && v >= -1e-5 && u + v <= 1. + 1e-5 {
            hits.push(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]));
        }
    }
    hits.sort_by(f32::total_cmp);
    hits.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    hits
}

#[test]
fn thread_flanks_follow_helix_pitch_and_hand_instead_of_becoming_a_smooth_bore() {
    for dir in [ArcDir::Ccw, ArcDir::Cw] {
        let mut program = program(4);
        for step in &mut program.steps {
            if let Step::Arc { dir: direction, .. } = step {
                *direction = dir;
            }
        }
        let mesh = run(&program, &tools()).field.to_solid_mesh(0.);
        for angle in [0.4f32, 0.4 + std::f32::consts::FRAC_PI_2] {
            let hits = vertical_hits(&mesh, [4.25 * angle.cos(), 4.25 * angle.sin()]);
            let phase = if dir == ArcDir::Ccw { angle } else { -angle }
                .rem_euclid(std::f32::consts::TAU)
                / std::f32::consts::TAU;
            for z in [
                1. + phase + 0.25 / 3f32.sqrt(),
                1. + phase + 0.75 / 3f32.sqrt(),
            ] {
                assert!(
                    hits.iter().any(|hit| (hit - z).abs() < 0.04),
                    "{dir:?}, angle {angle}: expected flank {z}, got {hits:?}"
                );
            }
        }
    }
}

#[test]
fn a_later_cut_removes_thread_faces_without_double_counting_the_removed_volume() {
    let mut threaded = run(&program(4), &tools()).field;
    let mut baseline = run(&program(0), &tools()).field;
    for field in [&mut threaded, &mut baseline] {
        field.cut_segment([0., 0., 0.], [0., 0., 0.], 5.);
    }
    assert!(!threaded.to_solid_mesh(0.).operations.contains(&Some(2)));
    assert_eq!(threaded.removed_volume(), baseline.removed_volume());
}

#[test]
fn partial_thread_and_rewind_restore_the_exact_stock_and_operation_owners() {
    let table = tools();
    let mut cache = SimulationCache::default();
    for turns in [0, 1, 4, 2, 0, 4] {
        let program = program(turns);
        let cached = cache
            .simulate_controlled(
                &program,
                [-6., -6., 0.],
                [6., 6., 5.],
                &options(),
                &table,
                &|| false,
                |_, _| {},
            )
            .unwrap();
        let fresh = run(&program, &table);
        assert_eq!(cached.removed_volume, fresh.removed_volume);
        assert_eq!(cached.collisions, fresh.collisions);
        assert_eq!(
            cached.field.to_solid_mesh(0.),
            fresh.field.to_solid_mesh(0.)
        );
    }
}

#[test]
fn full_form_thread_mill_removes_separate_tooth_intervals() {
    let mut table = tools();
    table[1].profile.shape = ProfileShape::Thread {
        root_radius: 0.5,
        tooth_height: 0.6,
        pitch: 1.,
        tooth_count: 4,
    };
    let result = run(&program(1), &table);
    assert!(result.collisions.is_empty(), "{:?}", result.collisions);
    let mesh = result.field.to_solid_mesh(0.);
    for band in 0..4 {
        assert!(
            mesh.positions
                .iter()
                .zip(&mesh.operations)
                .any(|(p, op)| *op == Some(2) && p[2] > band as f32 && p[2] < band as f32 + 0.8),
            "tooth {band}"
        );
    }
    assert_eq!(result.field.sample(4.25, 0.), 5.);
}
