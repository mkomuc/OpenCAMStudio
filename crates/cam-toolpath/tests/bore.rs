use cam_cldata::{validate_program, ArcDir, MoveKind, SpindleDir, Step};
use cam_model::{BoreOp, EntryFeeds, Heights, Tool, ToolKind};
use cam_toolpath::{BoreStrategy, CancelToken, JobEnv, Strategy, StrategyResult};

fn operation() -> BoreOp {
    BoreOp {
        id: 42,
        points: vec![[20., 30.]],
        z_bottom: -4.3,
        ..Default::default()
    }
}
fn run(op: BoreOp, spindle: SpindleDir) -> StrategyResult {
    let tools = [Tool {
        number: 1,
        diameter: 6.,
        length: 50.,
        flute_length: 20.,
        kind: ToolKind::EndMill,
        ..Default::default()
    }];
    let result = BoreStrategy::new(op).compute(
        &JobEnv {
            heights: Heights::new(10., 2., 0.),
            tools: &tools,
            stock: None,
            spindle,
        },
        &CancelToken::new(),
    );
    if !result.has_errors() {
        validate_program(&result.program).unwrap();
    }
    result
}
fn cutting(result: &StrategyResult) -> Vec<(cam_cldata::Point3, ArcDir, f64)> {
    result
        .program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Arc {
                end,
                dir,
                feed,
                tag,
                ..
            } if tag.kind == MoveKind::Cutting => Some((*end, *dir, *feed)),
            _ => None,
        })
        .collect()
}

#[test]
fn fractional_depth_has_exact_radius_pitch_and_complete_bottom_circle() {
    let result = run(operation(), SpindleDir::Cw);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let arcs = cutting(&result);
    assert!(arcs.len() > 3);
    for (p, direction, _) in &arcs {
        assert!(((p.x - 20.).hypot(p.y - 30.) - 3.).abs() < 1e-8);
        assert_eq!(*direction, ArcDir::Ccw);
    }
    let helix = &arcs[..arcs.len() - 2];
    let mut last_z = 0.;
    let dz = 4.3 / helix.len() as f64;
    for (p, _, _) in helix {
        assert!((last_z - p.z - dz).abs() < 1e-10);
        last_z = p.z;
    }
    assert!((last_z + 4.3).abs() < 1e-10);
    assert_eq!(arcs[arcs.len() - 2].0.z, -4.3);
    assert_eq!(arcs[arcs.len() - 1].0.z, -4.3);
    // The bottom circle consists of two opposite half-turns ending at the helix endpoint.
    let last = arcs.last().unwrap().0;
    assert!((last.x - helix.last().unwrap().0.x).abs() < 1e-8);
    assert!((last.y - helix.last().unwrap().0.y).abs() < 1e-8);
}

#[test]
fn direction_accounts_for_feature_side_cut_style_and_spindle() {
    for internal in [true, false] {
        for climb in [true, false] {
            for spindle in [SpindleDir::Cw, SpindleDir::Ccw] {
                let result = run(
                    BoreOp {
                        internal,
                        climb,
                        ..operation()
                    },
                    spindle,
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let dir = cutting(&result)[0].1;
                assert_eq!(
                    dir == ArcDir::Ccw,
                    (internal == climb) == (spindle == SpindleDir::Cw)
                );
            }
        }
    }
}

#[test]
fn allowances_multiple_passes_and_spring_pass_reach_the_requested_wall() {
    let result = run(
        BoreOp {
            stepovers: 2,
            stepover: 0.5,
            finishing_stepover: 0.2,
            repeat_pass: true,
            radial_stock: 0.1,
            axial_stock: 0.3,
            ..operation()
        },
        SpindleDir::Cw,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entries: Vec<_> = result
        .program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Arc { end, tag, .. } if tag.kind == MoveKind::LeadIn => Some(end.x - 20.),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), 5);
    for (actual, expected) in entries.iter().zip([1.7, 2.2, 2.7, 2.9, 2.9]) {
        assert!((actual - expected).abs() < 1e-9);
    }
    assert!(cutting(&result).iter().all(|(p, _, _)| p.z >= -4.));
}

#[test]
fn angle_limits_axial_travel_and_entry_feeds_are_independent() {
    let op = BoreOp {
        ramp_angle: Some(2.),
        entry_feeds: EntryFeeds {
            ramp: Some(111.),
            lead_in: Some(222.),
            lead_out: Some(333.),
        },
        ..operation()
    };
    let result = run(op, SpindleDir::Cw);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let mut prev_z = 0.;
    for (p, _, feed) in cutting(&result) {
        if p.z < prev_z {
            assert_eq!(feed, 111.);
            assert!(prev_z - p.z <= std::f64::consts::PI * 3. * 2f64.to_radians().tan() + 1e-9);
        } else {
            assert_eq!(feed, 300.);
        }
        prev_z = p.z;
    }
    assert!(result.program.steps().iter().any(|s| matches!(s, Step::Arc { feed, tag, .. } if tag.kind == MoveKind::LeadIn && *feed == 222.)));
    assert!(result.program.steps().iter().any(|s| matches!(s, Step::Arc { feed, tag, .. } if tag.kind == MoveKind::LeadOut && *feed == 333.)));
}

