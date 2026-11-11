//! Reproduce a rotary project without a GUI and time the worker phases.
//! cargo run -p cam-app --example rotary_verify_bench -- project.ocam [--simulate] [--timeout 600]
//! --profile reports operation timings from progress polling and a stock-result digest.
use cam_app::{default_machine, AppController};
use cam_cldata::Step;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("Supply a .ocam project path")?;
    let simulate = args.iter().any(|a| a == "--simulate");
    let profile = args.iter().any(|a| a == "--profile");
    let timeout = args
        .iter()
        .position(|a| a == "--timeout")
        .map(|i| {
            args.get(i + 1)
                .ok_or("--timeout requires seconds")?
                .parse::<u64>()
                .map_err(|_| "Invalid timeout")
        })
        .transpose()?
        .unwrap_or(180);
    let mut app = AppController::new(default_machine());
    let start = Instant::now();
    app.open_project(path)?;
    app.set_post_kind(cam_post::PostKind::LinuxCnc);
    println!(
        "load {:.3} s; stock {:?}",
        start.elapsed().as_secs_f64(),
        app.stock_box()
    );
    if args.iter().any(|a| a == "--split-cylinders") {
        let source = app
            .document()
            .setup()
            .operations
            .first()
            .cloned()
            .ok_or("No operation")?;
        let references = source
            .rotary_wrap()
            .ok_or("First operation is not wrapped")?
            .geometry
            .clone();
        let settings = app
            .document()
            .setup()
            .height_settings
            .get(&source.id())
            .cloned();
        let geometry = app
            .document()
            .geometry
            .geometry_in_setup(&app.document().setup().frame)?;
        for (n, face) in references.into_iter().enumerate() {
            let mut operation = source.clone();
            let mut wrap = cam_geometry3d::wrap_cylinder(
                &geometry,
                &face,
                operation.rotary_wrap().unwrap().clone(),
            )?;
            wrap.geometry = vec![face];
            operation.set_rotary_wrap(Some(wrap));
            let id = if n == 0 {
                let id = source.id();
                app.edit_operation(id, |saved| *saved = operation);
                id
            } else {
                app.add_operation(operation);
                app.document().setup().operations.last().unwrap().id()
            };
            if let Some(settings) = settings.clone() {
                app.edit_setup(|s| {
                    s.height_settings.insert(id, settings);
                });
            }
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--accuracy") {
        app.set_simulation_accuracy(args[i + 1].parse()?);
    }
    let start = Instant::now();
    if simulate {
        app.start_run();
    } else {
        app.start_toolpath_run(None, true)?;
    }
    let mut last = Instant::now();
    let mut phase = "Queued".to_string();
    let mut times = BTreeMap::<String, f64>::new();
    let mut verification = Vec::<(f64, Option<usize>)>::new();
    while app.run_pending() {
        app.poll_run();
        if let Some(p) = app.run_progress() {
            if profile {
                let step = (p.phase == cam_app::JobPhase::Verification).then_some(p.completed);
                if verification.last().map(|v| v.1) != Some(step) {
                    verification.push((start.elapsed().as_secs_f64(), step));
                }
            }
            let next = format!("{:?}", p.phase);
            if next != phase {
                *times.entry(phase.clone()).or_default() += last.elapsed().as_secs_f64();
                last = Instant::now();
                phase = next.clone();
                println!("phase {next}");
            }
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            app.cancel_run();
            return Err(format!("Timed out after {timeout} seconds").into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    *times.entry(phase).or_default() += last.elapsed().as_secs_f64();
    verification.push((start.elapsed().as_secs_f64(), None));
    let out = app.outcome().ok_or("No outcome")?;
    println!(
        "completion {:?}; total {:.3} s; phases {:?}; {} steps; {} triangles; {} collisions",
        out.completion,
        start.elapsed().as_secs_f64(),
        times,
        out.program.steps().len(),
        out.stock_indices.len() / 3,
        out.collisions.len()
    );
    println!("diagnostics {:?}", out.diagnostics);
    if profile {
        let mut operations = BTreeMap::<Option<u32>, f64>::new();
        for pair in verification.windows(2) {
            if let Some(index) = pair[0].1 {
                let operation = out
                    .program
                    .steps()
                    .get(index)
                    .and_then(Step::tag)
                    .map(|t| t.op_id);
                *operations.entry(operation).or_default() += pair[1].0 - pair[0].0;
            }
        }
        println!("verification seconds by operation (polled): {operations:?}");
        // Hash all persisted result buffers in serialization order without allocating
        // a second full mesh. This is a reproducibility check, not a security hash.
        struct Digest(u64);
        impl std::io::Write for Digest {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                for byte in bytes {
                    self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut digest = Digest(0xcbf29ce484222325);
        serde_json::to_writer(
            &mut digest,
            &(
                &out.program,
                &out.diagnostics,
                &out.collisions,
                &out.initial_stock,
                &out.stock_vertices,
                &out.stock_indices,
                &out.stock_machined,
                &out.stock_operations,
                &out.stock_cut_sources,
            ),
        )?;
        println!("stock result digest: {:016x}", digest.0);
    }
    let mut radii = BTreeMap::<u32, (f64, f64)>::new();
    for step in out.program.steps() {
        if let Step::Rotary {
            to,
            center,
            tag,
            feed: Some(_),
            ..
        } = step
        {
            if tag.kind == cam_cldata::MoveKind::Cutting {
                let r = to.z - center.z;
                let e = radii.entry(tag.op_id).or_insert((r, r));
                e.0 = e.0.min(r);
                e.1 = e.1.max(r);
            }
        }
    }
    println!("cutting radius ranges: {radii:?}");
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        std::fs::write(
            &args[i + 1],
            serde_json::to_vec(
                &serde_json::json!({"document":app.document(),"program":out.program,"min":app.stock_box().0,"max":app.stock_box().1,"phases":times}),
            )?,
        )?;
    }
    if let Some(i) = args.iter().position(|a| a == "--save") {
        let start = Instant::now();
        app.save_project(&args[i + 1])?;
        println!(
            "save {:.3} s; {} bytes",
            start.elapsed().as_secs_f64(),
            std::fs::metadata(&args[i + 1])?.len()
        );
    }
    Ok(())
}
