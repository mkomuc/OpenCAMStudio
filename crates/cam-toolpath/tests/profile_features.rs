use cam_cldata::{CutterComp, MoveKind, Point3, SpindleDir, Step};
use cam_geo::{Contour, Point};
use cam_model::*;
use cam_toolpath::{
    profile_tab_preview, CancelToken, JobEnv, ProfileStrategy, Strategy, StrategyResult,
};

fn profile() -> ProfileOp {
    ProfileOp {
        passes: Default::default(),
        selections: None,
        closed: true,
        additional_chains: vec![],
        tabs: Default::default(),
        id: 1,
        tool: 1,
        chain: Contour::new(vec![
            Point::new(0., 0.),
            Point::new(20., 0.),
            Point::new(20., 20.),
            Point::new(0., 20.),
        ]),
        side: Side::Outside,
        comp: Comp::Computed,
        offset: 0.,
        depth: 4.,
        stepdown: 2.,
        stepover: 0.,
        spindle_rpm: 1000.,
        tool_preset: None,
        cooling: Default::default(),
        entry_feeds: Default::default(),
        work_offset: 1,
        feed: 300.,
        plunge_feed: 100.,
        end: None,
        start: Some([10., 0.]),
        lead_in: Lead::None,
        lead_out: Lead::None,
        lead_overlap: 0.,
        plunge: Plunge::Straight,
        clearing: Default::default(),
    }
}
fn run(op: ProfileOp) -> StrategyResult {
    let tools = [Tool {
        number: 1,
        diameter: 6.,
        length: 30.,
        flutes: 2,
        kind: ToolKind::EndMill,
        ..Default::default()
    }];
    ProfileStrategy::new(op).compute(
        &JobEnv {
            heights: Heights::new(8., 3., 0.),
            tools: &tools,
            stock: Some(([-10., -10.], [30., 30.])),
            spindle: SpindleDir::Cw,
        },
        &CancelToken::new(),
    )
}
fn cuts(r: &StrategyResult) -> Vec<Point3> {
    r.program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
            Step::Arc { end, tag, .. } if tag.kind == MoveKind::Cutting => Some(*end),
            _ => None,
        })
        .collect()
}
fn plunges(r: &StrategyResult) -> Vec<Point3> {
    r.program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(*to),
            _ => None,
        })
        .collect()
}

#[test]
fn invalid_and_excessive_depth_requests_fail_atomically() {
    for value in [f64::NAN, f64::INFINITY, 1e-20, 0., -1.] {
        let mut op = profile();
        op.stepdown = value;
        let r = run(op);
        assert!(r.has_errors(), "{value}: {:?}", r.diagnostics);
        assert!(r.program.is_empty());
    }
    let mut op = profile();
    op.plunge = Plunge::Ramp { angle_deg: 0.0001 };
    let r = run(op);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}

#[test]
fn even_and_finishing_depths_reach_the_exact_allowanced_floor() {
    let mut op = profile();
    op.depth = 7.;
    op.stepdown = 3.;
    op.passes.even_stepdowns = true;
    op.passes.floor_allowance = 1.;
    op.passes.finishing_depths = 2;
    op.passes.finishing_stepdown = 0.5;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(
        plunges(&r).iter().map(|p| p.z).collect::<Vec<_>>(),
        vec![-2.5, -5., -5.5, -6.]
    );
    assert_eq!(cuts(&r).iter().map(|p| p.z).fold(0., f64::min), -6.);
}

#[test]
fn finishing_and_spring_passes_have_distinct_offsets_and_feed() {
    let mut op = profile();
    op.passes.finishing = true;
    op.passes.finish_count = 3;
    op.passes.finish_stepover = 0.5;
    op.passes.spring_passes = 2;
    op.passes.finish_feed = 125.;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(plunges(&r).len(), 10);
    assert!(r
        .program
        .steps()
        .iter()
        .filter_map(|s| match s {
            Step::Linear { feed, tag, .. } | Step::Arc { feed, tag, .. }
                if tag.kind == MoveKind::Cutting =>
                Some(*feed),
            _ => None,
        })
        .all(|f| f == 125.));
    let starts: Vec<_> = plunges(&r)
        .into_iter()
        .filter(|p| p.z == -4.)
        .map(|p| p.y)
        .collect();
    for (actual, expected) in starts.iter().zip([-4., -3.5, -3., -3., -3.]) {
        assert!((actual - expected).abs() < 1e-8);
    }
}

