# Rotary simulation and project I/O: screw_test.ocam

Investigation on 2026-11-09. The supplied project was read without changing it.
Only benchmark utilities and this report were changed; the production simulator,
project format, grid resolution and pose spacing are unchanged.

A subsequent [native CPU profile](rotary-verification-profile.md) identifies
repeated stock-interval map searches as the dominant hotspot. It reports the
per-operation and sampled-function breakdown and refines the CPU priorities below.

## Measurements

The input is 245,895,627 bytes (245.9 MB / 234.5 MiB), schema 45. It has three
operations: two indexed pockets and one wrapped pocket, using a diameter-6 flat
end mill. The saved program contains 5,404 steps.

Section sizes below are measured by compact Rust serialization after loading;
they are not resident memory sizes.

| Data | Size | Detail |
| --- | ---: | --- |
| Editable document, including embedded geometry | 127,799 bytes | About 0.052% of the file |
| Operation cutting-fragment cache | 171,171 bytes | Small relative to verification |
| Saved verification | 245,595,386 bytes | About 99.88% of the file |
| Starting-stock mesh, inside verification | 144,153,024 bytes | 1,190,632 vertices; 595,316 triangles |
| Final mesh vertices and indices, inside verification | 93,427,131 bytes | 780,260 vertices; 390,130 triangles |

The rest of verification includes the full program and per-vertex machined and
operation arrays. Starting stock alone accounts for 58.6% of the input file.
Dropping all verification would make files small but would discard reusable
results and make reopening/reverification slower; that is not the recommendation.

Local headless timings, excluding compilation:

| Operation | Development build | Release build |
| --- | ---: | ---: |
| Read original JSON into a string | 0.027 s | 0.024 s |
| Current migrating loader and project validation | 6.794 s | 0.923 s |
| Clone snapshot, serialize tagged JSON and validate | 4.865 s | 0.355 s |
| Experimental direct typed parse and validation | 4.245 s | 0.386 s |
| Controller open through the normal load path | 6.647 s | 0.938 s |
| Controller save, including atomic write/readback | 17.896 s | 2.205 s |

The typed-parse experiment uses already migrated, current-schema JSON and checks
complete `Project` equality with the normal loader's result. It does **not**
implement migration or replace the production loader.

Lossless ZIP experiments on the original bytes, using the existing `zip` crate:

| Compression | Archive bytes | Original size retained | Release encode | Release decode |
| --- | ---: | ---: | ---: | ---: |
| Deflate level 1 | 17,014,667 | 6.9% | 0.158 s | 0.174 s |
| Deflate level 6 | 11,220,386 | 4.6% | 1.209 s | 0.138 s |

Both archive decodes were checked byte for byte against the source. These are
in-memory compression timings, excluding project parsing, disk sync and controller
work. Compression by itself still leaves the expanded JSON parsing cost.
Level 1 is the more promising first trial for interactive saves on this input.

An unchanged-default-accuracy development run through the normal controller and
worker completed in **123.883 s**:

| Worker phase | Time |
| --- | ---: |
| Geometry resolution | 0.003 s |
| Generation | 0.329 s |
| Verification | 123.326 s |
| Render preparation | 0.223 s |

It produced 5,404 steps, 688,992 final stock triangles, no diagnostics and zero
reported collisions. The freshly produced mesh differs from the persisted mesh;
the saved file is not assumed to be an equality oracle for today's simulator.
The release run produced the same counts in **119.541 s**, including **119.063 s**
in verification, **0.323 s** in generation and **0.152 s** in render preparation.
Saving that newly verified result took **3.846 s** and produced **430,211,278 bytes**;
this differs from the I/O table's resave of the original cached result. Release
optimization alone therefore does not resolve the rotary simulation bottleneck.
Its grid spacing is approximately **0.086667 mm**, with **600 × 439 columns**;
pose spacing remains at most one quarter of that resolution. Applying the current
pose-count formula to the saved rapid/rotary steps gives **459,123 pose evaluations**
before air rejection (437,737 in the wrapped operation alone).

These are individual wall-clock observations on the shared arm64 development
machine, not isolated benchmark medians. Some I/O experiments overlapped the
simulation. Development already compiles `cam-sim` at optimization level 3, while
JSON and much application code remain unoptimized. Compare like build profiles;
do not use the development I/O times as release-performance predictions.
Controller benchmarks use `default_machine()` and default accuracy 1.0; the rotary
benchmark selects LinuxCNC. The loader retains its normal fingerprint checks for
restoring saved verification rather than forcing a cache hit.