#[test]
fn tapered_spiral_respects_wall_and_chord_tolerance() {
    for internal in [true, false] {
        let taper = if internal { 5. } else { -5. };
        let op = BoreOp {
            internal,
            taper_angle: taper,
            tolerance: 0.002,
            ..operation()
        };
        let result = run(op.clone(), SpindleDir::Cw);
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let radius = |z: f64| 6. + z * taper.to_radians().tan() + if internal { -3. } else { 3. };
        let mut previous = cam_cldata::Point3::new(20. + radius(0.), 30., 0.);
        let mut count = 0;
        for step in result.program.steps() {
            if let Step::Linear { to, tag, .. } = step {
                if tag.kind != MoveKind::Cutting {
                    continue;
                }
                assert!(((to.x - 20.).hypot(to.y - 30.) - radius(to.z)).abs() < 1e-9);
                let mid_r = ((previous.x + to.x) / 2. - 20.).hypot((previous.y + to.y) / 2. - 30.);
                assert!((mid_r - radius((previous.z + to.z) / 2.)).abs() <= op.tolerance + 1e-9);
                previous = *to;
                count += 1;
            }
        }
        assert!(count > 50);
    }
}

#[test]
fn features_and_passes_link_only_at_clearance() {
    let result = run(
        BoreOp {
            points: vec![[20., 30.], [50., 40.]],
            stepovers: 1,
            ..operation()
        },
        SpindleDir::Cw,
    );
    assert!(!result.has_errors());
    let mut previous: Option<cam_cldata::Point3> = None;
    for step in result.program.steps() {
        match step {
            Step::Rapid { to, .. } => {
                if let Some(p) = previous {
                    if (p.x - to.x).abs() > 1e-8 || (p.y - to.y).abs() > 1e-8 {
                        assert_eq!(p.z, 10.);
                        assert_eq!(to.z, 10.);
                    }
                }
                previous = Some(*to);
            }
            Step::Linear { to, .. } => previous = Some(*to),
            Step::Arc { end, .. } => previous = Some(*end),
            _ => {}
        }
    }
}

#[test]
fn invalid_inputs_return_errors_without_motions() {
    let mut invalid = Vec::new();
    for pitch in [0., -1., f64::NAN, f64::INFINITY, 1e-300] {
        invalid.push(BoreOp {
            pitch,
            ..operation()
        });
    }
    invalid.extend([
        BoreOp {
            diameter: 6.,
            ..operation()
        },
        BoreOp {
            radial_stock: -3.,
            ..operation()
        },
        BoreOp {
            stepovers: 8,
            ..operation()
        },
        BoreOp {
            z_bottom: 1.,
            ..operation()
        },
        BoreOp {
            z_bottom: -30.,
            ..operation()
        },
        BoreOp {
            taper_angle: -5.,
            ..operation()
        },
        BoreOp {
            internal: false,
            taper_angle: 5.,
            ..operation()
        },
        BoreOp {
            points: vec![[f64::NAN, 0.]],
            ..operation()
        },
        BoreOp {
            ramp_angle: Some(90.),
            ..operation()
        },
    ]);
    for op in invalid {
        let result = run(op.clone(), SpindleDir::Cw);
        assert!(result.has_errors(), "accepted {op:?}");
        assert!(result.program.steps().is_empty());
    }
}

#[test]
fn cancelled_operation_does_not_emit_motion() {
    let token = CancelToken::new();
    token.cancel();
    let result = BoreStrategy::new(operation()).compute(
        &JobEnv {
            heights: Heights::new(10., 2., 0.),
            tools: &[],
            stock: None,
            spindle: SpindleDir::Cw,
        },
        &token,
    );
    assert!(result.cancelled);
    assert!(result.program.steps().is_empty());
}

#[test]
fn ordering_and_reverse_are_deterministic() {
    use cam_model::BoreOrder;
    let points = vec![[30., 10.], [0., 30.], [10., 0.]];
    for (order, expected) in [
        (BoreOrder::Selected, vec![points[0], points[1], points[2]]),
        (BoreOrder::Optimized, vec![points[0], points[2], points[1]]),
        (BoreOrder::InsideOut, vec![points[2], points[0], points[1]]),
        (BoreOrder::X, vec![points[1], points[2], points[0]]),
        (BoreOrder::Y, vec![points[2], points[0], points[1]]),
    ] {
        for reverse in [false, true] {
            let result = run(
                BoreOp {
                    points: points.clone(),
                    order,
                    reverse_order: reverse,
                    ..operation()
                },
                SpindleDir::Cw,
            );
            let actual: Vec<_> = result
                .program
                .steps()
                .iter()
                .filter_map(|s| match s {
                    Step::Rapid { to, tag } if tag.kind == MoveKind::Link && to.z == 10. => {
                        Some([to.x, to.y])
                    }
                    _ => None,
                })
                .collect();
            let mut expected = expected.clone();
            if reverse {
                expected.reverse();
            }
            assert_eq!(actual, expected);
        }
    }
}
