# Motion validation and swept arc travel bounds

Implemented on `codex/validate-motion-and-arc-bounds`, based on the completion/export package `f7eebe0`. Date: **2026-10-13**. This M0 package corrects the A02 travel-span defect and the malformed-motion portion of A04 in the [architecture assessment](architecture-analysis.md).

## Result

All seven built-in posts now reject malformed motion and machine limits before emitting NC. The 40 mm full-circle probe is rejected by a machine with 10 mm travel, even though the arc's start and end coincide. NaN positions and nonpositive feeds are rejected. Valid existing golden NC fixtures remain unchanged.

Planning also validates its assembled program. Invalid motion makes the job `Failed` and adds an operation-tagged diagnostic. The application skips toolpath rendering for invalid IR and skips stock simulation for failed jobs. Invalid machine definitions prevent planning. Completion and file-export gates from the preceding package still apply.

## Ownership and public contracts

- `cam-cldata::Program::validate()` is controller-neutral and dependency-free. It returns optional `MotionBounds`, or the first `ValidationError` with a zero-based step index and optional operation ID. Display messages use one-based step numbers.
- `cam-model::Machine::validate()` checks physical numeric definitions independently of the post implementation.
- Both post emission families call the same preflight, covering `PostKind` and direct `GrblPost`, `FanucPost` and `OkumaPost` calls.

The IR checks finite XYZ coordinates, positive finite feeds and spindle speeds, nonnegative finite dwell, valid nonzero arc radii, radius agreement, datum indices starting at one, and drill-cycle numeric semantics. Drilling requires `retract >= top > depth`, finite hole coordinates, positive peck increments that can advance at the supplied Z values, and legal dwell/feed. A completed drilling cycle leaves its last hole at retract as the next arc's anchor. Empty cycles preserve the prior anchor. Datum events invalidate the anchor until an explicit position is established in that datum.

Machine checks require finite positive rapid/feed/spindle limits, finite envelope endpoints with finite positive travel on each axis, finite safe Z, and finite optional tool-change coordinates. They do not infer machine placement from work coordinates.

Arc radius agreement uses an absolute **0.005 mm** tolerance. Swept extrema use the larger admitted radius. This is a conservative consistency threshold, informed by the arc-radius check in [Grbl's parser](https://raw.githubusercontent.com/gnea/grbl/master/grbl/gcode.c); it does not qualify output for every controller or installation.

Public post errors now include `InvalidProgram`, `InvalidMachine`, `InvalidPrecision`, `InvalidOutput` and `UnsafeRounding`. Existing upper-rate, travel-span, missing-arc-anchor and unsupported-datum refusal paths remain compatible. Saved-project schema stays at version 13; no persistence change is needed. Callers exhaustively matching `PostError` must account for the new variants.

## Bounds and formatted output

Bounds include endpoints and each XY quadrant extremum within the commanded clockwise/counterclockwise sweep. Equal angular endpoints represent a full circle; helix Z bounds come from its endpoints. Partial arcs do not acquire the extrema of the unswept portion of their circle. The Z component of the centre does not define a separate arc plane: the existing IR supports XY arcs with independent helical Z.

Preflight checks both the original program and a numeric reconstruction using the exact NC word formatters. Reconstructed arc centres use the rounded start position plus rounded incremental I/J, matching the writer. Checks catch zero feed/RPM/peck after formatting, collapsed drill depth, inconsistent rounded arc radii, partial-to-full circle collapse, large changes of arc sweep, and rounded travel or rates exceeding machine limits. Coordinate precision is bounded to **0–9 decimal places**, preventing unreasonable formatting allocations.

Explicit drilling expansion is capped at **100,000 pecks per cycle** and guarded against nonadvancing arithmetic. Excessive expansion returns an actionable error; canned-cycle posts can still emit a compact cycle with a small valid Q value. This is an output resource limit, not a controller's machining limit.

Travel checks still compare work-coordinate **spans** with available axis travel. Negative work coordinates that fit by size remain allowed. Multiple datum groups retain the existing combined span precheck. Absolute axis limits require resolved setup-to-machine transforms and initial machine state, which do not exist yet.

## Verification

Nineteen new tests cover numeric input/machine failures, diagnostic provenance, circle/helix bounds, directional partial arcs, datum/arc anchoring, drilling parameters and final position, direct and selectable post entry points, output rounding, and bounded peck expansion. The bounds test independently samples 50 directed arcs (including zero-angle crossings and full circles) at 2,001 positions each.

Validation on the review host:

```sh
cargo test --workspace --locked
# 707 passed, 0 failed, 23 ignored, including two doctests.

cargo test -p cam-app --features gui --locked --offline
# 184 passed, 0 failed; includes shared controller tests.

cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo clippy -p cam-app --features gui --all-targets --locked --offline -- -D warnings
# Passed. Existing dependency block 0.1.6 has a Cargo future-compatibility warning.
```

The [standalone probe](architecture-validation-probe.rs) now reports:

```text
full circle X span 40 vs travel 10: export accepted = false
NaN position and negative feed: export accepted = false
drill then lateral rapid into uncut stock: collisions = 0
pre-cancelled run: steps = 1, errors = false
pre-cancelled run: export accepted = false
```

## Remaining work and next package

The next M0 package should introduce shared cycle traversal/expansion and fix drill simulation (A03): verify approach, pecks, retracts, links and the rapid after a drill cycle while retaining current position. Then address Grbl M6 capabilities and explicit tool-change/TLO policy.

This validator does not establish job completion by itself, resolve tool/register identities against a tool library, validate every strategy/CAD/document input, or interpret final NC modal state/macros. It does not verify holder, fixture or machine-body collisions, tool-change macro trajectories, or controller compensation. Tool changes retain the existing anchor assumption until installation policy defines their physical return pose. Formatted numeric reconstruction is a useful preflight; the planned independent NC interpreter and machine verification remain necessary.
