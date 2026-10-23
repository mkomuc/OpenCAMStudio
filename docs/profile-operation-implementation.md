# Profile operation implementation status

This document records the implemented fixed-axis increment from the [profile review and backlog](profile-operation-review-plan.md). It does **not** mark the entire 20-item backlog complete. The remaining capabilities below need further implementation and qualification; no controls were added for them.

## Available behavior

- One controller-compensation method, independent of climb/conventional direction. Old `ControlLeft` and `ControlRight` records load through aliases without changing the saved direction. The effective G41/G42 side follows actual travel and spindle direction. Control mode requires an Inside/Outside contour, straight descent, and linear engagement/disengagement at least one tool radius long; uncompensated approach/departure points clear the wall by the full cutter radius and are checked against the available air region. Compensation brackets the lead moves; supported posts place G41/G42 on the linear XY engagement block and reject vertical or rounded-away engagement. Unsupported posts remain unavailable.
- Bounded, validated depth and entry generation. Nonfinite parameters, non-progressing stepdowns, excessive levels/ramps and excessive motion expansion fail with diagnostics. Cancellation discards partial profile motion. Generated profile IR is validated before returning it. Clearing also checks ring/entry budgets and cancellation.
- Explicit roughing failure when stock is unavailable or the chosen boundary cannot contain the allowanced part. Negative wall allowance now reaches the same wall in roughing and single-pass profiling. Setup stock and saved drawing-outline snapshots use the existing clearing engine.
- Configurable radial finishing count and spacing, finish feed, spring repeats and finish-only-at-final-depth. Wall allowance remains `offset`; floor allowance is a separate signed value. Additional finishing depths, even stepdowns and contour/depth/radial scheduling share one scheduler. Smaller bottom increments can apply to every stage (Rough final), or only to the final wall and its spring passes while intermediate stages use regular maximum stepdowns. Preserved selection order can be replaced by a deterministic nearest-contour heuristic.
- Open chains support tangent start/end extensions, independent linear/arc leads, contour ramps, helix/zigzag descent and alternating depth-pass direction. A contour ramp retraces its entry at depth to clear the entry wedge. An insufficiently long chain fails instead of increasing the requested angle. Open paths remain open; intersecting offsets are rejected.
- One Arc editor supports sweeps greater than zero and at most 170 degrees. Legacy 90-degree arcs retain their saved shape until edited; there are no separate fixed/custom arc modes in the inspector. Exit shape can follow entry shape while retaining an independent exit feed. Lead end distance starts that feed before the contour ends; splitting occurs before fitting so arcs cannot erase the feed boundary. This feed transition currently excludes tabs.
- Optional ramp RPM surrounds ramp/helix/zigzag entry and restores the explicit cutting RPM before subsequent motion. Both commands are present in the shared IR, so post limits and the next operation see the same state. Zero retains the original cutting RPM.
- Holding tabs support rectangles/triangles, exact count, distance spacing or manual placement. Preview and motion use the same compensation and validated placement/layout calculations. Triangular motion includes cutter-diameter protection and piecewise-linear shoulders; preview uses a triangular prism.
- Round or sharp outer-corner offsets, configurable finishing arc-fit tolerance and optional fed retracts. Shared radial clearing keeps its established round corners and default fitting tolerance; independent finish settings require an explicit finishing stage.
- Stepped taper for unlinked computed profiles with a material side, using the nominal bottom as reference. Only zero/negative angles are supported: higher cuts remove more material. Positive angles would require undercutting and are rejected. Tabs and linked CAD remain unavailable with taper.
- Duplicate resolved boundaries are reported rather than silently discarded or machined twice. Named entry collection and the public strategy both validate selection identity.

Schema 26 adds defaulted pass settings and tab shape/count fields. Existing `Lead::Arc` records keep their 90-degree meaning; `ArcSweep` is an additional representation for a different sweep. Existing files keep their original defaults. Old requests that relied on silently ignored/unsafe options now need explicit correction.

## Inspector and explanatory graphics

Passes contains finishing, depth, ordering, corner and fitting controls. Linking contains open extensions, custom lead sweeps, linked exit shape, lead-end feed distance and fed retracts. Tool contains the optional ramp RPM. Geometry contains tab shape and distance/count/manual placement. Applicable source/post restrictions and cross-field errors are shown before applying the draft. Optional finishing values survive collapse/enable cycles; Apply/Cancel and undo retain one operation transaction.

The shared help renderer serves ordinary inspector hints, geometry hints and a persistent **Profile help** action. It reuses the existing Climb illustration and adds original SVG diagrams for cutter side, wall/floor allowance, depth/stepover, tabs, leads/overlap, downward entry and height planes. Text remains available alongside each illustration. Hover help retains the 600 ms delay and respects the tooltip setting; the persistent button provides an alternative to hovering.

The source [GUI style specification](gui-style-spec.md) remains the style contract. Graphics are bundled with the application and do not require network access.

## Backlog disposition

“Implemented” describes the software behavior tested in this increment, not a machine trial. “Partial” lists concrete remaining work from the original acceptance scope.

