# Architecture review: validation evidence

Review date: **2026-10-13**. Audited source: [`5c510b504fe39f4e40700ff3b1e331d0309fcad3`](https://github.com/pr0m1th3as/OpenCAMStudio/tree/5c510b504fe39f4e40700ff3b1e331d0309fcad3). Companion [analysis](architecture-analysis.md) and [implementation plan](implementation-plan.md).

This appendix records observations, not fixes. Application source was not modified during the review. The standalone [probe](architecture-validation-probe.rs) is outside the Cargo crate source/test tree.

## Environment and test outcome

Host: macOS, Apple silicon (`arm64`). Rust: `rustc 1.99.0 (b940084d7 2026-09-28)`. Cargo: `cargo 1.99.0 (5f94df478 2026-08-27)`. Repository toolchain is `stable`, with declared minimum Rust 1.88. Because `stable` floats, this is an environment-specific observation, not a claim that the same four tests fail on every previous compiler/platform.

The initial offline attempt could not resolve uncached `iced`; network access was then authorized to download locked dependencies. `cargo test --workspace --locked` compiled and ran, failing on `cam-toolpath` unit tests. A second complete collection run used:

```sh
cargo test --workspace --locked --offline --no-fail-fast
```

**Result: 671 passed, 4 failed, 23 ignored**, including two passing doctests. Exit status 101. All four failures are in the `cam-toolpath` library target; that target reports 216 passed, 4 failed, 19 ignored. Remaining executed headless targets pass. The failures appeared in both runs.

| Failed test | Location | Observed assertion |
|---|---|---|
| `clearing::tests::a_re_entry_descends_at_cutting_feed_from_the_stock_top` | `crates/cam-toolpath/src/clearing.rs:504` | 0 re-entries at cutting feed vs 22 at plunge feed |
| `clearing::tests::an_adaptive_re_entry_does_not_helix_through_cleared_stock` | `crates/cam-toolpath/src/clearing.rs:643` | Expected steered path, got 12 rings; left 12, right 1 |
| `clearing::tests::a_holed_region_routes_through_the_certified_steered_clearer` | `crates/cam-toolpath/src/clearing.rs:667` | Expected one continuous certified path; left 12, right 1 |
| `clearing::tests::the_adaptive_clearer_declines_a_reversed_spindle` | `crates/cam-toolpath/src/clearing.rs:745` | M3 climb + cap expected to reach steered generator; left 12, right 1 |

These observations suggest a discrepancy between expected adaptive dispatch/re-entry behavior and the produced concentric fallback. They do **not** establish whether the algorithm, certification tolerance, test expectations, dependency/compiler behavior or platform causes that discrepancy. Diagnosis is M0 work. Successful fallback is also not evidence that the requested engagement constraints are fully satisfied everywhere.

The headless suite excludes GUI-feature tests. No GUI window, physical machine, controller emulator, full-machine collision test, CAD backend benchmark or cutting trial was run. The CI configuration was inspected, but its remote results were not treated as proof of the local checkout's behavior.

## Public-API counterexamples

The following output was observed by compiling a standalone Rust program against the unmodified workspace libraries:

```text
full circle X span 40 vs travel 10: export accepted = true
NaN position and negative feed: export accepted = true
drill then lateral rapid into uncut stock: collisions = 0
pre-cancelled run: steps = 1, errors = false
pre-cancelled run: export accepted = true
```

### A02: arc sweep is missing from travel bounds

Construct a full circular move with radius 20 mm around `(21,0,5)`, starting and ending at `(1,0,5)`. Its XY spans are 40 mm, exceeding a machine's 10 × 10 mm travel. The Fanuc post returns `Ok` because `program_bounds` adds the arc endpoint but not intermediate extrema. This reproduces a defect even in the intentionally limited travel-span check; it does not depend on knowing absolute work-offset placement.

Source: [pinned post bounds/check](https://github.com/pr0m1th3as/OpenCAMStudio/blob/5c510b504fe39f4e40700ff3b1e331d0309fcad3/crates/cam-post/src/lib.rs#L350).

**Required correction:** bound the entire motion with valid arc/plane semantics and apply the travel-span precheck; later use resolved setup-to-machine placement and kinematics for actual axis limits.

### A04: invalid numeric values pass posting

Pass `Linear { to: (NaN,1,1), feed: -10 }` to the Fanuc post. It returns `Ok`. The writer tests upper feed/spindle ceilings, while no central validator rejects nonfinite coordinates or nonpositive feed in the public post path. GUI field restrictions cannot guarantee the safety of all library callers, imported documents or future plugins.

Source: [pinned writer](https://github.com/pr0m1th3as/OpenCAMStudio/blob/5c510b504fe39f4e40700ff3b1e331d0309fcad3/crates/cam-post/src/writer.rs#L70).

**Required correction:** validate input/model/machine/options and full IR independently of the GUI; validate the exact formatted output as well. This probe uses a direct program caller, not a claim that the current GUI lets an operator type NaN.

### A03: a rapid after a drill cycle is not checked

Create stock with top Z0, drill at `(0,0)` to Z−1 and retract to Z2, then issue a rapid to `(8,0,−1)`, in uncut stock. The simulator reports no collision. Its drill branch removes material at hole endpoints and sets `cur = None`, so the next rapid's sweep is never checked. The counterexample is a deliberately invalid program: verification should detect its collision even if existing strategies normally avoid generating it.

Source: [pinned simulator](https://github.com/pr0m1th3as/OpenCAMStudio/blob/5c510b504fe39f4e40700ff3b1e331d0309fcad3/crates/cam-sim/src/lib.rs#L280).

The probe uses the documented default flat cutter to isolate cycle traversal. Separate source inspection establishes that unresolved tool numbers also use that fallback. A future verifier should reject unresolved tools rather than silently substituting geometry.

**Required correction:** simulate complete canonical cycle motions and preserve final pose; check inter-cycle/operation links and reject unknown execution state where a complete verification claim would be required.

### A01: a cancelled run can be exported

Load the repository's DXF fixture through the controller, cancel the token before computation, then call `run` and `export_nc`. The run contains one setup-comment step, reports no error and exports successfully. `build_job` stops on cancellation without returning completion state; the export gate checks diagnostics and selected collisions but has no cancellation/completeness gate.

Sources: [pinned planner](https://github.com/pr0m1th3as/OpenCAMStudio/blob/5c510b504fe39f4e40700ff3b1e331d0309fcad3/crates/cam-toolpath/src/plan.rs#L77), [pinned run/export](https://github.com/pr0m1th3as/OpenCAMStudio/blob/5c510b504fe39f4e40700ff3b1e331d0309fcad3/crates/cam-app/src/controller.rs#L2053).

**Required correction:** propagate completion state, including mid-operation cancellation; forbid partial/empty cancelled results from all export APIs and distinguish valid intentionally empty jobs.

## Reproducing the probe

Run from the repository root at the audited revision. This script builds the default headless libraries and compiles/runs the saved probe. It uses the exact artifact paths reported by Cargo, so cached builds with different feature combinations cannot mix incompatible libraries. It also respects `CARGO_TARGET_DIR`. It creates normal build output and a temporary executable without adding a workspace member, changing dependencies or modifying runtime code.

```sh
python3 - <<'PY'
from pathlib import Path
import json
import subprocess
import tempfile

build = subprocess.run(
    ['cargo', 'build', '--workspace', '--locked', '--message-format=json'],
    stdout=subprocess.PIPE, text=True, check=True,
)
libraries = {}
for line in build.stdout.splitlines():
    record = json.loads(line)
    if record.get('reason') == 'compiler-artifact':
        for filename in record['filenames']:
            if filename.endswith('.rlib'):
                libraries[record['target']['name']] = filename
dependencies = Path(libraries['cam_app']).parent / 'deps'
with tempfile.TemporaryDirectory(prefix='opencam-review-') as scratch:
    binary = Path(scratch) / 'probe'
    command = [
        'rustc', '--edition=2021', 'docs/architecture-validation-probe.rs',
        '-L', 'dependency=' + str(dependencies), '-o', str(binary),
    ]
    for name in ['cam_cldata', 'cam_model', 'cam_post', 'cam_sim',
                 'cam_app', 'cam_toolpath']:
        command.extend(['--extern', name + '=' + libraries[name]])
    subprocess.run(command, check=True)
    subprocess.run([str(binary)], check=True)
PY
```

For a modified revision, printouts may change as fixes land. Convert these cases into appropriately located regression tests during implementation; they are documented probes here, not permanent assertions that defects should remain.

## Source-only observations and confidence limits

| Observation | Evidence | Confidence / limitation |
|---|---|---|
| Nine implemented crates; no kernel/plugin API crates | Workspace manifests and file inventory | High at audited revision |
| Synchronous GUI recomputation | `gui.rs:4713`, direct `controller.run(&CancelToken::new())` | High; no live responsiveness measurement |
| Single setup and translation-only additional origins | `document.rs`, `Document`, `Setup`, `Origin` | High; no rotational setup transform exists in these types |
| Simulation lacks holders, fixtures and machine-body collision inputs | `SimTool`, `simulate`, `CollisionKind` | High; absence is visible in public model/API and traversed implementation |
| Target gouge API is not used by application pipeline | `check_gouge` vs `controller::simulate_stock` / export gate | High; target comparison exists as a library API/test capability |
| Grbl M6 mismatch | `grbl.rs` capability and shared emitter; official Grbl supported commands | High for stock Grbl; descendants/senders require distinct profiles |
| Shared post TLO policy is incomplete | Preamble G49; no explicit G43 in shared change branch; Okuma emits G56 H | High in source; exact machine behavior depends on configured macros/options |
| Same-datum/internal tool transitions need policy consolidation | Planner change branch and Carve insertion | High for distributed state; no physical collision claimed without machine geometry |
| Position is deliberately retained across tool changes | `dialect.rs` M6 branch and `tests/toolchange.rs` | High in source; safety depends on a macro return-pose guarantee not represented in the profile |
| Simulator ignores controller radius compensation events | `simulate` match handles moves/cycles and ignores other events | High in source; physical cutter-center trajectory requires resolved offset semantics |
| Project machine/post are provenance, not adopted | `controller::open_project` and `machines.rs` | High; some field comments contradict current behavior |
| Project writes are non-atomic; exclusions reset on open | `save_project`, `config::save_to`, `open_project` | High in source; no crash/data-loss trial performed |
| Current architecture could evolve to qualified industrial milling | Existing boundaries plus proposed contracts | Engineering judgment; depends on implementation and qualification |

External controller sources were checked during the review: [Grbl command reference](https://github.com/gnea/grbl/blob/master/doc/markdown/commands.md?plain=1), [grblHAL M6/reference](https://github.com/grblHAL/grblhal_docs/blob/main/content/markdown/04-Reference/02-complete-gcode-and-mcode-reference.md), and [LinuxCNC compensation](https://linuxcnc.org/docs/html/gcode/tool-compensation.html). Their current documentation is supporting context, not a substitute for version-specific controller/machine qualification.
