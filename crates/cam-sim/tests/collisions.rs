use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_sim::{simulate, CollisionKind, SimOptions, SimTool, SimulationCache, ToolProfile};

fn tag(kind: MoveKind) -> Tag {
    Tag {
        op_id: 7,
        kind,
        pattern_id: Some(9),
        instance_index: Some(2),
    }
}
fn tools() -> [SimTool; 2] {
    [
        SimTool {
            number: 1,
            profile: ToolProfile::flat(2.),
            noncutting_tip_radius: None,
        },
        SimTool {
            number: 2,
            profile: ToolProfile::flat(2.),
            noncutting_tip_radius: Some(1.),
        },
    ]
}
fn options() -> SimOptions {
    SimOptions {
        resolution: 0.2,
        tool_radius: 2.,
    }
}
fn run(program: &Program) -> cam_sim::SimResult {
    simulate(program, [-5., -5., -5.], [5., 5., 0.], &options(), &tools())
}
fn plunge(tool: u32, kind: MoveKind) -> Program {
    Program {
        steps: vec![
            Step::ToolChange { tool },
            Step::Rapid {
                to: Point3::new(0., 0., 2.),
                tag: tag(MoveKind::Link),
            },
            Step::Linear {
                to: Point3::new(0., 0., -3.),
                feed: 60.,
                tag: tag(kind),
            },
        ],
    }
}

#[test]
fn noncutting_tip_cannot_erase_stock_during_a_feed_plunge() {
    let result = run(&plunge(2, MoveKind::Plunge));
    assert_eq!(result.removed_volume, 0.);
    assert_eq!(result.field.sample(0., 0.), 0.);
    let c = &result.collisions[0];
    assert_eq!(c.kind, CollisionKind::NonCuttingThroughStock);
    assert_eq!(
        (c.op_id, c.pattern_id, c.instance_index),
        (Some(7), Some(9), Some(2))
    );
    assert_eq!(c.step_index, Some(2));
    assert!((c.at[2] + 0.001).abs() < 0.00002, "{c:?}");
    assert!((c.fraction - 0.4002).abs() < 0.00001);
    let cutting = run(&plunge(1, MoveKind::Plunge));
    assert!(cutting.collisions.is_empty());
    assert!(cutting.removed_volume > 30.);
}

#[test]
fn noncutting_feeds_check_stock_instead_of_cutting_it() {
    for kind in [MoveKind::Link, MoveKind::Retract, MoveKind::Traverse] {
        let result = run(&plunge(1, kind));
        assert_eq!(result.collisions.len(), 1, "{kind:?}");
        assert_eq!(result.removed_volume, 0.);
    }
}

#[test]
fn prepared_bore_accepts_a_thread_mill_and_cache_rewind_restores_the_core() {
    let mut program = plunge(1, MoveKind::Plunge);
    program.steps.push(Step::Rapid {
        to: Point3::new(0., 0., 2.),
        tag: tag(MoveKind::Retract),
    });
    program.steps.extend(plunge(2, MoveKind::Plunge).steps);
    assert!(run(&program).collisions.is_empty());
    let mut cache = SimulationCache::default();
    for program in [&program, &plunge(2, MoveKind::Plunge)] {
        let cached = cache
            .simulate_controlled(
                program,
                [-5., -5., -5.],
                [5., 5., 0.],
                &options(),
                &tools(),
                &|| false,
                |_, _| {},
            )
            .unwrap();
        let fresh = run(program);
        assert_eq!(cached.collisions, fresh.collisions);
        assert_eq!(cached.removed_volume, fresh.removed_volume);
    }
}

#[test]
fn a_pilot_hole_that_clears_only_the_neck_is_too_small_for_a_thread_mill_plunge() {
    let mut program = plunge(1, MoveKind::Plunge);
    program.steps.push(Step::ToolChange { tool: 2 });
    program.steps.extend(plunge(2, MoveKind::Plunge).steps);
    let mut table = tools();
    table[0].profile = ToolProfile::flat(1.2);
    let result = simulate(&program, [-5., -5., -5.], [5., 5., 0.], &options(), &table);
    assert_eq!(result.collisions.len(), 1);
    assert_eq!(
        result.collisions[0].kind,
        CollisionKind::NonCuttingThroughStock
    );
    assert!((result.collisions[0].at[2] + 0.001).abs() < 0.00002);
}