| Backlog | Status | Remaining scope |
| --- | --- | --- |
| P01 — Canonical names/calculations | Implemented | One perimeter helper, one compensation mode, shared compensated loops and tab layouts; no parallel Profile/Contour operation. |
| P02 — Bounded generation | Implemented for Profile and shared clearing | Further strategies should continue to adopt boundary validation; this does not certify every non-profile entry algorithm. |
| P03 — Controller compensation | Partial | Engagement, lead fitting, side ownership and post availability repaired. Actual controller-offset cutter-envelope simulation and machine-specific qualification remain; the existing backplot follows programmed coordinates. |
| P04 — Roughing honesty | Implemented | Signed allowance and explicit stock/region errors replace the silent fallback. |
| P05 — Availability/source identity | Implemented for this increment | New behavior shares model/source validation, capability checks and duplicate diagnostics. Future features must extend that contract. |
| P06 — Illustrated help | Implemented | SVG parsing, offline rendering and inspector behavior tested; manual DPI/accessibility review of the live desktop remains. |
| P07 — Open profiles | Partial | Generated-fragment extensions and geometry extensions that change an open chain into a closed boundary remain. Current extensions preserve topology and reject invalid results. |
| P08 — Finishing | Partial | Separate lead policy per finishing pass remains. Lead-end feed transition distance is implemented; finishing passes share the existing lead settings. |
| P09 — Scheduling | Implemented | Even levels, smaller finishing depths, Rough final stage policy, one execution order and independent selection/travel ordering. |
| P10 — Roughing boundaries | Partial | Drawing snapshots/setup stock supported for one unlinked closed outside contour. Open-boundary clipping, partial stock intersections, explicit roughing-step limits and linked CAD roughing remain. |
| P11 — Tabs | Partial | Combining generated/manual locations, multiple contour ownership and combinations with ramps/overlap/roughing remain. Current placement modes are mutually exclusive. |
| P12 — Accuracy/corners | Partial | Source-to-post error budgets, inner-corner blends, sharp corners with loops, and separate rough/finish smoothing deviations remain. Fitting tolerance is explicitly not a source-accuracy guarantee. |
| P13 — Entry definitions | Partial | Custom arc sweep and linked exit shape implemented. Compound/perpendicular/vertical leads, validated predrill sources, per-contour entry sets, ramp-clearance and per-turn limits remain. |
| P14 — Wear compensation | Not implemented | Wear/inverse-wear register conventions, radius allowance and compensated-envelope verification. |
| P15 — Cutting-data/feed conveniences | Partial | Ramp RPM and restoration implemented. Linked surface-speed/chip-load/feed-per-revolution editors, corner feed zones and additional coolant types remain. |
| P16 — Linking policy | Partial | Fed retract implemented. User-defined safe/stay-down/lift limits and consistent rapid/high-feed lowering/verification remain. |
| P17 — Rest machining | Not implemented | Previous-tool/residual-stock geometry, revision dependencies and safe residual-only entry. |
| P18 — Taper | Partial | Reachable stepped profiles supported with the sign/source restrictions above. Continuous tapered-surface finishing and geometry-driven taper qualification remain. |
| P19 — Thin walls | Not implemented | Opposite-wall identification, support constraints and paired scheduling. |
| P20 — Indexed orientation | Not implemented | Per-operation frame, machine/post transitions, fixture clearance and simulation. Continue using separate setups for machining sides. |

Chamfer remains the existing `ChamferOp` and `ChamferStrategy`; no duplicate profile-chamfer generator was introduced.

## Validation

The original headless package suite passed before implementation. New regression cases cover bounded/invalid requests, allowances, finishing feeds/offsets/counts, even and finishing depths, ordering, control engagement/rounded posting, custom lead sweep, lead-end feed boundaries, ramp-RPM restoration, open-chain ramp/extension/direction, tab count/triangle protection, taper reachability, stock-boundary roughing, duplicate geometry, schema compatibility and inspector transactions.

The old roughing NC fixture passed `stock=None`, so it was actually pinning a single finishing pass. It now supplies stock and its NC reference includes the stock-frame cuts. The other clearing NC references remain unchanged. Unsafe-height tests now assert that generation rejects the setup; the simulation test also exercises deliberately unsafe IR independently to retain collision-detector coverage.

Validation results: the full workspace suite with the GUI feature passed **1,139 tests**, with 29 ignored. After consolidating the Arc editor, the complete GUI library suite passed **370 tests**, with 4 ignored. Workspace/all-target Clippy with the GUI feature passed with warnings denied. The final diff passes whitespace checks.

Validation commands for this increment:

```sh
cargo test --workspace --features cam-app/gui
cargo test -p cam-app --features gui --lib
cargo clippy --workspace --all-targets --features cam-app/gui -- -D warnings
```

The seven diagrams also render through the same resvg version used by iced, including their text labels. Rendering exposed unsupported SVG font shorthand; the assets now use explicit font-family/font-size properties with platform fallbacks.

These checks compile and exercise the desktop feature and headless CAD snapshots. They do not run a live desktop session, native OCCT import or a physical controller trial. Those qualification boundaries remain explicit, especially for controller compensation, source-bound CAD restrictions and tapered profiles.
