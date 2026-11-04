# Calculation performance

CPU acceleration is enabled automatically on macOS, Windows and Linux, in both
desktop and headless builds. No GPU or vendor SDK is required for calculations.

- **3D Parallel:** independent rows, including swept-cutter refinement, run on
  multiple cores in bounded batches. Linking and motion emission retain row order.
- **3D Scallop:** cutter-contact surface samples run on multiple cores. Distance
  propagation, contour ordering and linking retain their sequential algorithms.
- **Simulation preview:** changed stock mesh tiles build on multiple cores, then
  assemble in tile order, preserving operation and cutting-move ownership.
- **Stock-to-CAD comparison:** unique uncached positions are classified on
  multiple cores; duplicate vertices continue sharing cached results.

These paths share Rayon's worker pool. It chooses the available logical CPU count
by default; the operating system schedules workers across performance and
efficiency cores on Apple Silicon. Set `RAYON_NUM_THREADS` **before launching** to
limit CPU use, or set it to `1` for sequential execution. A single-worker run
retains SIMD. Small jobs and sequential stages may see little improvement.

```sh
# macOS / Linux: limit the application to four calculation workers
RAYON_NUM_THREADS=4 ./opencamstudio
```

```powershell
# Windows PowerShell
$env:RAYON_NUM_THREADS = "4"
.\opencamstudio.exe
```

## Apple Silicon and SIMD

The native ARM64 build uses the `wide` library for four double-precision
triangle bounding-box tests at a time. On Apple Silicon this uses NEON; standard
x86-64 builds use SSE2, and other architectures use the library's fallback.
The filter retains the scalar formula and contact tolerance. Exact triangle
contact and swept-cutter checks still follow; coordinates are not reduced to
single precision. No nightly Rust or machine-specific build flags are needed.

For a build used only on your own machine, `RUSTFLAGS="-C target-cpu=native"`
can enable additional CPU instructions during a release build. Keep distributable
Windows/Linux binaries on their baseline CPU target so they run on older machines.

The viewport already uses GPU rendering through `wgpu`. Toolpath generation,
stock removal and verification do not use GPU compute. A GPU calculation backend
would need separate accuracy and conservative-error validation across Metal,
Direct3D and Vulkan before replacing these double-precision safety checks.
Stock removal and collision detection also depend on preceding moves and remain
in machining order.

## Reproduce measurements

Local release measurements on Apple Silicon with ten CPU workers, using the
synthetic benchmarks below:

| Work | Before | Accelerated |
|---|---:|---:|
| Parallel finishing | 39.7 ms | 10.0 ms |
| Scallop finishing | 125.7 ms | 106.9 ms |
| Fresh stock mesh at 0.083 mm (whole mesh vs tiled) | 30.2 ms | 6.0 ms |
| Isolated bounds filter (scalar vs SIMD) | 34.2 ms | 29.7 ms |

Finishing result hashes matched the original implementation and one-worker runs.
These are example workloads; improvement depends on model size, strategy and CPU.

Run the same release benchmark with one worker and with the automatic count:

```sh
RAYON_NUM_THREADS=1 cargo run --release -p cam-toolpath --example finishing_bench
cargo run --release -p cam-toolpath --example finishing_bench
cargo run --release -p cam-sim --example preview_bench
cargo test --release -p cam-toolpath --lib bounds_filter_benchmark -- --ignored --nocapture
```

The finishing benchmark reports median warm-run times and a result hash. Compare
hashes on the same machine/build to check that worker count does not change the
generated motions or diagnostics. The bounds benchmark compares SIMD with the
scalar filter. Tests additionally compare complete one-worker/four-worker
toolpath and stock-mesh results, check scalar/SIMD filter equality, and exercise
cancellation and cache reuse. CI runs finishing and simulation checks on macOS
ARM64, Windows x86-64 and Linux x86-64.
