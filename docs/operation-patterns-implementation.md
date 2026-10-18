# Operation patterns implementation evidence

The implementation follows the proposal in [operation-patterns-plan.md](operation-patterns-plan.md) for rectangular and circular setup-level patterns. The proposal remains the broader acceptance specification; this note records implemented behavior and current qualification limits.

## Delivered behavior

- Schema 23 stores setup-local pattern definitions and a document-wide pattern ID floor. The v22 migration initializes empty pattern lists and preserves or initializes the floor.
- Pattern membership is validated against the canonical ordered operation list, including same-datum, unique-ID, non-overlap, contiguity, and reference checks. `execution_items()` exposes grouped and ordinary operations to the planner.
- Controller commands create and edit a group atomically, compact noncontiguous selected sources at the earliest selected position, dissolve or delete a group, and keep source deletion, suppression, duplication, and reorder behavior group-aware. Undo does not reuse allocated pattern IDs.
- The GUI provides setup and operation context-menu entry points, preserves selected operations on a selected-row context click, renders expandable groups with editable children, and supports transactional create/edit/apply/cancel. Invalid values, mixed datums, and already-grouped sources block Apply with a reason. Circular center coordinates are numeric fields defaulted from the active datum.
- Generation computes each included source fragment once, expands it in By instance or By operation order, applies setup-XY rigid transforms before job assembly, rebuilds transitions, and attaches pattern/instance provenance to neutral-motion tags. Pattern runs preserve the complete setup graph and apply exclusions in the immutable worker snapshot. Suppressed groups and excluded sources are skipped before CAD resolution and seed generation.
- Generated backplot strips, rapid-stock collision records, and motion-validation diagnostics retain the source operation and pattern-instance identity where the originating motion carries it.

## Current limits

The draft viewport overlays transformed paths from a current generated result when available; otherwise it shows setup-frame placement crosses as markers. It does not synchronously regenerate seed paths on each draft edit, and the center is numeric-only rather than viewport-picked. Native desktop picking/visual smoke tests and OCCT-backed STEP/IGES import qualification have not been performed in this environment. The pattern regression set covers drill cycles and typed CL transforms, but does not qualify every operation family, every controller post, fixture/holder collisions, stock-wide Face footprints, or arbitrary multi-tool Carve patterns. The 10,000-instance and 5,000,000-motion-step caps are enforced but are not performance-qualified.

## Verification

Verified with Rust/Cargo 1.99.1 and cached dependencies using Cargo offline. The GUI feature checks are headless; they do not establish visual desktop behavior.

- `cargo test --workspace --offline --no-fail-fast`: passed (903 passed, 25 ignored, no failures).
- `cargo test -p cam-app --offline`: passed, including controller, project persistence, CAD, setup, job-scope, and pattern-ID exhaustion coverage.
- `cargo test -p cam-app --features gui --offline`: passed (346 unit tests passed, 4 ignored; all enabled CLI, CAD, setup, entry-feed, startup, and tool-preset integration tests passed).
- `cargo test -p cam-toolpath --offline cancellation_during_the_last_operation_retains_only_completed_fragments`: passed; a canceled preview retains completed ordinary fragments and does not expand pattern instances.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo clippy -p cam-app --features gui --offline -- -D warnings`: passed.
- `git diff --check`: passed.

The workspace suite includes the existing toolpath, post, simulation, geometry, import and project regression tests. Native OCCT and visual desktop qualification remain outside these headless checks.
