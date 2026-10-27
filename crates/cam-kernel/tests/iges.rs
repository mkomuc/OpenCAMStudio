#![cfg(feature = "occt")]
use cam_kernel::{read_iges_file, KernelError};
use cam_model::{CadSurface, SourceUnits};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
fn iges_solids_and_sewn_surfaces_use_millimetres() {
    assert!(cam_kernel::backend_available());
    for (name, units) in [
        ("box-mm.iges", SourceUnits::Millimetres),
        ("box-inch.igs", SourceUnits::Inches),
        ("box-m.iges", SourceUnits::Metres),
        ("box-surfaces-mm.igs", SourceUnits::Millimetres),
    ] {
        let import = read_iges_file(fixture(name), &Default::default()).unwrap();
        assert_eq!(import.source_units, units, "{name}");
        import.topology.validate().unwrap();
        assert_eq!(import.topology.bodies.len(), 1, "{name}");
        assert_eq!(import.placements.len(), 1);
        let body = &import.topology.bodies[0];
        assert!(body.valid_solid, "{name}");
        for (actual, expected) in body
            .bounds
            .iter()
            .flatten()
            .zip([0., 0., -10., 60., 40., 0.])
        {
            assert!(
                (actual - expected).abs() < 1e-6,
                "{name}: {:?}",
                body.bounds
            );
        }
        assert_eq!(import.topology.faces.len(), 6);
        assert!(import
            .topology
            .faces
            .iter()
            .all(|f| matches!(f.surface, CadSurface::Plane { .. })));
        assert!(!import.topology.mesh.triangles.is_empty());
        assert!(import.topology.projection.is_some());
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    }
}

#[test]
fn spline_planes_are_recognized_without_flattening_a_bulged_interior() {
    for (name, planes) in [
        ("spline-box-surfaces-mm.igs", 6),
        ("spline-bulge-surfaces-mm.igs", 5),
    ] {
        let import = read_iges_file(fixture(name), &Default::default()).unwrap();
        import.topology.validate().unwrap();
        assert!(import.topology.bodies[0].valid_solid, "{name}");
        assert_eq!(import.topology.faces.len(), 6);
        assert_eq!(import.topology.faces.iter().filter(|face| {
            matches!(face.surface, CadSurface::Plane { .. })
        }).count(), planes, "{name}");
        // Normals must retain the solid's outward orientation, also for reversed
        // faces; setup alignment and entry checks depend on this sign.
        let center = [30., 20., -5.];
        for face in &import.topology.faces {
            if let CadSurface::Plane { origin, normal } = face.surface {
                let outward: f64 = (0..3).map(|i| (origin[i] - center[i]) * normal[i]).sum();
                assert!(outward > 0., "{name}: {face:?}");
            }
        }
    }
}

#[test]
fn iges_imports_every_body_and_preserves_circular_geometry() {
    for name in ["two-boxes-mm.iges", "two-boxes-surfaces-mm.iges"] {
        let import = read_iges_file(fixture(name), &Default::default()).unwrap();
        import.topology.validate().unwrap();
        assert_eq!(import.topology.bodies.len(), 2);
        assert_eq!(import.placements.len(), 2);
        assert!(import.topology.bodies.iter().all(|body| body.valid_solid));
    }
    let import = read_iges_file(fixture("box-hole-mm.iges"), &Default::default()).unwrap();
    import.topology.validate().unwrap();
    assert!(import.topology.faces.iter().any(|f| matches!(f.surface,
        CadSurface::Cylinder { radius, inward: true, .. } if (radius - 2.5).abs() < 1e-6)));
}

#[test]
fn iges_rejects_open_surfaces_even_alongside_solids() {
    for name in ["open-surface-mm.iges", "solids-and-open-surface-mm.iges"] {
        let error = read_iges_file(fixture(name), &Default::default()).unwrap_err();
        assert!(matches!(error, KernelError::Translation(_)));
        assert!(error.to_string().contains("open surfaces"), "{error}");
    }
}

#[test]
fn malformed_iges_reports_a_translation_error() {
    let path = std::env::temp_dir().join(format!("malformed-iges-{}.igs", std::process::id()));
    std::fs::write(&path, "This is not IGES data.").unwrap();
    let result = read_iges_file(&path, &Default::default());
    std::fs::remove_file(path).unwrap();
    assert!(matches!(result, Err(KernelError::Translation(_))));
}

#[test]
fn nested_surface_shells_cannot_silently_fill_a_void() {
    let error =
        read_iges_file(fixture("nested-surfaces-mm.iges"), &Default::default()).unwrap_err();
    assert!(
        error.to_string().contains("nested surface shells"),
        "{error}"
    );
}

#[test]
fn unsupported_units_and_wire_geometry_fail_explicitly() {
    for (name, message) in [
        ("unsupported-unit-cm.iges", "source units"),
        ("wire-mm.iges", "wire geometry"),
    ] {
        let error = read_iges_file(fixture(name), &Default::default()).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}
