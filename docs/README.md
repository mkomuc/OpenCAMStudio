# Documentation

## Architecture review and industrial CAM plan

The 2026-10-13 review is pinned to source revision `5c510b504fe39f4e40700ff3b1e331d0309fcad3`.

- [Detailed architecture analysis](architecture-analysis.md): actual module boundaries, strengths, gaps, requested feature readiness and prioritized findings.
- [Implementation plan](implementation-plan.md): proposed contracts, dependencies, milestones, acceptance gates and initial backlog.
- [High-level requirements analysis](high-level-requirements-analysis.md): 2026-10-14 requirement coverage, current backend operation parameters, scope limits and revised workflow gates.
- [High-level functions](high-level-functions.md): source requirements for Machining, setups, tools and operation inspectors.
- [Tool-library implementation reference](tool-library-implementation-reference.md): recorded Fusion behavior and proposed library design; the current plan limits cutter types to the eight already supported.
- [Validation evidence](architecture-validation.md): headless test results, source observations and reproducible public-API counterexamples.
- [Standalone validation probe](architecture-validation-probe.rs): small Rust program demonstrating four export/verification gaps at the audited revision.

## Existing project references

- [Setup operation patterns](operation-patterns-implementation.md): rectangular and circular patterns for one or multiple operations, context-menu creation, transactional editing, toolpath reuse and qualification limits. The [feature plan](operation-patterns-plan.md) records the broader acceptance specification.
- [CAD import and feature selection plan](cad-import-feature-selection.md): STEP/IGES import, selectable topology, linked operation geometry, backend evaluation and staged acceptance gates.
- [Adaptive-clearing baseline fix](adaptive-clearing-fix.md): root cause, conservative restart change, measurements and regression validation on the fork.
- [Motion validation and arc bounds](motion-validation-and-arc-bounds.md): shared IR/machine validation, rounded NC checks, analytic arc bounds and remaining verification scope.
- [M1 domain services and asynchronous jobs](m1-domain-services.md): revision-stamped workers, cancellation/progress, per-operation cache, schema-16 suppression and identity marks, bounded history and acceptance evidence.
- [M1a/M1b workspaces and operation tabs](m1-workspaces-operation-tabs.md): Machining/Tools surfaces, overlay input isolation, stable-ID tree commands, transactional operation tabs and shared geometry workflows; automated evidence and visual acceptance limits.
- [Completion-aware job export](job-completion-export.md): explicit job states, cancelled/incomplete export gates, API migration and regression coverage.

- [Root architecture reference](../ARCHITECTURE.md)
- [Root roadmap](../ROADMAP.md)
- [Running and building](../RUNNING.md)
- [v0.1.0 release notes](release-notes/v0.1.0.md)

- [CAD import implementation](cad-import-implementation.md) — available fixed-axis STEP workflow, native build and qualification limits.
- [Multi-setup workflow](multi-setup-implementation.md) — independent machining sides, edge/model WCS selection, box stock, transactional right-column editors and separate NC output.
