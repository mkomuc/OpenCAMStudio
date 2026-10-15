# M1 — Domain services and asynchronous machining jobs

Implemented **2026-10-14** against the parent M1 acceptance gate in the
[implementation plan](implementation-plan.md#m1--domain-contracts-services-and-asynchronous-jobs).
M1a (Machining/Tools workspace) and M1b (operation tabs) remain separate proposed
workflow gates. This package does not complete the remaining M0 export
qualification or add fixture, holder, machine-body or volume-stock verification.

## Delivered behavior

The GUI submits an immutable machining request and immediately returns to its
message loop. Geometry resolution, strategy generation, existing heightfield
stock verification, backplot preparation and stock-mesh preparation run in a
background worker. The Output view reports progress and offers Cancel; Home's
Run command becomes Cancel while a request is pending. The synchronous
`AppController::run` API remains available to CLI callers and headless tests,
using the same computation services.

Every request carries a unique request ID, monotonic session revision and input
fingerprint. Edits and undo/redo increase the revision and cancel superseded work.
Publication requires the entire request stamp and current revision to match.
Starting a run clears the previous preview/export; a late, cancelled or failed
worker cannot restore it or enable export. Successful background results retain
their stamp. Worker panics, worker disconnection and thread-start failures produce
failed results with diagnostic codes, rather than a completed job.

The scheduler permits one active worker and one pending request. Repeated Run
requests replace the pending request, preventing an accumulation of worker
threads and input snapshots. Progress stores only the newest value. Cancellation
is cooperative: geometry resolution checks operation boundaries, strategies keep
their cancellation contracts, stock traversal checks motions and cell rows, and
preview preparation checks motions and mesh rows. Completed planning fragments
remain available as a non-exportable cancellation preview for synchronous users;
explicit GUI cancellation discards the request and its preview.

## Domain and service contracts

Schema **16** adds persistent excluded-operation IDs, setup-scoped disabled
work datums and allocation marks for setup, operation, asset and instance IDs.
Suppression is manufacturing state: it saves, reopens, undoes and redoes. It is
independent of display visibility. Removing a suppressed operation/setup clears
its assignments in the same undo transaction; datum renaming carries suppression
with the datum. Switching physical setups preserves each setup's suppression.
The headless planner also honors these fields directly.

Allocation marks prevent deletion and an abandoned undo branch from recycling
identities. Saving after undo preserves the session's marks before serialization.
Older projects migrate through the existing versioned loader with empty
suppression and default marks; their existing IDs and operation order remain.
Document revisions are session publication guards, independent of undo snapshots:
an undo restores manufacturing inputs while the revision continues increasing.

`DiagnosticCode` provides programmatic categories. `SourceRef` carries setup,
operation and optional segment ownership; `Diagnostic.message` remains display
text. The legacy `op` field stays synchronized for the existing tree markers.
Geometry resolution preserves operation ownership instead of encoding it only in
an error string, and invalid neutral motion includes its segment index.

Controller command/history, geometric selection/snapping, export and job
submission/publication services now live in separate headless modules. The GUI
inspector, shared operation-field inventory and job/output presentation have their
own modules. Typed first-party operation variants and the existing strategies,
posts, CAD qualification restrictions and export completion gates remain in use.

## Cache and invalidation decision

Cache operation fragments, rather than finished jobs. A key compares the exact
operation definition, effective heights, referenced tool definitions, stock bounds
and spindle direction. CAD bindings are resolved and qualified before cache
lookup; translated/resolved geometry and effective cutting heights reach the key.
Tool definitions include every tool used by a multi-tool operation. Failed or
cancelled computations never populate the cache.

| Changed input | Required recomputation |
| --- | --- |
| One operation's parameters/geometry | That operation's fragment; full assembly, simulation and preview |
| Referenced cutter geometry | Fragments using that tool; assembly, simulation and preview |
| Unreferenced cutter | Matching fragments remain reusable |
| Heights/stock | Fragments whose dependency key changed; assembly, simulation and preview |
| Operation ordering or suppression | Reassemble actual execution order; remaining matching fragments are reusable |
| CAD placement/setup frame | Re-resolve geometry, compare fragment inputs, rebuild simulation and transformed preview |
| Machine assignment | Revalidate/regenerate the job; rebuild transitions including machine-derived tool-change Z |
| Post selection | Cancel in-flight requests and discard NC; completed neutral results can be reposted |
| Undo/redo | Cancel old requests, restore manufacturing inputs, reuse only exact matching fragments |

Tool changes, spindle state, work-datum transitions, shutdown and planning order
are rebuilt on every run. No stock snapshot or predecessor-dependent path is
cached; future rest-stock planning needs explicit upstream result dependencies.
This avoids reusing a fragment as though it included a transition from a different
predecessor. Exact typed equality is the cache correctness guard; the request
fingerprint is a deterministic non-cryptographic provenance hash used together
with unique request IDs and revisions, rather than a cryptographic guarantee.

Cache retention is limited to 64 entries and 500,000 retained program steps.
Worker cache snapshots share both dependency keys and results through `Arc`.
History retains 64 undo checkpoints by default, with a configurable constructor
and a gesture-coalescing API; existing transactional GUI drafts still commit once.
The measurement regression performs 1,000 edits over a shared 4 MiB payload and
retains 64 checkpoints with 66 references to the same allocation (owner, present,
and checkpoints). Existing geometry tests confirm that imported source and
mesh/topology snapshots share allocations across copies. This measures sharing
and retention, not total application RSS; full snapshot history is retained until
real project measurements justify a command-log replacement.

## Acceptance evidence

Validated locally with **Rust/Cargo 1.99.0**, default geometry backend and the GUI
feature enabled separately:

- `cargo test --workspace`: **778 passed, 23 ignored**, no failures.
- `cargo test -p cam-app --features gui`: **227 passed**, including 13 CAD and
  three multi-setup integration tests; no failures.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo clippy -p cam-app --features gui --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

Deterministic worker latches prove that controller selection and GUI cursor input
continue while a worker is deliberately blocked. Edits and explicit cancellation
reach its token; releasing the old worker cannot publish stale geometry or enable
export. A replacement job publishes only its new stamp, and 100 additional Run
requests coalesce to the newest request. Separate tests reject mismatched request
IDs, revisions and fingerprints, preserve structured operation ownership, and
convert injected worker panics into failed, non-exportable results.

Cache regressions check a changed operation alongside an unchanged one, undo/redo
reuse, actual planning-order changes, tools, heights, stock and machine-derived
transitions. Persistence tests cover suppression across setup switching and
reopening, save after undo, and importing a new model after deletion without
reusing the old asset/instance identity. Simulation and rendering regressions
cancel inside a long cut, mesh rows and motion preparation. Existing CAD,
controller, strategy, simulation and golden-output suites remain passing.

The GUI tests exercise the message/update loop without a graphics adapter. Native
OCCT fixtures remain outside this run; those features were not expanded here.
Existing dormant legacy view controls and the 2D preview are retained with
narrowly scoped dead-code annotations while their workspace routing awaits M1a;
new service/job code is checked with warnings denied.
