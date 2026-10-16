//! Reproducible CPU preview benchmark: cargo run -p cam-sim --release --example preview_bench
use cam_cldata::{MoveKind, Point3, ProgramBuilder};
use cam_sim::{simulate, SimOptions, SimulationCache};
use std::time::Instant;

fn main() {
    let mut builder = ProgramBuilder::new()
        .feed(1000.)
        .rapid(Point3::new(80., 50., 5.), MoveKind::Link);
    for layer in 1..=8 {
        let z = -f64::from(layer);
        builder = builder.linear(Point3::new(80., 50., z), MoveKind::Plunge);
        for i in 1..=720 {
            let a = f64::from(i) * std::f64::consts::TAU / 720.;
            builder = builder.linear(
                Point3::new(50. + 30. * a.cos(), 50. + 30. * a.sin(), z),
                MoveKind::Cutting,
            );
        }
    }
    let program = builder.build();
    for resolution in [0.333333333, 0.166666667, 0.083333334] {
        let start = Instant::now();
        let sim = simulate(
            &program,
            [0., 0., -10.],
            [100., 100., 0.],
            &SimOptions {
                resolution,
                tool_radius: 3.,
            },
            &[],
        );
        let cut = start.elapsed();
        let mesh = sim.field.to_solid_mesh(-10.);
        println!(
            "{resolution:.3} mm: cut {:.2} ms, mesh {:.2} ms, {} triangles, {:.1} MiB mesh",
            cut.as_secs_f64() * 1000.,
            (start.elapsed() - cut).as_secs_f64() * 1000.,
            mesh.indices.len() / 3,
            (mesh.positions.len() * 25 + mesh.indices.len() * 4) as f64 / 1048576.
        );
        let mut cache = SimulationCache::default();
        let opts = SimOptions {
            resolution,
            tool_radius: 3.,
        };
        let mut frames = Vec::new();
        let mut last_mesh = None;
        for end in (1..program.steps.len()).step_by(72) {
            let prefix = cam_cldata::Program {
                steps: program.steps[..end].to_vec(),
            };
            let start = Instant::now();
            let sim = cache
                .simulate_controlled(
                    &prefix,
                    [0., 0., -10.],
                    [100., 100., 0.],
                    &opts,
                    &[],
                    &|| false,
                    |_, _| {},
                )
                .unwrap();
            last_mesh = Some(sim.field.to_solid_mesh(-10.));
            frames.push(start.elapsed().as_secs_f64() * 1000.);
        }
        std::hint::black_box(last_mesh);
        frames.sort_by(f64::total_cmp);
        println!(
            "  incremental cut + mesh: median {:.2} ms, p95 {:.2} ms ({} frames)",
            frames[frames.len() / 2],
            frames[frames.len() * 95 / 100],
            frames.len()
        );
    }
}
