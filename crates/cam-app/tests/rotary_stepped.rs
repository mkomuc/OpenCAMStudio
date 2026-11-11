//! No native CAD SDK or user files: exact analytic STEP topology is embedded.
use cam_app::{default_machine, AppController};
use cam_cldata::{MoveKind, Step};
use cam_model::{EntityKind, Operation};
use cam_toolpath::{CancelToken, JobCompletion};

fn project() -> AppController {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "stepped-rotary-{}-{}.ocam",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, include_str!("fixtures/stepped_rotary.ocam")).unwrap();
    let mut app = AppController::new(default_machine());
    app.open_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    app.set_post_kind(cam_post::PostKind::LinuxCnc);
    app.set_simulation_accuracy(0.5);
    app
}
fn split(app: &mut AppController) {
    let source = app.document().setup().operations[0].clone();
    let settings = app.document().setup().height_settings[&source.id()].clone();
    let refs = source.rotary_wrap().unwrap().geometry.clone();
    for (i, face) in refs.into_iter().enumerate() {
        let mut op = source.clone();
        let mut wrap = op.rotary_wrap().unwrap().clone();
        assert_eq!(face.kind, EntityKind::Face);
        wrap.cylinder = Some(face.clone());
        wrap.geometry = vec![face];
        op.set_rotary_wrap(Some(wrap));
        let id = if i == 0 {
            let id = source.id();
            app.edit_operation(id, |saved| *saved = op);
            id
        } else {
            app.add_operation(op);
            app.document().setup().operations.last().unwrap().id()
        };
        app.edit_setup(|s| {
            s.height_settings.insert(id, settings.clone());
        });
    }
}

#[test]
fn mixed_model_radii_cannot_silently_fall_back_to_a_four_mm_gouge() {
    let mut app = project();
    let out = app.run(&CancelToken::new());
    assert_eq!(out.completion, JobCompletion::Failed);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.message.contains("different radii")),
        "{:?}",
        out.diagnostics
    );
    assert!(out
        .program
        .steps()
        .iter()
        .all(|s| !matches!(s, Step::Rotary { feed: Some(_), .. })));
    assert!(app.export_nc().is_err());
}

#[test]
fn split_cylinder_bands_reach_model_radii_clear_stock_and_cover_the_seam() {
    let mut app = project();
    split(&mut app);
    let out = app.run(&CancelToken::new());
    assert_eq!(
        out.completion,
        JobCompletion::Complete,
        "{:?}",
        out.diagnostics
    );
    assert!(out.collisions.is_empty(), "{:?}", out.collisions);
    let mut radii = std::collections::BTreeMap::<u32, Vec<f64>>::new();
    let mut angles = std::collections::BTreeMap::<u32, (f64, f64)>::new();
    for step in out.program.steps() {
        if let Step::Rotary {
            to,
            center,
            angle,
            feed: Some(_),
            tag,
            ..
        } = step
        {
            if tag.kind == MoveKind::Cutting {
                radii.entry(tag.op_id).or_default().push(to.z - center.z);
                let e = angles.entry(tag.op_id).or_insert((*angle, *angle));
                e.0 = e.0.min(*angle);
                e.1 = e.1.max(*angle);
            }
        }
    }
    assert!((radii[&0].iter().copied().fold(f64::INFINITY, f64::min) - 15.).abs() < 1e-6);
    assert!((radii[&1].iter().copied().fold(f64::INFINITY, f64::min) - 10.).abs() < 1e-6);
    assert!(
        radii.values().flatten().all(|r| *r <= 16.0001),
        "stock top must be measured in the source frame"
    );
    for step in out.program.steps() {
        if let Step::Rotary {
            to,
            center,
            feed: None,
            ..
        } = step
        {
            assert!(
                to.z - center.z >= 20.9999,
                "rapid must clear the radius-16 blank: {step:?}"
            );
        }
    }
    for range in angles.values() {
        assert!(range.1 - range.0 >= 359.99, "{range:?}");
    }
    // Nominal CAD coordinates are vertical; a cut stock skin must have no
    // interior caps, stray triangles, missing angular wedge, or deep gouges.
    let mut counts = [0; 2];
    let mut stray = Vec::new();
    for v in &out.stock_vertices {
        assert!(v
            .position
            .iter()
            .chain(v.normal.iter())
            .all(|v| v.is_finite()));
        let [x, y, z] = v.position.map(f64::from);
        let radius = x.hypot(y);
        let region = if z > 0.5 && z < 9.5 {
            Some((0, 15.))
        } else if z > 10.5 && z < 39.5 {
            Some((1, 10.))
        } else {
            None
        };
        if let Some((i, target)) = region {
            counts[i] += 1;
            if (radius - target).abs() >= 0.3 {
                stray.push((x, y, z, radius, target));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "{} stray vertices, first {:?}, last {:?}",
        stray.len(),
        &stray[..stray.len().min(8)],
        &stray[stray.len().saturating_sub(8)..]
    );
    assert!(counts.into_iter().all(|n| n > 100));
    assert!(out.stock_indices.len() > 100);
    app.export_nc().unwrap();
    assert!(matches!(
        app.document().setup().operations[0],
        Operation::Pocket(_)
    ));
}
