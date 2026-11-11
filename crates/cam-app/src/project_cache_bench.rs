//! Opt-in measurements of project caches and complete-motion playback prefixes.
use super::*;
use crate::{default_machine, AppController};

/// Profile derived-data rebuilds and seek reuse against an external project.
/// OCAM_PROFILE_PROJECT=/path/job.ocam OCAM_PROFILE_ACCURACY=0.75 cargo test
/// --release -p cam-app --lib project_stock_cache_benchmark -- --ignored --nocapture
#[test]
#[ignore = "requires OCAM_PROFILE_PROJECT; performs full rotary simulations"]
fn project_stock_cache_benchmark() {
    use std::{collections::BTreeMap, time::Instant};
    let path = std::env::var("OCAM_PROFILE_PROJECT").expect("set OCAM_PROFILE_PROJECT");
    let accuracy = std::env::var("OCAM_PROFILE_ACCURACY")
        .map(|s| s.parse::<f32>().unwrap())
        .unwrap_or(1.);
    let crate::OcamFile::Project(mut project) = crate::OcamFile::read(&path).unwrap() else {
        panic!("expected project");
    };
    let programs: BTreeMap<_, _> = project
        .toolpaths
        .verification
        .iter()
        .map(|(id, result)| (*id, result.program.clone()))
        .collect();
    let mut operation_cache = std::mem::take(&mut project.toolpaths.cache);
    project.toolpaths.verification.clear();
    let core = project.to_json().unwrap();
    drop(project);
    let scratch =
        std::env::temp_dir().join(format!("stock-cache-bench-{}.ocam", std::process::id()));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&scratch)
        .unwrap();
    file.write_all(core.as_bytes()).unwrap();
    drop(file);
    let mut app = AppController::new(default_machine());
    app.set_post_kind(cam_post::PostKind::LinuxCnc);
    let start = Instant::now();
    let opened = app.open_project(&scratch);
    let open_time = start.elapsed();
    std::fs::remove_file(scratch).unwrap();
    opened.unwrap();
    app.set_simulation_accuracy(accuracy);
    println!(
        "core document {} bytes; open {:.3} s; accuracy {accuracy}",
        core.len(),
        open_time.as_secs_f64()
    );
    let mut inputs = app.job_inputs();
    let saved_program = programs
        .get(&inputs.document.setup().id)
        .expect("saved program for active setup");
    let cancel = CancelToken::new();
    let progress = Progress::default();
    // Browser path generation skips stock verification, retaining the normal planner.
    let mut generation = inputs.clone();
    generation.toolpath_preview = true;
    let start = Instant::now();
    let generated = generation.execute(
        &cancel,
        &mut operation_cache,
        &progress,
        &mut Default::default(),
    );
    assert_eq!(
        generated.completion,
        JobCompletion::Complete,
        "{:?}",
        generated.diagnostics
    );
    println!(
        "cache-assisted toolpath regeneration {:.3} s; {} steps; saved program {} steps",
        start.elapsed().as_secs_f64(),
        generated.program.steps().len(),
        saved_program.steps().len()
    );
    let start = Instant::now();
    let cold = generation.execute(
        &cancel,
        &mut OperationCache::default(),
        &progress,
        &mut Default::default(),
    );
    assert_eq!(
        cold.completion,
        JobCompletion::Complete,
        "{:?}",
        cold.diagnostics
    );
    println!(
        "uncached toolpath regeneration {:.3} s",
        start.elapsed().as_secs_f64()
    );
    assert_eq!(cold.program, generated.program);
    if std::env::var_os("OCAM_PROFILE_GENERATION_ONLY").is_some() {
        return;
    }
    let program = &generated.program;
    let resolution = inputs.stock_resolution(program);
    inputs.preview_program = Some(program.clone());
    inputs.preview_resolution = Some(resolution);
    let start = Instant::now();
    let initial = inputs
        .defined_round_stock(resolution, &cancel)
        .unwrap()
        .expect("round stock");
    println!(
        "initial dexel stock {:.3} s; grid {:?}; resolution {resolution:.6} mm",
        start.elapsed().as_secs_f64(),
        initial.dims()
    );
    let start = Instant::now();
    let blank_mesh = initial.to_solid_mesh(inputs.min[2]);
    println!(
        "high-resolution starting-stock mesh {:.3} s; {} vertices",
        start.elapsed().as_secs_f64(),
        blank_mesh.positions.len()
    );
    drop(blank_mesh);
    let start = Instant::now();
    let analytic =
        crate::stock_shape::oriented_round_mesh(&inputs.document, inputs.document.setup())
            .unwrap()
            .unwrap();
    println!(
        "analytic viewport blank {:.6} s; {} vertices",
        start.elapsed().as_secs_f64(),
        analytic.positions.len()
    );
    drop(analytic);
    let mut simulation = StockSimulationCache::default();
    // Complete-motion prefixes exercise the same stable-prefix/checkpoint policy
    // as playback. Fractions here refer to step count, not timeline duration.
    for (name, fraction, sources) in [
        ("cold end, material coloring", 1., false),
        ("rewind to 95% of steps", 0.95, false),
        ("forward to 96%", 0.96, false),
        ("repeat 96%", 0.96, false),
        ("cold end, force surface tracking", 1., true),
    ] {
        inputs.stock_force_surfaces = sources;
        let count = ((program.steps().len() as f64 * fraction) as usize).max(1);
        let mut prefix = Program {
            steps: program.steps()[..count].to_vec(),
        };
        let start = Instant::now();
        let result = inputs
            .simulate_field(
                &mut prefix,
                resolution,
                Some(("unchanged-blank", &initial)),
                &cancel,
                &progress,
                &mut simulation.current,
                &mut simulation.rotary,
            )
            .unwrap();
        let verification = start.elapsed();
        let start = Instant::now();
        let mesh = simulation
            .mesh
            .mesh_controlled(&result.field, inputs.min[2], &|| false)
            .unwrap();
        let meshing = start.elapsed();
        let start = Instant::now();
        let vertices = inputs.world_stock_vertices(&mesh, &cancel).unwrap();
        println!("{name}: stock {:.3} s; mesh {:.3} s; world vertices {:.3} s; {} vertices; {} collisions",
            verification.as_secs_f64(), meshing.as_secs_f64(), start.elapsed().as_secs_f64(), vertices.len(), result.collisions.len());
        if fraction == 1. && !sources {
            let start = Instant::now();
            let rebuilt = cam_sim::StockMeshCache::default()
                .mesh_controlled(&result.field, inputs.min[2], &|| false)
                .unwrap();
            assert_eq!(rebuilt, mesh);
            println!(
                "rebuild final mesh from retained dexels {:.3} s",
                start.elapsed().as_secs_f64()
            );
        }
    }
    let start = Instant::now();
    let samples = inputs
        .analyze_stock_removal(&cancel, &mut operation_cache, &progress)
        .unwrap();
    println!(
        "separate force-removal analysis {:.3} s; {} samples",
        start.elapsed().as_secs_f64(),
        samples.len()
    );
}
