#![cfg(feature = "occt")]
use cam_kernel::{read_iges_file, read_step_file, KernelError, SolidImport};
use cam_model::{CadCurve, CadSurface};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}
fn import(name: &str) -> Result<SolidImport, KernelError> {
    if name.ends_with(".step") {
        read_step_file(fixture(name), &Default::default())
    } else {
        read_iges_file(fixture(name), &Default::default())
    }
}

#[test]
fn degree_elevation_does_not_hide_a_cylindrical_bore() {
    for name in [
        "elevated-spline-hole-mm.step",
        "elevated-spline-hole-mm.iges",
    ] {
        let cad = import(name).unwrap();
        cad.topology.validate().unwrap();
        assert!(cad.topology.bodies[0].valid_solid, "{name}");
        assert_eq!(
            cad.topology
                .faces
                .iter()
                .filter(|face| matches!(face.surface,
            CadSurface::Cylinder { radius, inward: true, .. } if (radius - 2.5).abs() < 1e-8))
                .count(),
            1,
            "{name}"
        );
        assert!(
            !cad.topology
                .edges
                .iter()
                .any(|edge| matches!(edge.curve, CadCurve::Unsupported { .. })),
            "{name}"
        );
    }
}

#[test]
fn conic_edges_keep_their_complete_trim_as_exact_rational_curves() {
    for kind in ["ellipse", "parabola", "hyperbola"] {
        for ext in ["step", "iges"] {
            let name = format!("{kind}-prism-mm.{ext}");
            let cad = import(&name).unwrap();
            cad.topology.validate().unwrap();
            assert!(cad.topology.bodies[0].valid_solid, "{name}");
            assert!(
                !cad.topology
                    .edges
                    .iter()
                    .any(|edge| matches!(edge.curve, CadCurve::Unsupported { .. })),
                "{name}: {:?}",
                cad.warnings
            );
            let mut curved = 0;
            for edge in &cad.topology.edges {
                let CadCurve::BezierSpans { spans } = &edge.curve else {
                    continue;
                };
                curved += 1;
                for span in spans {
                    // Independently check the preserved control data against
                    // each conic's defining equation, including trim endpoints.
                    for t in [0., 0.13, 0.57, 0.97, 1.] {
                        let mut p: Vec<_> = span
                            .poles
                            .iter()
                            .zip(&span.weights)
                            .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
                            .collect();
                        for n in (1..p.len()).rev() {
                            for i in 0..n {
                                for c in 0..4 {
                                    p[i][c] = (1. - t) * p[i][c] + t * p[i + 1][c];
                                }
                            }
                        }
                        let x = p[0][0] / p[0][3] - 30.;
                        let y = p[0][1] / p[0][3] - 20.;
                        let residual = match kind {
                            "ellipse" => x * x / 100. + y * y / 25. - 1.,
                            "parabola" => y * y - 12. * x,
                            "hyperbola" => x * x / 25. - y * y / 9. - 1.,
                            _ => unreachable!(),
                        };
                        assert!(
                            residual.abs() < 1e-7,
                            "{name} edge {} t={t}: residual={residual}",
                            edge.id
                        );
                    }
                }
            }
            assert_eq!(curved, 2, "{name}");
        }
    }
}

#[test]
fn cones_and_spheres_retain_analytic_metadata() {
    for name in ["cone-sphere-mm.step", "cone-sphere-mm.iges"] {
        let cad = import(name).unwrap();
        cad.topology.validate().unwrap();
        assert_eq!(cad.topology.bodies.len(), 2, "{name}");
        assert!(
            cad.topology.bodies.iter().all(|body| body.valid_solid),
            "{name}"
        );
        assert_eq!(
            cad.topology
                .faces
                .iter()
                .filter(|face| matches!(face.surface, CadSurface::Cone { .. }))
                .count(),
            1,
            "{name}"
        );
        assert_eq!(cad.topology.faces.iter().filter(|face| matches!(face.surface, CadSurface::Sphere { radius, .. } if (radius - 5.).abs() < 1e-9)).count(), 1, "{name}");
    }
}

#[test]
fn closed_step_shells_form_solids_without_dropping_open_or_nested_parts() {
    let cad = import("closed-shell-mm.step").unwrap();
    cad.topology.validate().unwrap();
    assert_eq!(cad.topology.bodies.len(), 1);
    assert!(cad.topology.bodies[0].valid_solid);
    assert_eq!(cad.topology.faces.len(), 6);
    for (name, reason) in [
        ("solid-and-open-shell-mm.step", "open surfaces"),
        ("nested-shells-mm.step", "nested surface shells"),
    ] {
        let error = import(name).unwrap_err();
        assert!(error.to_string().contains(reason), "{name}: {error}");
    }
}

#[test]
fn unsupported_surface_details_are_reported_once_on_import() {
    let cad = import("spline-barrel-mm.iges").unwrap();
    assert!(cad.topology.faces.iter().any(|face| matches!(&face.surface,
        CadSurface::Unsupported { description } if description.contains("B-spline surface"))));
    assert_eq!(
        cad.warnings
            .iter()
            .filter(|warning| warning.contains("no analytic feature representation"))
            .count(),
        1
    );
}

#[test]
fn repair_cannot_turn_an_incomplete_body_into_a_valid_solid() {
    let cad = import("incomplete-parabola-mm.iges").unwrap();
    cad.topology.validate().unwrap();
    assert_eq!(cad.topology.bodies.len(), 1);
    assert!(!cad.topology.bodies[0].valid_solid);
    assert!(cad
        .warnings
        .iter()
        .any(|warning| warning.contains("bounded repair did not succeed")));
}