#[test]
fn a_thread_mill_can_side_cut_a_descending_helix_with_its_tip_in_a_prepared_hole() {
    let mut table = tools();
    table[0].profile = ToolProfile::flat(3.);
    let mut program = plunge(1, MoveKind::Plunge);
    program.steps.extend([
        Step::Rapid {
            to: Point3::new(0., 0., 2.),
            tag: tag(MoveKind::Retract),
        },
        Step::ToolChange { tool: 2 },
        Step::Rapid {
            to: Point3::new(1.5, 0., 2.),
            tag: tag(MoveKind::Link),
        },
        Step::Linear {
            to: Point3::new(1.5, 0., 0.5),
            feed: 60.,
            tag: tag(MoveKind::Plunge),
        },
        Step::Arc {
            end: Point3::new(1.5, 0., -2.),
            center: Point3::new(0., 0., 0.5),
            dir: ArcDir::Cw,
            feed: 60.,
            tag: tag(MoveKind::Cutting),
        },
    ]);
    let result = simulate(&program, [-5., -5., -5.], [5., 5., 0.], &options(), &table);
    assert!(result.collisions.is_empty(), "{:?}", result.collisions);
    assert!(
        result.field.sample(3.2, 0.) < 0.,
        "the teeth can cut the side wall"
    );
}

#[test]
fn a_diagonal_rapid_only_hits_stock_where_the_tool_actually_descends() {
    let mut program = plunge(1, MoveKind::Plunge);
    // Start outside the stock at a low Z, then rise before reaching its edge.
    program.steps = vec![
        Step::Rapid {
            to: Point3::new(-10., 0., -2.),
            tag: tag(MoveKind::Link),
        },
        Step::Rapid {
            to: Point3::new(0., 0., 8.),
            tag: tag(MoveKind::Link),
        },
    ];
    assert!(run(&program).collisions.is_empty());
    program.steps.push(Step::Rapid {
        to: Point3::new(0., 0., -2.),
        tag: tag(MoveKind::Link),
    });
    let result = run(&program);
    assert_eq!(result.collisions.len(), 1);
    assert_eq!(result.collisions[0].step_index, Some(2));
    assert!(result.collisions[0].at[2].abs() < 0.002);
}

#[test]
fn noncutting_arc_contact_keeps_the_arc_fraction_and_does_not_clear_stock() {
    let program = Program {
        steps: vec![
            Step::Rapid {
                to: Point3::new(1., 0., 2.),
                tag: tag(MoveKind::Link),
            },
            Step::Arc {
                end: Point3::new(1., 0., -2.),
                center: Point3::new(0., 0., 2.),
                dir: ArcDir::Ccw,
                feed: 60.,
                tag: tag(MoveKind::Link),
            },
        ],
    };
    let result = run(&program);
    assert_eq!(result.collisions.len(), 1);
    assert_eq!(result.removed_volume, 0.);
    let c = &result.collisions[0];
    assert_eq!(c.step_index, Some(1));
    assert!((c.fraction - 0.50025).abs() < 1e-5, "{c:?}");
    assert!((c.at[0] + 1.).abs() < 0.02);
}

#[test]
fn canned_cycles_check_repositioning_between_holes_and_preserve_the_retract_pose() {
    let mut program = Program {
        steps: vec![Step::Drill(cam_cldata::DrillCycle {
            points: vec![[-2., 0.], [2., 0.]],
            z_top: 0.,
            depth: -3.,
            retract: -1.,
            peck: Some(1.),
            dwell: None,
            feed: 60.,
            tag: tag(MoveKind::Plunge),
        })],
    };
    let mut table = tools();
    table[0].profile = ToolProfile::flat(0.5);
    let run = |p: &Program| simulate(p, [-5., -5., -5.], [5., 5., 0.], &options(), &table);
    // Use the small cutter as the default, before any tool change.
    program.steps.insert(0, Step::ToolChange { tool: 1 });
    let between = run(&program);
    assert_eq!(between.collisions.len(), 1);
    assert_eq!(between.collisions[0].kind, CollisionKind::RapidThroughStock);
    assert_eq!(
        between.field.sample(0., 0.),
        0.,
        "the link cannot clear the bridge"
    );
    program.steps.push(Step::Rapid {
        to: Point3::new(2., 4., -1.),
        tag: tag(MoveKind::Link),
    });
    let after = run(&program);
    assert_eq!(
        after.collisions.len(),
        2,
        "the link after a cycle must also be checked"
    );
    assert_eq!(after.collisions[1].step_index, Some(2));
}