#[test]
fn finish_only_final_keeps_rough_depths_and_limits_finish_stages() {
    let mut op = profile();
    op.passes.finishing = true;
    op.passes.finish_count = 3;
    op.passes.spring_passes = 1;
    op.passes.finish_only_final = true;
    op.passes.finish_feed = 120.;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert_eq!(plunges(&r).len(), 5);
    assert!(r.program.steps().iter().any(|s|matches!(s,Step::Linear{to,feed,tag,..} if to.z == -2. && *feed==300. && tag.kind==MoveKind::Cutting)));
}

#[test]
fn depth_order_interleaves_contours_and_contour_order_finishes_each() {
    let mut op = profile();
    let mut other = op.chain.clone();
    let points = other
        .points()
        .iter()
        .map(|p| Point::new(p.x + 50., p.y))
        .collect();
    other = Contour::new(points);
    op.additional_chains.push(ProfileContour {
        chain: other,
        closed: true,
        side: Side::Outside,
    });
    let a = run(op.clone());
    op.passes.order = ProfileOrder::Depth;
    let b = run(op);
    assert!(
        !a.has_errors() && !b.has_errors(),
        "{:?} {:?}",
        a.diagnostics,
        b.diagnostics
    );
    assert_eq!(
        plunges(&a).iter().map(|p| p.z).collect::<Vec<_>>(),
        vec![-2., -4., -2., -4.]
    );
    assert_eq!(
        plunges(&b).iter().map(|p| p.z).collect::<Vec<_>>(),
        vec![-2., -2., -4., -4.]
    );
}

#[test]
fn control_compensation_brackets_linear_leads_and_rejects_missing_engagement() {
    let mut op = profile();
    op.comp = Comp::Control;
    let bad = run(op.clone());
    assert!(bad.has_errors());
    assert!(bad.program.is_empty());
    op.lead_in = Lead::Linear { length: 4. };
    op.lead_out = op.lead_in;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let s = r.program.steps();
    assert!(
        plunges(&r).iter().all(|point| point.y < -3.),
        "Uncompensated plunges need full-radius clearance from the nominal wall"
    );
    let enable = s
        .iter()
        .position(|x| {
            matches!(
                x,
                Step::CutterComp(CutterComp::Left(_) | CutterComp::Right(_))
            )
        })
        .unwrap();
    assert!(matches!(&s[enable+1],Step::Linear{tag,..} if tag.kind==MoveKind::LeadIn));
    let disable = s
        .iter()
        .position(|x| matches!(x, Step::CutterComp(CutterComp::Off)))
        .unwrap();
    assert!(matches!(&s[disable-1],Step::Linear{tag,..} if tag.kind==MoveKind::LeadOut));
    assert!(
        matches!(&s[disable-1],Step::Linear{to,..} if to.y < -3.),
        "Cancellation must also occur at an air-side point"
    );
}

#[test]
fn open_leads_extensions_ramp_and_alternating_levels_preserve_endpoints() {
    let mut op = profile();
    op.closed = false;
    op.side = Side::On;
    op.chain = Contour::new(vec![Point::new(0., 0.), Point::new(40., 0.)]);
    op.passes.start_extension = 5.;
    op.passes.end_extension = 10.;
    op.passes.both_ways = true;
    op.lead_in = Lead::Linear { length: 2. };
    op.lead_out = op.lead_in;
    op.plunge = Plunge::Ramp { angle_deg: 5. };
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let c = cuts(&r);
    assert_eq!((c[0].x, c[0].z), (50., -2.));
    assert_eq!((c.last().unwrap().x, c.last().unwrap().z), (-5., -4.));
    let mut short = profile();
    short.closed = false;
    short.chain = Contour::new(vec![Point::new(0., 0.), Point::new(1., 0.)]);
    short.plunge = Plunge::Ramp { angle_deg: 1. };
    let r = run(short);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}

#[test]
fn triangular_tab_count_matches_preview_and_has_no_vertical_shoulders() {
    let mut op = profile();
    op.side = Side::On;
    op.start = None;
    op.tabs.enabled = true;
    op.tabs.count = Some(3);
    op.tabs.width = 2.;
    op.tabs.height = 1.;
    op.tabs.shape = TabShape::Triangular;
    assert_eq!(profile_tab_preview(&op, 6., SpindleDir::Cw).len(), 3);
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let mut previous: Option<Point3> = None;
    for s in r.program.steps() {
        if let Step::Linear { to, tag, .. } = s {
            if tag.kind == MoveKind::Cutting {
                if let Some(p) = previous {
                    assert!((p.x - to.x).hypot(p.y - to.y) > 1e-10 || (p.z - to.z).abs() < 1e-10);
                }
            }
            previous = Some(*to);
        } else {
            previous = None;
        }
    }
    assert!(cuts(&r).iter().any(|p| p.z == -3.));
}

