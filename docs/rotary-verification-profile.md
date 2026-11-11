# Where rotary verification spends its time

Measured on 2026-11-09 with the supplied `screw_test.ocam`, default accuracy 1.0,
default controller machine and LinuxCNC post. Grid spacing remains approximately
0.086667 mm and pose spacing remains resolution / 4. No simulator code, toolpath
parameters or resolution were changed for this investigation.

This profile describes the original implementation. The subsequent
[implemented optimization](#implemented-optimization) retains the same sampling.

## Findings

The dominant cost is repeated access to the stock interval `BTreeMap`, especially
testing whether a cutter intersection still contains material. Cutter intersection
math is a smaller part of the cost. For this cold, full-job verification,
checkpoint copying and interval sorting are secondary.

Two release runs with short native CPU samples took 123.200 s and 120.459 s in
Verification. A longer sampled run took 132.628 s in Verification (133.176 s for
the full worker job). Sampling and machine scheduling affect elapsed time; these
are individual observations, not a controlled speed comparison.

Per-operation timing from the longer run, using the worker's progress index:

| Operation | Verification time | Share |
| --- | ---: | ---: |
| 1: first indexed pocket | 0.265 s | 0.2% |
| 2: second indexed pocket | 1.599 s | 1.2% |
| 3: wrapped pocket | 130.754 s | 98.6% |
| Untagged/final progress | 0.009 s | <0.1% |

Progress polling has approximately millisecond-scale boundary uncertainty. The
result had 5,404 steps, 688,992 final triangles, no diagnostics and zero collisions,
matching the counts from the earlier release benchmarks. The new `--profile`
option also emitted the result digest `2b43d21b1e528a3c`, covering program,
diagnostics, collisions, initial/final meshes and their metadata. Earlier runs did
not emit this digest, so matching counts are not a claim of full buffer equality.

## CPU sample breakdown

macOS `sample` captured native stacks at a requested 1 ms interval. The long sample
started 17.7 seconds after process launch and continued through completion; it
therefore primarily describes the expensive wrapped operation. It captured
90,026 stacks of the `machining-job` worker. The main thread's sleeps and other
threads' idle stacks are excluded from the denominator.

Exclusive categories below do not double-count callees. In particular,
`subtract_interval` excludes its volume-accounting and sorting calls.

| Region | Worker samples | Share |
| --- | ---: | ---: |
| `State::motion`, including inlined remaining-material/contact queries | 51,911 | 57.66% |
| `ray_tool`: finite cutter/body intersection | 13,504 | 15.00% |
| `subtract_interval`: map access, merging, mesh invalidation and remaining local work | 12,287 | 13.65% |
| `track_removed_interval`: removed-volume accounting | 11,214 | 12.46% |
| Interval-list sorting called by subtraction | 952 | 1.06% |
| Other worker work, including final preparation | 158 | 0.18% |

These are sampled stack shares, not independently timed function budgets. The
first row includes traversal and coordinate arithmetic as well as inlined queries;
it must not be interpreted as a precise standalone `BTreeMap::get` measurement.

However, symbol lookup and DWARF inspection identify the hottest instruction
locations in that row as the B-tree's `find_key_index` search, at these call sites:

- Cutting material test in
  [`rotary_simulation.rs`](../crates/cam-sim/src/rotary_simulation.rs)
  (line 778 in the profiled version).
  Its native call-tree region contains 48,540 worker samples (53.92%).
- Non-cutting-body stock contact in the same file
  (line 737 in the profiled version).
  Its inlined-query region contains another 2,159 samples (2.40%).

The hottest locations inside subtraction resolve to the map entry lookup in
[`rotary_stock.rs`](../crates/cam-sim/src/rotary_stock.rs) (original line 183). The hottest
volume-accounting locations resolve to the additional map lookup at
[`heightfield.rs`](../crates/cam-sim/src/heightfield.rs) (original line 494). DWARF also resolves
the native `<deduplicated_symbol>` called by subtraction to the short-list sorting
implementation, rather than to mesh invalidation.

The two shorter samples support the same diagnosis: their motion-loop shares
were 58.2% and 51.2%, volume-accounting shares 13.7% and 18.1%, and ray-intersection
shares 10.2% and 9.2%. The mix changes along the wrapped cut; the longer sample is
the basis for the table above.

Raw local evidence is retained in `/tmp/opencam-rotary-full.sample.txt`, with a
worker-only excerpt in `/tmp/opencam-rotary-worker.sample.txt` and benchmark output
in `/tmp/opencam-rotary-profile-run.txt`. These temporary files are local evidence,
not portable repository fixtures.

## Why stock queries are expensive

The hot path performs three searches of the same map for a changed column:

1. `interval_material` checks whether the intersected interval still has stock.
2. `track_removed_interval` looks up the existing voids again to compute newly
   removed volume.
3. `subtract_interval` looks up the column entry again to update the voids.

The horizontal cylindrical blank already records a bottom void for essentially
every occupied column. Thus this rotary use of the map is dense from the start,
despite the representation being designed for sparse bounded voids. At this
resolution the grid has 263,400 columns.

The work repeats over roughly 459,000 poses. Cleared material is represented by
void intervals while `column_top` retains the original upper surface. The early
`ceiling <= floor` rejection therefore cannot generally skip columns emptied by
these rotary subtractions: they still reach the ray/material queries. This helps
explain why checking for remaining material costs more than updating it.

Volume accounting is enabled unconditionally by `State::new`, including normal
stock previews whose application caller does not consume `removed_volume`.
Force analysis and existing simulator clients do need that value, so any change
must preserve their behavior and explicitly distinguish the display-only request.

There is no evidence in these samples that allocating new interval vectors,
checkpoint cloning or sorting is the main cause of the two-minute runtime.
This does not measure the cost of repeated playback frames or rewinds, where
checkpoint copying may have a different importance.

## Optimization priorities supported by this profile

1. **Remove repeated tree searches.** Prototype directly indexed column storage
   inside tiles, preserving exactly the same interval endpoints, ownership and
   source metadata. Keep a sparse/tiled option for cases where most columns do not
   need void lists. A hash map still requires a lookup for every query; dense
   rotary columns are candidates for direct indexing.
2. **Reuse a column lookup through a cutting update.** Share the interval list
   between material testing, volume accounting and subtraction. This also avoids
   rescanning the list when its relevant overlaps are already known.
3. **Reject cleared regions before the expensive queries.** Maintain conservative
   occupied bounds/flags for columns or tiles, distinct from the original blank
   top. Preserve all disconnected intervals and thin residual material.
4. **Make removed-volume accounting optional for callers that do not consume it.**
   Its sampled share is about 12.5% here. That share is an optimization target,
   not a measured speedup from disabling it.
5. **Then parallelize disjoint tiles/columns and improve cutter queries.** Keep
   time ordering, deterministic collision handling and force-volume reduction.
   Improving only `ray_tool` addresses about 15% of the sampled work; even making
   that part free would imply only about a 1.18x speedup if everything else stayed
   unchanged. A swept-volume backend could also reduce the number of stock
   queries, but needs separate correctness validation.

Sorting, mesh invalidation batching and copy-on-write checkpoints remain useful
follow-ups. The measurements put interval access ahead of them for this full run.
All of the first four proposals can retain the current resolution and pose
sequence; no accuracy reduction is needed to test them.

## Reproduction

The benchmark's `--profile` option reports operation timings and a deterministic
result digest. It does not instrument or change the simulation loop; digest
serialization happens after the reported job timing.

```sh
cargo build --release --config 'profile.release.package.cam-sim.debug=2' -p cam-app --example rotary_verify_bench
target/release/examples/rotary_verify_bench /path/to/screw_test.ocam --simulate --timeout 600 --profile
```

For native sampling, attach to that benchmark PID while it prints `Verification`:

```sh
sample BENCHMARK_PID 150 1 -mayDie -fullPaths -file /tmp/rotary.sample.txt
xcrun dsymutil target/release/examples/rotary_verify_bench -o /tmp/rotary.dSYM
```

The operating system may require additional permission to sample the process.
Use `atos` with the report's load address for source call sites and `dwarfdump
--lookup` on the corresponding unslid addresses to inspect inlined B-tree
searches. When comparing samples, use exclusive worker counts, not all-thread
totals or inclusive parent-plus-child counts.

## Implemented optimization

The stock interval map is now
[`IntervalColumns`](../crates/cam-sim/src/interval_columns.rs): directly indexed,
row-major pages of 256 columns. Pages allocate on first use, and checkpoints share
them with `Arc`; writes copy only a shared page. Interval lists, ordering, float
precision, operation ownership and surface provenance are retained. Iteration
still visits columns in ascending order, preserving volume summation order.

Subtraction accounts for removed volume through the same column lookup used to
update its intervals. `RotarySimulationCache` also exposes `set_volume_tracking`:
it defaults to enabled, while the application's geometry/stock preview lane
disables unused accounting. Changing this mode invalidates incompatible cached
states. The separate force-analysis traversal keeps volume accounting enabled.

Cylindrical blanks additionally restrict candidate windows to a conservative
cylinder envelope, including possible outward rounding of stored f32 endpoints.
Exact ray tests and serial collision/removal order remain unchanged. General
stock retains the existing bounding-box fallback.

On the supplied project, replacing interval storage alone reduced release
verification to **41.126 s**. With shared volume/update queries, optional volume
tracking and the narrower candidate search, the next run took **40.278 s**,
compared with the earlier **119–123 s** observations. Both optimized runs retained
the original result digest **`2b43d21b1e528a3c`**, 5,404 steps, 688,992 triangles,
zero collisions and no diagnostics. Grid spacing and pose spacing are unchanged.
These timings are individual local measurements, not isolated benchmark medians.

A subsequent paired release run used the preserved pre-change executable and
the optimized executable sequentially on the same current project snapshot,
without native CPU sampling:

| | Before | After |
| --- | ---: | ---: |
| Verification | 118.045 s | 40.942 s |
| Full worker job | 118.482 s | 41.262 s |
| Result digest | `2b43d21b1e528a3c` | `2b43d21b1e528a3c` |

That is a **2.88x verification speedup**, or **65.3% less verification time**.
Both jobs were saved to temporary copies; a streaming byte comparison confirmed
that the **entire 430,082,287-byte project outputs were identical**, including
document, toolpaths, saved verification meshes and metadata. The original input
was not overwritten. The paired benchmark logs remain at
`/tmp/opencam-rotary-before-optimization.txt` and
`/tmp/opencam-rotary-after-optimization.txt`; the large temporary output copies
were removed after comparison.

Validation includes:

- Exact comparisons with a test-only B-tree backend and unbounded candidate
  searches for A/B motion, flat/ball/bull/cone/rounded-V/thread cutters, hollow
  blanks, repeated cuts, air turns and colliding indexes. The comparison includes
  meshes and surface sources, collisions, removed volume and force samples, both
  near zero and at an offset of 10,000.2 mm in Z.
- Page boundary and copy-on-write checkpoint isolation checks.
- Volume-tracking mode changes with cached state and unchanged stock results.
- Existing forward/rewind/edit/cancellation and stock-transfer regressions.
- 69 `cam-sim` unit tests, 236 application unit tests, seven rotary integration
  tests, and a GUI-feature compile check. One existing user-file-dependent
  application test remains ignored.

```sh
cargo test -p cam-sim --lib
cargo test -p cam-app --lib --test rotary_stepped --test rotary_wrap
cargo check -p cam-app --features gui
```

This change optimizes verification and shared stock storage. Project encoding
and saved verification meshes remain unchanged; the project I/O recommendations
in the [initial investigation](rotary-performance-investigation.md) are separate.
