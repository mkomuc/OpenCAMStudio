# Documentation

## Architecture review and industrial CAM plan

The 2026-10-13 review is pinned to source revision `5c510b504fe39f4e40700ff3b1e331d0309fcad3`.

- [Detailed architecture analysis](architecture-analysis.md): actual module boundaries, strengths, gaps, requested feature readiness and prioritized findings.
- [Implementation plan](implementation-plan.md): proposed contracts, dependencies, milestones, acceptance gates and initial backlog.
- [Validation evidence](architecture-validation.md): headless test results, source observations and reproducible public-API counterexamples.
- [Standalone validation probe](architecture-validation-probe.rs): small Rust program demonstrating four export/verification gaps at the audited revision.

## Existing project references

- [CAD import and feature selection plan](cad-import-feature-selection.md): STEP/IGES import, selectable topology, linked operation geometry, backend evaluation and staged acceptance gates.
- [Adaptive-clearing baseline fix](adaptive-clearing-fix.md): root cause, conservative restart change, measurements and regression validation on the fork.
- [Motion validation and arc bounds](motion-validation-and-arc-bounds.md): shared IR/machine validation, rounded NC checks, analytic arc bounds and remaining verification scope.
- [Completion-aware job export](job-completion-export.md): explicit job states, cancelled/incomplete export gates, API migration and regression coverage.

- [Root architecture reference](../ARCHITECTURE.md)
- [Root roadmap](../ROADMAP.md)
- [Running and building](../RUNNING.md)
- [v0.1.0 release notes](release-notes/v0.1.0.md)

- [CAD import implementation](cad-import-implementation.md) — available fixed-axis STEP workflow, native build and qualification limits.
- [Multi-setup workflow](multi-setup-implementation.md) — independent machining sides, edge/model WCS selection, box stock, transactional right-column editors and separate NC output.