#[test]
fn taper_uses_the_nominal_bottom_reference_and_retract_feed_is_applied_to_motion() {
    let mut op = profile();
    op.passes.taper_angle = -10.;
    op.passes.feed_retract = true;
    op.entry_feeds.lead_out = Some(85.);
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let p = plunges(&r);
    assert!((p[0].y + 3. + 2. * (-10f64).to_radians().tan()).abs() < cam_geo::GRID_MM);
    assert!((p[1].y + 3.).abs() < cam_geo::GRID_MM);
    assert!(r
        .program
        .steps()
        .iter()
        .any(|s| matches!(s,Step::Linear{feed,tag,..} if tag.kind==MoveKind::Retract&&*feed==85.)));
    assert!(!r
        .program
        .steps()
        .iter()
        .any(|s| matches!(s,Step::Rapid{tag,..} if tag.kind==MoveKind::Retract)));
}

#[test]
fn repeated_contours_are_rejected_without_silently_deleting_them() {
    let mut op = profile();
    let mut points = op.chain.points().to_vec();
    points.rotate_left(2);
    points.reverse();
    op.additional_chains.push(ProfileContour {
        chain: Contour::new(points),
        closed: true,
        side: op.side,
    });
    let r = run(op);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}

#[test]
fn negative_roughing_allowance_reaches_the_same_wall_as_a_single_finish() {
    let mut op = profile();
    op.depth = 2.;
    op.offset = -0.2;
    op.stepover = 2.;
    let rough = run(op.clone());
    assert!(!rough.has_errors(), "{:?}", rough.diagnostics);
    assert!(cuts(&rough)
        .iter()
        .any(|p| (p.x + 2.8).abs() < cam_geo::GRID_MM || (p.y + 2.8).abs() < cam_geo::GRID_MM));
    op.stepover = 0.;
    let finish = run(op);
    assert!(!finish.has_errors());
    assert!(cuts(&finish)
        .iter()
        .any(|p| (p.x + 2.8).abs() < cam_geo::GRID_MM || (p.y + 2.8).abs() < cam_geo::GRID_MM));
    rough.program.validate().unwrap();
    finish.program.validate().unwrap();
}

#[test]
fn a_selected_irregular_stock_boundary_uses_the_shared_roughing_engine() {
    let mut op = profile();
    op.depth = 2.;
    op.stepover = 2.;
    op.passes.stock_boundary = Some(Contour::new(vec![
        Point::new(-8., -8.),
        Point::new(30., -8.),
        Point::new(30., 12.),
        Point::new(25., 30.),
        Point::new(-8., 30.),
    ]));
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert!(!r.program.is_empty());
    r.program.validate().unwrap();
}

#[test]
fn custom_sweep_leads_keep_their_angle_and_linked_exit_shape() {
    let mut op = profile();
    op.lead_in = Lead::ArcSweep {
        radius: 3.,
        angle_deg: 45.,
    };
    op.passes.same_lead_out = true;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    r.program.validate().unwrap();
    let mut previous = None;
    let mut leads = 0;
    for step in r.program.steps() {
        match step {
            Step::Arc {
                end, center, tag, ..
            } => {
                if matches!(tag.kind, MoveKind::LeadIn | MoveKind::LeadOut) {
                    let p: Point3 = previous.unwrap();
                    let a = [p.x - center.x, p.y - center.y];
                    let b = [end.x - center.x, end.y - center.y];
                    let angle = (a[0] * b[1] - a[1] * b[0])
                        .atan2(a[0] * b[0] + a[1] * b[1])
                        .abs();
                    assert!((angle - std::f64::consts::FRAC_PI_4).abs() < 1e-8);
                    leads += 1;
                }
                previous = Some(*end);
            }
            Step::Rapid { to, .. } | Step::Linear { to, .. } => previous = Some(*to),
            _ => {}
        }
    }
    assert_eq!(leads, 4);
}