## What the code is doing

### Simulation

[`rotary_simulation.rs`](../crates/cam-sim/src/rotary_simulation.rs) keeps disjoint
material intervals in a stationary XY dexel grid. For each motion it samples
translation plus rotary travel at `resolution / 4`, then scans candidate columns
for every pose. That pose × column multiplier is the primary algorithmic cost;
the phase timings establish verification as the dominant phase, but do not yet
attribute percentages to its individual functions.

Several optimizations already exist: per-pose sine/cosine, stock-box clipping,
radial rejection of clear tool-body sections, unchanged-prefix reuse and
operation-boundary checkpoints. Merely adding those again will not address this
case. Checkpoints currently clone complete `Heightfield` states, including the
interval map; the cache also clones saved checkpoints and working state on a new
request.

[`rotary_stock.rs`](../crates/cam-sim/src/rotary_stock.rs) repeatedly searches a
`BTreeMap<usize, Vec<RemovedSpan>>` to test material, account removed volume and
subtract intervals. Each subtraction pushes, sorts and merges a list and calls
mesh invalidation, which issues an atomic revision increment. A horizontal round
blank inserts a bottom void into essentially every occupied column, so the
supposedly sparse map is already dense before machining.

`State::new` enables incremental volume tracking for every rotary run. Normal
[`simulate_stock`](../crates/cam-app/src/job_simulation.rs) consumes the field and
collisions, but not `SimResult.removed_volume`. Force analysis and public simulator
clients do need that value, so disabling accounting must be an explicit request
mode, preserving the existing API's results by default.

Playback uses a separate `JobService` in
[`gui_simulation.rs`](../crates/cam-app/src/gui_simulation.rs). Opening playback
does not reuse the full job's dexel checkpoints. Saved verification contains
meshes, not a resumable dexel field. Forward frames reuse their own stable prefix;
rewinds within a long operation can still replay from an earlier operation
checkpoint. The disposable partial last step is intentionally recalculated.

[`surface.rs`](../crates/cam-sim/src/surface.rs) already caches 32 × 32 mesh tiles
and rebuilds dirty tiles with Rayon. However, it assembles every cached tile into
new full-mesh vectors on each frame, after which world-space vertices are rebuilt.
That is a playback-copying opportunity, though initial meshing is not the main
cost in the measured full run.

### Project I/O

[`project.rs`](../crates/cam-app/src/project.rs) parses the entire file into
`serde_json::Value`, migrates the embedded document, then deserializes typed
structures. The intermediate tree includes millions of numeric mesh values even
though document migrations only need the small editable document.

[`save_project`](../crates/cam-app/src/controller.rs) clones the document and all
saved verification meshes into a snapshot, then creates a complete JSON string.
`atomic_save` fully parses and validates that string **before** writing, writes
and syncs a temporary file, reads it back, fully parses and validates it **again**,
then renames it. These two large parse passes explain much of the save cost.
Opening and remembering a verification result also clone its mesh buffers.

## Recommended implementation order

These are proposed changes, not measured simulator speedups. Keep the existing
grid, pose sequence, interval endpoints and tolerance rules for the first stages.

1. **Make project I/O lossless and cheaper.** Introduce a versioned archive with
   compact editable JSON and independently compressed verification blocks; retain
   a reader for current tagged and legacy untagged JSON. Start with fast Deflate
   using the existing dependency. Deserialize bulk arrays directly and migrate
   only the document subtree; the document's version remains authoritative and
   newer versions must still be refused. Stream output to the temporary file and
   remove the duplicate pre-write parse after equivalent typed validation exists.
   Keep sync, readback validation, atomic rename and failure cleanup. This targets
   both size and load/save time without changing any geometry.

2. **Stop persisting redundant display data.** Reconstruct the starting-stock
   mesh lazily from the document's blank definition and source geometry. For this
   file that avoids persisting 144 MB of derived JSON. Preserve fast setup-view
   behavior with a cache. In a subsequent archive version, encode remaining mesh
   arrays as binary blocks using the same f32 values/u32 indices, with lossless
   bitsets/run-length encoding for flags and operation IDs. Keep program,
   diagnostics, collisions, fingerprints and surface provenance together; do not
   mark unverified jobs verified or silently discard their results.

