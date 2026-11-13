//! cargo run --release -p cam-toolpath --example finishing_bench
//! Compare RAYON_NUM_THREADS=1 with the default on the same machine.
use cam_cldata::SpindleDir;
use cam_geometry3d::ParallelGeometry;
use cam_model::{CuttingHeights, Heights, ParallelOp, ParallelParams, Tool, ToolKind};
use cam_toolpath::{CancelToken, JobEnv, ParallelStrategy, Strategy};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Instant,
};

fn main() {
    let point = |x: usize, y: usize| {
        let x = x as f64 * 2.;
        let y = y as f64 * 2.;
        [x, y, (x * 0.08).sin() * (y * 0.08).cos()]
    };
    let mut triangles = Vec::new();
    for y in 0..24 {
        for x in 0..24 {
            triangles.push([point(x, y), point(x + 1, y), point(x + 1, y + 1)]);
            triangles.push([point(x, y), point(x + 1, y + 1), point(x, y + 1)]);
        }
    }
    let geometry = ParallelGeometry {
        selected: triangles.clone(),
        triangles,
        error_bound: 0.0025,
        heights: CuttingHeights {
            top: 2.,
            bottom: -2.,
        },
    };
    let tools = [Tool {
        kind: ToolKind::BallMill,
        diameter: 4.,
        number: 1,
        length: 20.,
        ..Default::default()
    }];
    let env = JobEnv {
        heights: Heights::new(10., 5., 2.),
        tools: &tools,
        stock: None,
        spindle: SpindleDir::Cw,
    };
    println!("CPU workers: {}", rayon::current_num_threads());
    for scallop in [false, true] {
        let operation = ParallelOp {
            parameter_expressions: Default::default(),
            id: 1,
            tool: 1,
            work_offset: 1,
            spindle_rpm: 10_000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            params: ParallelParams {
                scallop,
                stepover: 1.,
                ..Default::default()
            },
        };
        let strategy = ParallelStrategy::new(operation, Default::default(), geometry.clone());
        let mut times = Vec::new();
        let mut hash = 0;
        let mut steps = 0;
        for _ in 0..4 {
            let start = Instant::now();
            let result = strategy.compute(&env, &CancelToken::new());
            times.push(start.elapsed().as_secs_f64() * 1000.);
            assert!(
                !result.cancelled && !result.has_errors(),
                "{:?}",
                result.diagnostics
            );
            steps = result.program.steps().len();
            let mut hasher = DefaultHasher::new();
            serde_json::to_string(&result).unwrap().hash(&mut hasher);
            hash = hasher.finish();
            std::hint::black_box(result);
        }
        // Ignore the first run (pool startup and cold caches).
        times.remove(0);
        times.sort_by(f64::total_cmp);
        println!(
            "{}: {:.2} ms median, {steps} steps, result {hash:016x}",
            if scallop { "Scallop" } else { "Parallel" },
            times[1]
        );
    }
}
