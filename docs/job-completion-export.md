# M0 package: completion-aware job export

Date: **2026-10-13**. Base revision: `87792b7f487b2d73dcc5dfae69226b893a0d8362`. Working branch: `codex/block-incomplete-job-export` in [mkomuc/OpenCAMStudio](https://github.com/mkomuc/OpenCAMStudio).

Addresses architecture finding **A01**: a cancelled run previously returned a setup-comment-only program without error diagnostics and could be exported successfully.

## Behavior

The planner now returns a `JobResult` containing the program, diagnostics and a `JobCompletion` state:

| State | Meaning | Application export |
|---|---|---|
| `Incomplete` | Completion has not been established; the default state | Rejected |
| `Complete` | All requested operations finished without error diagnostics | Eligible for existing diagnostic, collision and post checks |
| `Cancelled` | The token was cancelled or a strategy reported cancellation | Rejected |
| `Failed` | An operation reported an error | Rejected |

Cancellation is observed before each operation, after each strategy returns, and after program assembly. This includes cancellation in the last operation and a strategy reporting cancellation independently of the shared token. Cancelled/failed operation fragments are discarded. Previously completed operations may remain available for inspection, but cannot be exported as a complete job. Errors retain operation ownership and do not prevent collection of diagnostics from subsequent operations unless the job is cancelled.

An intentionally empty job can complete successfully; program size is never used to infer completion. A cancellation request after the run has already completed does not retroactively alter that immutable result.

The controller clears the previous result and NC cache when starting a new run. It carries planner completion through scene/stock preparation, skips stock simulation for already cancelled/failed jobs and checks cancellation again before storing the outcome. The GUI reports cancelled/incomplete/failed runs instead of claiming that their toolpath is ready.

Both `export_nc` and `export_nc_to` use the same completion gate. Blocked file export does not create a file or overwrite an existing program. The existing error-diagnostic, rapid-through-stock and post validation gates still apply to completed jobs.

## API migration

`cam_toolpath::build_job` now returns `JobResult` instead of `(Program, Vec<Diagnostic>)`. Callers must check completion before handing a planned program to a post:

```rust,ignore
let job = build_job(&document, rpm, direction, stock, change_height, &cancel);
if job.completion != JobCompletion::Complete {
    // Report cancellation/failure and retain diagnostics; do not post the program.
    return;
}
let program = job.program;
let diagnostics = job.diagnostics;
```

`JobCompletion` is exported by both `cam-toolpath` and `cam-app`; `RunOutcome` now carries `completion`. Default results are incomplete. `JobResult` is marked `must_use`. Existing in-repository callers were migrated to the new result type.

This is a source API change for library callers, not a saved-project format change; schema version remains 13. Low-level `cam-post` still formats arbitrary controller-neutral programs and does not establish job completion itself. Callers bypassing the application export API must honor the planner's completion contract.

## Regression coverage

Six deterministic planner tests cover pre-cancellation, strategy-only cancellation, cancellation during the final operation, omission of errored fragments with continued diagnostics, warning-only empty operations, and completed versus cancelled empty documents. Mid-operation cases use an internal computation seam, avoiding timing-dependent thread sleeps.

Five controller tests cover replacement of previous cached NC, fresh-run recovery, blocked creation/overwrite of NC files, non-complete previews without error diagnostics, empty jobs, and cancellation after an already completed run. Existing error-message and failed-job tests also check the new behavior.

The standalone architecture probe now reports `pre-cancelled run: export accepted = false`; its other three counterexamples remain separate M0 work.

Completed validation on the review host:

```sh
cargo test --workspace --locked --offline --no-fail-fast
# 688 passed, 0 failed, 23 ignored, including two passing doctests.

cargo test -p cam-app --features gui --locked --offline
# 183 passed, 0 failed; includes the controller tests as well as GUI-feature tests.

cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo clippy -p cam-app --features gui --all-targets --locked --offline -- -D warnings
# Both passed.
```

The documented standalone reproduction, local documentation links and
`git diff --check` also passed. Existing golden NC fixtures were not changed.

## Limits and next package

This package prevents incomplete-job export. It does not introduce background jobs or fully cooperative cancellation inside stock simulation and mesh preparation; those routines currently finish their work before the controller's final cancellation check. General motion validation, full swept-arc travel bounds and drill-cycle verification are the next M0 packages. Simulation coverage remains the current heightfield/cutter model.

The subsequent [motion-validation and arc-bounds package](motion-validation-and-arc-bounds.md) now addresses general motion preflight and swept travel bounds. Drill-cycle verification remains next.