3. **Reduce work in the existing CPU kernel.** Profile column visits, ray tests,
   changed intervals, allocations, volume accounting and checkpoint-copy bytes.
   Fuse the material test/account/subtract operation into one lookup and maintain
   sorted intervals without sorting the entire list on every insertion. Accumulate
   dirty tile IDs and issue revisions once per changed tile/batch. Add the explicit
   no-volume-accounting mode for display-only consumers. These retain the same
   sampled geometry and are the smallest simulation changes to validate first.

4. **Use tiled interval storage with copy-on-write snapshots.** Keep the general
   multiple-interval representation, but replace dense rotary use of the tree with
   contiguous tile/column indexing and pooled or inline short interval lists.
   Share untouched tiles between checkpoints, workers and playback. Add bounded
   checkpoints within long operations so a seek does not replay the whole wrapped
   pocket. Cache the defined round blank instead of rebuilding it per frame.
   Tile occupancy and conservative material bounds can reject whole regions
   before exact ray tests. Existing prefix/cancellation semantics must survive.

5. **Specialize A/B ray tests and parallelize disjoint columns.** For A, the axial
   X coordinate is invariant under rotation; for B, Y is invariant. Reuse the
   corresponding ray terms across rows/columns, and bound searches by active flute
   and probe sections rather than the full stickout capsule. Parallelize columns
   or tiles within a time-ordered pose/batch, not successive dependent motions.
   Preserve deterministic first-contact behavior: the current serial loop can
   remove material in earlier columns before finding a collision in a later one.
   A parallel reduction must reproduce that order or explicitly validate a changed
   collision/removal contract. Reduce volume in deterministic order for force tests.

6. **Then evaluate cutter sweeps and GPU execution.** Intersect a column with a
   bounded cutter sweep over a motion, reducing the pose multiplier. Start with
   fixed-index flat tools before continuously rotating and non-flat profiles.
   Keeping the same XY grid alone does not prove equal accuracy: sweep boundaries,
   collision fractions, thin walls and force sampling need separate error bounds.
   GPU tiled dexel subtraction is a later backend candidate once the CPU data is
   contiguous and its contracts are tested; include transfer/readback cost when
   measuring it. A specialized polar radius grid is insufficient for general
   indexed pockets, tubes and disconnected material intervals in this application.

For playback, carrying dirty tile meshes through to GPU buffer updates can avoid
full-mesh assembly/transforms/uploads each frame. This should follow kernel and
snapshot improvements, since the measured full-run meshing time is already small.

Do not achieve a speedup by increasing grid spacing, widening contact tolerance,
skipping thin residual material or coarsening the pose spacing. Lossless archive
compression, storage layout changes and exact work reuse avoid that tradeoff.

## Validation and reproduction

The new I/O benchmark checks typed-project equality and byte-exact archive
round trips. `--save-copy` refuses an existing destination. It writes a copy only
when that option is provided; the input project is never overwritten.

```sh
cargo run -p cam-app --example project_io_bench -- /path/to/screw_test.ocam
cargo run --release -p cam-app --example project_io_bench -- /path/to/screw_test.ocam --save-copy /tmp/screw-copy.ocam
cargo run -p cam-app --example rotary_verify_bench -- /path/to/screw_test.ocam --simulate --timeout 600
cargo run --release -p cam-app --example rotary_verify_bench -- /path/to/screw_test.ocam --simulate --timeout 600
cargo check -p cam-app --example project_io_bench --example rotary_verify_bench
```

For implementation, compare optimized and reference backends on identical inputs
and resolution. Cover A/B axes, indexed and continuous paths, multiple turns,
flat/ball/bull/conical/thread tools, tubes, previous-setup stock, rapid and shank
contacts, cancellation, forward playback, rewind and edited prefixes. Compare
material intervals/meshes, ownership/provenance, collisions and fractions, removed
volume and force samples. Existing rotary kernel tests already compare bounded
versus unbounded search and cached versus fresh runs. I/O changes additionally
need legacy migration, too-new schema, exact float round trips, restored export
eligibility, corrupt/truncated archives and failed-save preservation tests.

Run repeated release measurements with the same machine, accuracy and program;
record peak memory and seek/frame latency as well as full-run wall time. The
[follow-up profile](rotary-verification-profile.md) distinguishes cutter-query time
from interval storage and volume accounting; playback snapshot/frame costs still
need separate measurement before assigning expected speedups to those changes.