#[test]
fn sharp_outer_corner_policy_changes_the_offset_geometry() {
    let mut op = profile();
    op.depth = 2.;
    op.passes.outer_corner = ProfileCorner::Sharp;
    let sharp = run(op.clone());
    assert!(!sharp.has_errors(), "{:?}", sharp.diagnostics);
    assert!(cuts(&sharp)
        .iter()
        .any(|p| (p.x + 3.).abs() < cam_geo::GRID_MM && (p.y + 3.).abs() < cam_geo::GRID_MM));
    op.passes.outer_corner = ProfileCorner::Round;
    let round = run(op);
    assert!(!cuts(&round)
        .iter()
        .any(|p| (p.x + 3.).abs() < cam_geo::GRID_MM && (p.y + 3.).abs() < cam_geo::GRID_MM));
}

#[test]
fn a_taper_that_would_require_undercutting_is_rejected() {
    let mut op = profile();
    op.passes.taper_angle = 10.;
    let r = run(op);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}

#[test]
fn lead_end_distance_splits_the_contour_before_fitting_and_clamps_to_its_length() {
    let mut op = profile();
    op.closed = false;
    op.side = Side::On;
    op.chain = Contour::new(vec![Point::new(0., 0.), Point::new(20., 0.)]);
    op.depth = 2.;
    op.passes.lead_end_distance = 5.;
    op.entry_feeds.lead_out = Some(80.);
    let r = run(op.clone());
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let feeds: Vec<_> = r
        .program
        .steps()
        .iter()
        .filter_map(|step| match step {
            Step::Linear { to, feed, tag } if tag.kind == MoveKind::Cutting => Some((to.x, *feed)),
            _ => None,
        })
        .collect();
    assert_eq!(feeds, vec![(15., 300.), (20., 80.)]);
    op.passes.lead_end_distance = 100.;
    let r = run(op);
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    assert!(r.program.steps().iter().any(|step| matches!(step, Step::Linear { to, feed, tag } if tag.kind == MoveKind::Cutting && to.x == 20. && *feed == 80.)));
}

#[test]
fn rough_final_selects_bottom_increments_independently_for_intermediate_radial_cuts() {
    let mut op = profile();
    op.passes.finishing = true;
    op.passes.finish_count = 2;
    op.passes.finishing_depths = 2;
    op.passes.finishing_stepdown = 0.5;
    op.passes.order = ProfileOrder::Radial;
    let all = run(op.clone());
    assert!(!all.has_errors(), "{:?}", all.diagnostics);
    assert_eq!(
        plunges(&all).iter().map(|p| p.z).collect::<Vec<_>>(),
        vec![-2., -3., -3.5, -4., -2., -3., -3.5, -4.]
    );
    op.passes.rough_final = false;
    let final_only = run(op);
    assert!(!final_only.has_errors(), "{:?}", final_only.diagnostics);
    assert_eq!(
        plunges(&final_only).iter().map(|p| p.z).collect::<Vec<_>>(),
        vec![-2., -4., -2., -3., -3.5, -4.]
    );
}

#[test]
fn ramp_rpm_is_restored_before_cutting_and_invalid_restoration_is_rejected() {
    let mut op = profile();
    op.plunge = Plunge::Helix {
        radius: 1.,
        pitch: 1.,
    };
    op.passes.ramp_rpm = 500.;
    let r = run(op.clone());
    assert!(!r.has_errors(), "{:?}", r.diagnostics);
    let mut rpm = op.spindle_rpm;
    let mut ramps = 0;
    for step in r.program.steps() {
        match step {
            Step::Spindle { rpm: new, dir } => {
                rpm = *new;
                assert_eq!(*dir, SpindleDir::Cw);
            }
            Step::Linear { tag, .. } | Step::Arc { tag, .. } if tag.kind == MoveKind::Plunge => {
                assert_eq!(rpm, 500.);
                ramps += 1;
            }
            Step::Linear { tag, .. } | Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting => {
                assert_eq!(rpm, 1000.)
            }
            _ => (),
        }
    }
    assert!(ramps > 0);
    assert_eq!(rpm, 1000.);
    op.spindle_rpm = 0.;
    let r = run(op);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}

#[test]
fn a_secondary_open_chain_cannot_silently_ignore_closed_contour_overlap() {
    let mut op = profile();
    op.additional_chains.push(ProfileContour {
        chain: Contour::new(vec![Point::new(40., 0.), Point::new(60., 0.)]),
        closed: false,
        side: Side::On,
    });
    op.lead_overlap = 1.;
    let r = run(op);
    assert!(r.has_errors());
    assert!(r.program.is_empty());
}
