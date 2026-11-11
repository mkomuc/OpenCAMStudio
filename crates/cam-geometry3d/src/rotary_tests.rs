use super::*;
fn cylindrical_point(x: f64, degrees: f64) -> [f64; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [x, 20. * s, 20. * c]
}
fn patch(x0: f64, x1: f64, a0: f64, a1: f64) -> Vec<[f64; 3]> {
    vec![
        cylindrical_point(x0, a0),
        cylindrical_point(x1, a0),
        cylindrical_point(x1, a1),
        cylindrical_point(x0, a1),
    ]
}
#[test]
fn rotary_development_keeps_seam_crossing_islands_and_rejects_stale_geometry() {
    let mut f = Fixture::new();
    let face = f.face(
        vec![patch(10., 30., 170., 190.), patch(15., 25., 175., 185.)],
        [0., 0., 1.],
    );
    let mut geometry = f.state();
    let wrap = RotaryWrap {
        radius: 20.,
        geometry: vec![reference(face, EntityKind::Face)],
        ..Default::default()
    };
    let contours = unwrap_boundaries(&geometry, &wrap).unwrap();
    assert_eq!(contours.len(), 2);
    let regions = wrapped_regions(&contours).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes().len(), 1);
    let expected = 20. * 20. * 20_f64.to_radians() - 10. * 20. * 10_f64.to_radians();
    assert!(
        (regions[0].area() - expected).abs() < 0.002,
        "{} vs {expected}",
        regions[0].area()
    );
    let points = contours[0].chain.points();
    assert!(points.windows(2).all(|w| (w[1].y - w[0].y).abs() < 2.));
    geometry.assets[0].revision += 1;
    assert!(unwrap_boundaries(&geometry, &wrap)
        .unwrap_err()
        .contains("stale"));
}
#[test]
fn rotary_cylinder_uses_placement_and_rejects_tilted_or_non_cylindrical_faces() {
    let mut f = Fixture::new();
    let face = f.face(vec![square(0., 10., 0., 10., 0.)], [0., 0., 1.]);
    let mut geometry = f.state();
    let r = reference(face, EntityKind::Face);
    assert!(wrap_cylinder(&geometry, &r, RotaryWrap::default()).is_err());
    std::sync::Arc::make_mut(&mut geometry.assets[0].topology).faces[0].surface =
        CadSurface::Cylinder {
            origin: [0.; 3],
            axis: [1., 0., 0.],
            radius: 20.,
            inward: false,
        };
    geometry.instances[0].placement = RigidTransform {
        rotation: [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
        translation: [12., 34., 56.],
    };
    let wrap = wrap_cylinder(&geometry, &r, RotaryWrap::default()).unwrap();
    assert_eq!(wrap.axis, RotaryAxis::B);
    assert_eq!(wrap.center, [12., 34., 56.]);
    assert_eq!(wrap.radius, 20.);
    std::sync::Arc::make_mut(&mut geometry.assets[0].topology).faces[0].surface =
        CadSurface::Cylinder {
            origin: [0.; 3],
            axis: [0., 0., 1.],
            radius: 20.,
            inward: false,
        };
    assert!(wrap_cylinder(&geometry, &r, RotaryWrap::default())
        .unwrap_err()
        .contains("Align"));
}
#[test]
fn rotary_full_winding_boundaries_require_patches() {
    let mut f = Fixture::new();
    let face = f.face(
        vec![(0..36)
            .map(|i| cylindrical_point(10., i as f64 * 10.))
            .collect()],
        [1., 0., 0.],
    );
    let geometry = f.state();
    let wrap = RotaryWrap {
        radius: 20.,
        geometry: vec![reference(face, EntityKind::Face)],
        ..Default::default()
    };
    assert!(unwrap_boundaries(&geometry, &wrap)
        .unwrap_err()
        .contains("winds completely"));
}

#[test]
fn rotary_cad_boundary_height_is_radial_and_follows_the_wrap_offset() {
    let mut f = Fixture::new();
    f.face(vec![square(10., 30., 0., 1., 20.)], [0., 0., 1.]);
    let geometry = f.state();
    let edge = geometry.assets[0].topology.loops[0].edge_uses[0].edge_id;
    let mut wrap = RotaryWrap {
        radius: 25.,
        geometry: vec![reference(edge, EntityKind::Edge)],
        ..Default::default()
    };
    assert_eq!(
        wrapped_boundary_height(&geometry, &wrap).unwrap(),
        Some(-5.)
    );
    wrap.offset = -2.;
    wrap.surface_z = 3.;
    assert_eq!(wrapped_boundary_height(&geometry, &wrap).unwrap(), Some(0.));
}

#[test]
fn rotary_periodic_cylinder_face_develops_its_seam_into_a_full_width_band() {
    let mut f = Fixture::new();
    let bounds = [[10., -20., -20.], [30., 20., 20.]];
    f.t.bodies[0].bounds = bounds;
    f.t.vertices = vec![
        CadVertex {
            id: 10,
            body_id: 1,
            point: [10., 0., 20.],
        },
        CadVertex {
            id: 11,
            body_id: 1,
            point: [30., 0., 20.],
        },
    ];
    let circle = |x| CadCurve::CircleArc {
        center: [x, 0., 0.],
        normal: [-1., 0., 0.],
        x_axis: [0., 0., 1.],
        radius: 20.,
        start_angle: 0.,
        end_angle: TAU,
    };
    f.t.edges = vec![
        CadEdge {
            id: 20,
            body_id: 1,
            vertices: [10, 10],
            face_ids: vec![40],
            curve: circle(10.),
        },
        CadEdge {
            id: 21,
            body_id: 1,
            vertices: [10, 11],
            face_ids: vec![40],
            curve: CadCurve::Line {
                start: [10., 0., 20.],
                end: [30., 0., 20.],
            },
        },
        CadEdge {
            id: 22,
            body_id: 1,
            vertices: [11, 11],
            face_ids: vec![40],
            curve: circle(30.),
        },
    ];
    f.t.loops = vec![CadLoop {
        id: 30,
        body_id: 1,
        face_id: 40,
        is_outer: true,
        edge_uses: vec![
            EdgeUse {
                edge_id: 20,
                reversed: false,
            },
            EdgeUse {
                edge_id: 21,
                reversed: false,
            },
            EdgeUse {
                edge_id: 22,
                reversed: true,
            },
            EdgeUse {
                edge_id: 21,
                reversed: true,
            },
        ],
    }];
    f.t.faces = vec![CadFace {
        id: 40,
        body_id: 1,
        bounds,
        loop_ids: vec![30],
        surface: CadSurface::Cylinder {
            origin: [0.; 3],
            axis: [1., 0., 0.],
            radius: 20.,
            inward: false,
        },
    }];
    let geometry = f.state();
    let wrap = RotaryWrap {
        radius: 25.,
        geometry: vec![reference(40, EntityKind::Face)],
        ..Default::default()
    };
    let contours = unwrap_boundaries(&geometry, &wrap).unwrap();
    let regions = wrapped_regions(&contours).unwrap();
    assert_eq!(regions.len(), 1);
    assert!(
        (regions[0].area() - 20. * 25. * TAU).abs() < 0.01,
        "{}",
        regions[0].area()
    );
    assert!((wrapped_boundary_height(&geometry, &wrap).unwrap().unwrap() + 5.).abs() < 1e-8);
}
