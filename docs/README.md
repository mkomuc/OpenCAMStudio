# Documentation

## Architecture review and industrial CAM plan

The 2026-10-13 review is pinned to source revision `5c510b504fe39f4e40700ff3b1e331d0309fcad3`.

- [Detailed architecture analysis](architecture-analysis.md): actual module boundaries, strengths, gaps, requested feature readiness and prioritized findings.
- [Implementation plan](implementation-plan.md): proposed contracts, dependencies, milestones, acceptance gates and initial backlog.
- [Validation evidence](architecture-validation.md): headless test results, source observations and reproducible public-API counterexamples.
- [Standalone validation probe](architecture-validation-probe.rs): small Rust program demonstrating four export/verification gaps at the audited revision.

## Existing project references

- [Adaptive-clearing baseline fix](adaptive-clearing-fix.md): root cause, conservative restart change, measurements and regression validation on the fork.

- [Root architecture reference](../ARCHITECTURE.md)
- [Root roadmap](../ROADMAP.md)
- [Running and building](../RUNNING.md)
- [v0.1.0 release notes](release-notes/v0.1.0.md)
