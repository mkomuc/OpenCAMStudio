//! Measure legacy/new project I/O without changing the source.
//! cargo run --release -p cam-app --example project_io_bench -- project.ocam
//! Optional: --save-copy /tmp/new-project.ocam --accuracy 0.75
use cam_app::{default_machine, AppController, OcamFile};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("Supply a .ocam project path")?;
    let start = Instant::now();
    let bytes = std::fs::read(path)?;
    println!(
        "read: {:.3} s; {} bytes",
        start.elapsed().as_secs_f64(),
        bytes.len()
    );
    let start = Instant::now();
    let OcamFile::Project(project) = OcamFile::from_bytes(&bytes)? else {
        return Err("Expected a project".into());
    };
    println!(
        "decode + migration + validation: {:.3} s",
        start.elapsed().as_secs_f64()
    );
    for (id, result) in &project.toolpaths.verification {
        println!(
            "setup {id}: {} steps; stock snapshot {} bytes; legacy vertices {}",
            result.program.steps().len(),
            result
                .stock_snapshot
                .as_ref()
                .map_or(0, |s| s.as_bytes().len()),
            result.stock_vertices.len()
        );
    }
    let start = Instant::now();
    let archive = project.to_archive()?;
    println!(
        "archive encode: {:.3} s; {} bytes ({:.2}% of input)",
        start.elapsed().as_secs_f64(),
        archive.len(),
        archive.len() as f64 / bytes.len() as f64 * 100.
    );
    let start = Instant::now();
    let OcamFile::Project(restored) = OcamFile::from_bytes(&archive)? else {
        unreachable!()
    };
    println!("archive decode: {:.3} s", start.elapsed().as_secs_f64());
    assert_eq!(restored.document, project.document);
    assert_eq!(restored.toolpaths.cache, project.toolpaths.cache);
    for (id, before) in &project.toolpaths.verification {
        let after = &restored.toolpaths.verification[id];
        assert_eq!(before.program, after.program);
        assert_eq!(before.diagnostics, after.diagnostics);
        assert_eq!(before.collisions, after.collisions);
        assert_eq!(before.stock_snapshot, after.stock_snapshot);
        assert_eq!(before.stock_vertices, after.stock_vertices);
        assert_eq!(before.stock_indices, after.stock_indices);
        assert_eq!(before.stock_machined, after.stock_machined);
        assert_eq!(before.stock_operations, after.stock_operations);
        assert_eq!(before.stock_cut_sources, after.stock_cut_sources);
    }
    println!("editable document, paths, verification and final stock: equal");
    let machine = project.machine.clone().unwrap_or_else(default_machine);
    let post = project.post;
    drop(restored);
    drop(project);
    drop(bytes);
    drop(archive);
    let mut app = AppController::new(machine);
    if let Some(post) = post {
        app.set_post_kind(post);
    }
    if let Some(i) = args.iter().position(|a| a == "--accuracy") {
        app.set_simulation_accuracy(args[i + 1].parse()?);
    }
    let start = Instant::now();
    app.open_project(path)?;
    println!(
        "controller open + mesh restore: {:.3} s; verification restored {}",
        start.elapsed().as_secs_f64(),
        app.outcome().is_some()
    );
    if let Some(i) = args.iter().position(|a| a == "--compare-stock") {
        let reference = args
            .get(i + 1)
            .ok_or("--compare-stock requires a legacy reference project")?;
        let OcamFile::Project(reference) = OcamFile::read(reference)? else {
            return Err("Expected a reference project".into());
        };
        let before = reference
            .toolpaths
            .verification
            .get(&app.active_setup_id())
            .ok_or("No reference verification")?;
        let after = app.outcome().ok_or("No restored verification")?;
        assert_eq!(before.program, after.program);
        assert_eq!(before.collisions, after.collisions);
        assert_eq!(before.stock_vertices, after.stock_vertices);
        assert_eq!(before.stock_indices, after.stock_indices);
        assert_eq!(before.stock_machined, after.stock_machined);
        assert_eq!(before.stock_operations, after.stock_operations);
        assert_eq!(before.stock_cut_sources, after.stock_cut_sources);
        println!("reference project program, collisions, render geometry and provenance: equal");
    }
    if let Some(i) = args.iter().position(|a| a == "--save-copy") {
        let copy = args.get(i + 1).ok_or("--save-copy requires a new path")?;
        if std::path::Path::new(copy).exists() {
            return Err("The save-copy destination must not exist".into());
        }
        let original = app.outcome().cloned();
        let exportable = app.export_nc().is_ok();
        let start = Instant::now();
        app.save_project(copy)?;
        println!(
            "atomic controller save + readback: {:.3} s; {} bytes",
            start.elapsed().as_secs_f64(),
            std::fs::metadata(copy)?.len()
        );
        let start = Instant::now();
        app.open_project(copy)?;
        println!(
            "controller reopen + mesh restore: {:.3} s",
            start.elapsed().as_secs_f64()
        );
        assert_eq!(app.export_nc().is_ok(), exportable);
        if let Some(before) = original {
            let after = app.outcome().ok_or("Saved verification was not restored")?;
            assert_eq!(before.program, after.program);
            assert_eq!(before.collisions, after.collisions);
            assert_eq!(before.stock_vertices, after.stock_vertices);
            assert_eq!(before.stock_indices, after.stock_indices);
            assert_eq!(before.stock_machined, after.stock_machined);
            assert_eq!(before.stock_operations, after.stock_operations);
            assert_eq!(before.stock_cut_sources, after.stock_cut_sources);
            println!("restored render geometry, provenance and export status: equal");
        }
    }
    Ok(())
}
