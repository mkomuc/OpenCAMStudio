# Adaptive clearing: restore the headless baseline

Date: **2026-10-13**. Working repository: [mkomuc/OpenCAMStudio](https://github.com/mkomuc/OpenCAMStudio). Base revision: `5c510b504fe39f4e40700ff3b1e331d0309fcad3`. Branch: `codex/fix-adaptive-clearing`.

The local checkout uses the fork as `origin` and the original `pr0m1th3as/OpenCAMStudio` repository as `upstream`. The architecture review documents remain historical assessments of the base revision, including its four failing tests.

## Problem and cause

Four `clearing` tests expected a certified steered path around a 20 × 20 mm island in a 60 × 60 mm pocket, using a 6 mm cutter and 2 mm nominal engagement. The generator produced a path that exceeded the existing certification bound, so dispatch correctly fell back to concentric rings. That fallback also explained the re-entry feed and helix expectation failures.

The source of excessive engagement was in `steer_path`: `decide_step` marked a proposal as `buried` when no feasible turn held the target. The main loop counted that condition but still committed the overloaded move. Seed lookahead only validated the first 25 steps of a restarted front; a longer front could become overloaded later.

An independent replay of the original path measured about **5.01 mm** peak engagement, against an unchanged **3.50 mm** bound. Polygon and grid diagnostic readings agreed on the failure, so weakening the gate or changing assertions would conceal the problem.

## Change

Retire an overloaded front before committing its next move. Reuse the existing search for a cleared re-entry point, including its feasibility and lookahead checks. Coverage, gouge and engagement certification remain the final authority; uncertifiable paths still fall back to concentric clearing.

The existing diagnostic counter now records rejected overloaded proposals rather than committed overloaded moves, and its table label documents that meaning. Temporary diagnostic logging used during investigation was removed.

For the affected pocket on the review host:

| Measurement | Original generator | Updated generator |
|---|---:|---:|
| Peak engagement | 5.0074 mm | 3.1047 mm |
| Engagement bound | 3.50 mm | 3.50 mm |
| Reachable stock left uncut | 13.48 mm² | 13.70 mm² |
| Coverage tolerance | 64.85 mm² | 64.85 mm² |
| Gouge area | 0 mm² | 0 mm² |
| Generated moves | 2,968 | 3,001 |
| Dispatch | Concentric fallback | Certified steered path |

These figures come from temporary instrumentation of the existing oracle on macOS/Apple silicon with Rust 1.99.0. They describe this regression fixture, not a universal performance or machine-safety guarantee.

## Regression checks

- The four original failures pass without changing their assertions or certification limits.
- `an_overloaded_front_restarts_without_losing_coverage` checks independently replayed engagement, coverage and gouge for the affected shape.
- That new regression was also run with the original restart condition temporarily restored. It failed specifically on `5.0074 > 3.50`, demonstrating that it detects the original behavior. The fix was restored afterward.
- `a_tool_width_channel_still_requires_fallback` checks that a channel exactly as wide as the cutter cannot be accepted as a certified adaptive path.
- Additional review ran the seven-shape measurement harness: six shapes certified, and the offset island with a full-width channel remained rejected for incomplete coverage.
- The affected 60 × 60 mm pocket's emitted two-level program was also simulated with straight and helical entry, each at 0.5 mm and 0.1 mm stock resolution. All four runs reported zero rapid-through-stock collisions.

Completed verification:

```sh
cargo test --workspace --locked --offline --no-fail-fast
# 677 passed, 0 failed, 23 ignored, including two passing doctests; exit 0.

cargo clippy --workspace --all-targets --locked --offline -- -D warnings
# Passed; exit 0.

git diff --check
# Passed.
```

GUI execution and physical-controller qualification were not part of this focused headless change. No golden NC fixtures or tolerance constants were changed.

## Next work

Continue M0 with completion-state propagation and rejection of cancelled/incomplete job exports, followed by central IR validation, full arc travel bounds and canonical drill-cycle verification. Those architecture findings are separate from this clearing fix and remain open.
