# Profile operation review and implementation plan

Review date: 2026-10-23. Source baseline: `8417102dfb75f4804aec6ca06eaa2d81f0aeba9d`, document schema 25.

This is a source review and proposed backlog. No machining behavior or GUI controls were changed. Suggested sequencing is advisory; the **User priority** column is deliberately unassigned so the work can be prioritized later.

## Reference and scope

The user-supplied **Fusion Help | 2D Contour reference | Autodesk.pdf** is the primary reference snapshot: 37 pages, printed 23 October 2026 at 09:57 according to its footer. Its SHA-256 is `0f9e2fbb4d9ca2cc658e523077d8bc08af6b12c48d1207b9dc57b68e91207735`. All pages were checked as extracted text; the tab and lead illustrations on pages 6 and 31 were also visually inspected. Page numbers below refer to this PDF. The PDF is an external review input, not duplicated into the source repository.

The supplied [Autodesk viewer link](https://help.autodesk.com/view/fusion360/ENU/?guid=GUID75B6821B-DE26-4E3B-AF10-4A54131CD9E4) returned “Page Not Found” through the browsing tool. Its exact GUID resolves through the [direct 2D Contour reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID75B6821B-DE26-4E3B-AF10-4A54131CD9E4.htm). The [current viewer reference](https://help.autodesk.com/view/fusion360/ENU/?contextId=MFG-REF-2D-CONTOUR-CMD) also resolves. The PDF confirms the intended subject and the feature inventory used here. These describe milling contours, which correspond to this application's **Profile** operation. This review does not substitute Fusion's cutting-machine “2D Profile” strategy.

The reference areas used for the gap inventory are geometry extensions and stock boundaries; tab placement; compensation; finishing, depth and ordering controls; corner treatment; linking and entry; cutting data; rest machining; taper, thin walls and orientation. The backlog below is an OpenCAMStudio design proposal, rather than a reproduction of Autodesk's dialog. Height behavior was also checked against [Setting machining heights](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID3EB12C22-C06B-4601-ABFE-7F1ECBABB9CE.htm).

Scope includes persisted parameters, CAD and drawing resolution, toolpath generation, inspector availability, hover help, preview and posting boundaries. “Present” below means implemented in the inspected source; it does not imply successful runtime qualification in this session.

## Findings to address before expanding functionality

### R1 — High: controller compensation has conflicting ownership and incomplete entry/exit handling

Evidence: [model compensation](../crates/cam-model/src/document.rs#L335), [GUI compensation picker](../crates/cam-app/src/gui_operation_tabs.rs#L204), [picker handler](../crates/cam-app/src/gui.rs#L3803), [strategy selection](../crates/cam-toolpath/src/profile.rs#L304), [simple emission](../crates/cam-toolpath/src/profile.rs#L721), [rich emission](../crates/cam-toolpath/src/profile.rs#L800).

For fixed geometry, spindle and `clearing.climb`, `Comp::ControlLeft` and `Comp::ControlRight` take the same strategy branch and generate the same compensation side. Selecting the two GUI choices produces a difference by also changing `clearing.climb`. The control therefore mixes compensation method with milling direction. Its displayed G41/G42 choice is inferred from climb alone, while generation additionally considers spindle direction and named-chain reversal.

Both emission paths enable compensation at the contour. The rich path has already emitted the lead-in, and disables compensation before the lead-out. The simple path permits control compensation without any lead. The post serializes these modal commands; the motion validator does not qualify a controller's compensation engagement requirements. Control-offset cutter motion also needs verification beyond the programmed centerline.

Reproduction to add: generate identical operations differing only in the two serialized control variants, then compare IR; repeat GUI selection with CW/CCW spindle and reversed named chains. Inspect activation/cancellation relative to entry/exit motion for supported posts. Existing availability tests only establish which picker choices exist.

Proposed owner: P01 for the canonical mode, P03 for engagement, post requirements and verification. Treat machine-specific consequences as unqualified until those tests exist.

### R2 — High: depth and ramp expansion can exceed resource or requested cutting limits

Evidence: [closed-profile checks](../crates/cam-toolpath/src/profile.rs#L217), [open-profile checks](../crates/cam-toolpath/src/profile.rs#L581), [depth levels](../crates/cam-toolpath/src/profile.rs#L1739), [bounded loop walker](../crates/cam-toolpath/src/profile.rs#L1396), [ramp construction](../crates/cam-toolpath/src/profile.rs#L1500).

The closed path checks `stepdown <= 0` but lacks the finite-depth/stepdown checks used by the open path. `depth_levels` repeatedly subtracts a step with no count limit, progress check or cancellation. Nonfinite public-API inputs and very small finite stepdowns can produce excessive allocation or a loop that never reaches its floor. For example, once `z` reaches `-1`, subtracting `1e-20` cannot change the floating-point value. GUI numeric checks do not protect every strategy caller.

`walk_loop` caps its output at roughly 32 laps. `contour_ramp` still reports the requested, possibly much longer distance and descends to the full requested depth over the returned path. Thus a sufficiently shallow requested ramp can become steeper without a diagnostic, and the full-depth cut can resume from a distance inconsistent with the actual ramp endpoint.

Proposed owner: P02. Reject an infeasible request or subdivide it while preserving the requested angle and stepdown. Resource limits must not silently increase tool load.

### R3 — Medium: roughing changes the meaning of allowance and can silently fall back

Evidence: [roughing dispatch](../crates/cam-toolpath/src/profile.rs#L327), [roughing region](../crates/cam-toolpath/src/profile.rs#L683), [numeric allowance validation](../crates/cam-app/src/gui_operation_tabs.rs#L167).

Single-pass compensation applies signed `offset`, including negative values. `roughing_region` enlarges the protected island only for positive allowance; negative allowance uses the original chain. Switching on radial roughing can therefore stop honoring a negative allowance. If stock bounds are absent or do not strictly contain the contour, a requested roughing operation falls back to one profile pass without explaining that stock clearing did not happen.

The roughing branch returns immediately after shared clearing succeeds. Its nearby comment refers to a following single-pass finish, but no separate lead-controlled finish is appended there. Do not describe the current operation as a configurable rough-and-finish cycle.

Reproduction to add: compare the final wall position for allowance `-0.2` with stepover zero/nonzero; request roughing with missing, touching and non-containing stock bounds. Use a supported unlinked profile, because linked CAD currently rejects negative allowance and roughing.

Proposed owner: P04 for consistent existing behavior; P08/P10 for new pass and boundary features.

### R4 — Medium: inspector availability is less restrictive than the execution contract

Evidence: [numeric inventory](../crates/cam-app/src/gui_operation_metadata.rs#L11), [inspector controls](../crates/cam-app/src/gui_operation_tabs.rs#L509), [open restrictions](../crates/cam-toolpath/src/profile.rs#L609), [tab restrictions](../crates/cam-toolpath/src/profile.rs#L275), [linked CAD restrictions](../crates/cam-app/src/cad_operations.rs#L729).

The Linking tab offers lead and plunge styles even when the open-profile strategy rejects them. Tabs require straight plunge, computed compensation, no overlap and no radial roughing, but these interactions are mainly enforced at generation. A single closed outside profile exposes stepover based on operation fields even if its CAD binding forbids roughing. Negative allowance passes the generic profile numeric check but is rejected for linked CAD.

Proposed owner: P05. One source-aware capability result should drive inspector availability, draft validation and execution, with a reason visible beside unavailable options. Preserve the user's draft; do not reset incompatible values silently.

### R5 — Medium: duplicate calculation paths and repeated selections can drift or repeat cuts

Evidence: [two perimeter helpers](../crates/cam-toolpath/src/profile.rs#L1007), [second perimeter helper](../crates/cam-toolpath/src/profile.rs#L1381), [tab preview](../crates/cam-toolpath/src/profile.rs#L20), [tab layout](../crates/cam-toolpath/src/profile.rs#L1091), [entry collection](../crates/cam-geometry3d/src/profile_entries.rs#L182).

`loop_length` and `loop_perimeter` calculate the same closed-polyline perimeter with different iteration syntax. This is an actual duplicate function under different naming.

Tab preview and generation separately calculate compensated loops, tab counts and centers. Only generation applies the full layout/entry-room checks. This is overlapping responsibility, rather than two completely interchangeable functions. A shared validated layout should supply both.

Named geometry entries are concatenated without cross-entry duplicate detection. Selecting the same boundary through a face entry and a chain entry can schedule it twice. This is a source-level risk requiring a regression fixture, not a runtime result established here. Distinct sides, intentionally different cuts and explicit repeat finishing must remain distinguishable.

Proposed owner: P01 for calculation consolidation; P05 for duplicate-source diagnostics.

### R6 — Low: graphical help exists, but most profile concepts still have text-only hints

Evidence: [shared tooltip primitives](../crates/cam-app/src/gui.rs#L1599), [selection hints](../crates/cam-app/src/gui_geometry_selection.rs#L107), [illustrated Climb control](../crates/cam-app/src/gui_operation_tabs.rs#L173), [existing SVG](../crates/cam-app/assets/help/climb-conventional.svg).

Climb already has a useful explanatory diagram. Numeric labels, compensation, entries and tabs mainly use prose. The common hover delay is 600 ms. `selection_hint` builds its own text popup and does not consult the global tooltip toggle itself. Some wording conflates XY lead motion with a Z ramp, and the generic Inside help can be read as promising pocket clearing, although an inner profile can leave a core.

Proposed owner: P06, extending the existing help machinery and artwork.

## Current coverage and gaps

The feature names in the first column map to the Autodesk reference. Implementation observations in the other columns refer to this repository. Each missing capability has one backlog owner; related aliases are consolidated below.

| Area | PDF pages | Current implementation and practical limits | Gap owner |
| --- | --- | --- | --- |
| Tool and cutting data | 2–3 | Tool selection, presets, RPM, cut/plunge and independent ramp/lead feeds exist. Cooling is None/MMS. Derived cutting quantities and independent ramp RPM are absent. | P15 |
| Geometry | 4–5 | Named Face Contours, Chain and Silhouette entries; filters, coplanar faces, reversal and per-contour sides exist. CAD qualification and common height limits still apply. | P05 for consistent availability |
| Multiple contours | 4, 21–22 | Stable entry order; each contour is completed through its depths and independently retracted. Tabs and radial roughing reject multiple chains. | P09, P11 |
| Open contours | 4–5, 14–16 | Left/right/on offsets, straight entry and retract per level exist. Leads, ramps, overlap, tabs, roughing and control compensation are rejected. | P07, P10 |
| Tangential extensions | 4–5, 14–15 | No geometry-end or generated-fragment extension parameters. | P07 |
| Stock contours | 5–6, 16–17 | Outside roughing uses a rectangular stock frame. Linked CAD disallows it. No selected stock boundary or open-end stock trimming. | P10 |
| Tabs | 6 | Rectangular, automatic distance or manual positions; width/height and viewport blocks exist. One closed contour only. Automatic and manual placement are exclusive. | P11 |
| Compensation | 10–12 | Computed and control paths exist, with the naming and engagement concerns in R1. On-contour placement already supplies a centerline mode. | P01, P03; P14 for wear/allowance |
| Wall/floor allowance | 24–26 | `offset` is existing radial wall allowance, ignored for Side::On. No independent floor allowance. Roughing has R3; linked CAD requires nonnegative offset. | P04, P08 |
| Finishing | 12–14, 18–20 | Closure overlap exists. No radial finish count/spacing, separate finish feed, spring-pass count, finish-only-at-bottom or finishing lead policy. | P08 |
| Depth passes | 17–20 | Maximum stepdown with a shorter final step exists. No equalized levels or dedicated finishing levels. | P09 |
| Ordering | 15, 21–22 | No selectable depth/contour/radial-step ordering or travel optimization. | P09 |
| Corners and smoothing | 9–10, 12, 14, 17, 26–27 | Rounded offsets and fixed 0.01 mm arc refitting exist. No operation error budget, selectable corner policy, corner blending or corner feed zones. Silhouette tolerance is a separate source-quality setting. | P12, P15 |
| Leads | 30–35 | Independent none/linear/90-degree arc leads and feeds exist on supported closed paths. No compound line-plus-arc definition, arbitrary sweep, perpendicular entry, vertical blend or linked exit definition. | P07, P13 |
| Ramps and entry positions | 35–36 | Closed paths support contour ramp, helix and zigzag. One XY start hint exists. No per-contour entry set or predrill association. | P02, P13 |
| Linking | 28–30 | Basic approaches, clearance retracts and some automatic stay-down behavior exist. No general stay-down distance/lift controls, retract feed policy or rapid-to-high-feed policy. | P16 |
| Heights | 8–9 | Five reference-based planes, offsets, selection references, preview/drag, validation and feed-height lowering already exist. | Reuse; P06 for illustrations |
| Rest machining | 7 | Profile does not consume previous-tool residual material. The simulator is not a profile rest-machining planner. | P17 |
| Wall taper | 19–20 | No depth-dependent profile offset. | P18 |
| Thin walls | 22 | No paired thin-wall schedule. | P19 |
| Chamfer | 22–23 | Dedicated Chamfer operation, tool geometry and shared leads already exist. Width and tip-depth controls already select the flank section; a tip-offset convenience should derive that depth. A second Profile chamfer generator would duplicate functionality. | Reuse existing operation |
| Tool orientation | 7–8 | Setup frames and multi-setup workflow exist. No independent indexed frame on Profile. | P20; reuse setup infrastructure |

The height implementation is substantially newer than the limitations recorded in the 2026-10-14 requirements analysis. Independent feed/lead feeds, coolant selection, multiple contours and tabs are also now present. This review supersedes those historical gap statements for Profile; they must not become duplicate feature tickets.

## Canonical functions, names and ownership

“No duplicates” is an implementation acceptance criterion. The current source has the R5 duplicate and R1 overlapping controls; this review does not claim they have already been removed.

| Names or apparently similar features | Canonical contract and reuse rule |
| --- | --- |
| Profile / contour / 2D contour | One `ProfileOp` and `ProfileStrategy`. Keep Profile as the product name; use “contour” for selected geometry. Do not add another operation variant for reference-name parity. |
| `loop_length` / `loop_perimeter` | Keep one closed-loop perimeter helper and migrate all callers. Keep `path_length` separately: an open path must not include the closing edge. |
| ControlLeft / ControlRight; G41 / G42; Climb | One compensation **method**, separate milling direction. Derive and display the effective controller side from actual motion and spindle. G41/G42 remain distinct IR output commands, not duplicate user methods. |
| Offset / leave / radial stock / wall allowance | One signed wall-allowance value, initially migrated from `ProfileOp.offset`. Use “Wall allowance” consistently in the GUI. A pure path translation would be a different feature and needs a separate requirement. |
| Lead overlap / finishing overlap | One contour-following closure-overlap value, migrated from `lead_overlap`; no second field. It is not the lead-out feed transition distance. |
| Repeat finishing / spring pass | One repeat count for the last nominal finish path. It differs from additional radial finishing cuts, which remove stock at different offsets. |
| Order by islands / order by contour | One contour-first schedule. Depth-first and radial-step-first are other values of a schedule policy, not independent contradictory checkboxes. Preserve-selection-order is a separate sequence/optimization choice. |
| Setup stock / selected stock contour | Sources for one roughing-boundary contract, not separate clearing engines. Route to shared `clearing::clear`. |
| Source extension / toolpath-fragment extension | Different stages: editing the selected curve can affect topology; extending a generated fragment affects motion. Use explicit stage names, one extension primitive, and avoid charging the same extension twice. |
| Tab preview / TabLayout | One validated layout result containing centers, protected spans, top and diagnostics; preview and motion consume it. Visualization is an adapter, not a second placement algorithm. |
| Leads / ramps / predrill | XY entry shape, Z descent and an entry-clearance source are distinct. Compose them through shared entry generation; do not alias one to another. |
| Silhouette tolerance / cutting tolerance / smoothing tolerance | Source projection quality, generated path accuracy and fitting error are distinct error-budget stages. Reuse a budget calculation; never promise accuracy finer than the source. |
| Profile chamfer / Chamfer operation | Route a contextual “Create chamfer” action to the existing operation and retain one chamfer implementation. |
| Text hint / graphical hint | One help-content model and renderer, with optional artwork. Existing label and selection wrappers can remain thin adapters. |

Keep the simple/rich closed emission branches until their equivalent cases have motion regression coverage. They have different entry/repositioning behavior and are not proven duplicates merely because both emit loops. Similarly, `lead_start_point`/`lead_end_point`, and preview/draft/source resolution adapters have different contracts. Consolidate shared calculations without deleting meaningful distinctions.

## Prioritizable backlog

All rows start **Proposed**. Sizes are relative engineering scope: S = localized, M = several modules, L = coordinated model/generator/GUI work, XL = new geometry or verification capability. They are not elapsed-time estimates. Dependencies name only prerequisites for the stated scope; existing shared infrastructure should be reused. Repair work is recommended before enabling dependent controls, while help can proceed independently.

| ID | Deliverable | Kind | Size | Depends on | Suggested sequence | User priority |
| --- | --- | --- | --- | --- | --- | --- |
| P01 | Canonical names and calculation consolidation | Repair | M | — | Foundation | Unassigned |
| P02 | Bounded, cancellable depth/ramp generation | Repair | M | — | Foundation | Unassigned |
| P03 | Qualified controller-compensation engagement | Repair | L | P01, P02 | Foundation | Unassigned |
| P04 | Consistent roughing allowance and explicit fallback | Repair | M | P02 | Foundation | Unassigned |
| P05 | Shared capabilities and duplicate-selection diagnostics | Repair/UX | M | P01 | Foundation | Unassigned |
| P06 | Illustrated, accessible hover help | UX | M | — | Early, parallelizable | Unassigned |
| P07 | Open-chain leads, descent and extensions | Feature | L | P02, P05 | Core expansion | Unassigned |
| P08 | Finishing cuts and wall/floor allowances | Feature | L | P01–P05 | Core expansion | Unassigned |
| P09 | Depth-level and contour scheduling | Feature | L | P02, P08 | Core expansion | Unassigned |
| P10 | Stock-boundary roughing | Feature | L | P04, P05; P07 for open chains | Core expansion | Unassigned |
| P11 | Expanded holding tabs | Feature | L | P01, P02, P05 | Core expansion | Unassigned |
| P12 | Accuracy budget, corner geometry and smoothing | Feature | L | P02, P05 | Advanced | Unassigned |
| P13 | Compound leads and qualified entry sources | Feature | L | P02, P03, P07 | Advanced | Unassigned |
| P14 | Wear compensation and radius allowance | Feature | L | P03, P08, P12 | Advanced | Unassigned |
| P15 | Cutting-data conveniences and corner feed control | Feature | M/L | P08; P12 for corner zones | Advanced | Unassigned |
| P16 | Configurable, verified linking and rapid policy | Feature | L | P03, P09, P10 | Advanced | Unassigned |
| P17 | Residual-material profile machining | Feature | XL | P08, P10, P12 | Advanced | Unassigned |
| P18 | Depth-dependent wall taper | Feature | L | P09, P12 | Advanced | Unassigned |
| P19 | Paired thin-wall scheduling | Feature | XL | P09, P12, P16 | Advanced | Unassigned |
| P20 | Indexed operation orientation | Feature | XL | P03, P05, P16 | Advanced | Unassigned |

### P01 — Canonical names and calculation consolidation

Replace `loop_length` with the existing `loop_perimeter` implementation after checking empty, single-point, explicitly closed and ordinary loop cases. Extract tab placement/layout into one result used by `profile_tab_preview` and emission. Keep source coordinates and compensated centers explicit.

Migrate control variants to one control method without silently rewriting saved milling direction. The current strategy ignores which control variant is stored once climb is fixed; migration should preserve the generated intent for supported legacy cases and explain ambiguous cases. Rename labels for allowance and overlap without adding parallel numeric storage. Update the stale “deferred” comments on compensation.

Touchpoints: model/document migration, `profile.rs`, operation field mapping, compensation picker, tab preview adapters. Acceptance: old fixture roundtrips retain cutting intent; one perimeter implementation and one tab-layout calculation remain; no new Profile/Contour strategy alias is introduced.

### P02 — Bounded, cancellable generation

Validate finite geometry, feeds, depth, stepdown, allowance, lead sizes and descent parameters at the public strategy boundary. Use a checked pass-count calculation, a proposed 100,000-level ceiling and a total motion budget shared with the job system. Reject requests that exceed a budget before allocating. Require numerical progress, check cancellation during level/ramp expansion and emission, and return diagnostics with no exportable partial result.

Keep the requested ramp angle and per-turn descent as limits. If the path budget is insufficient, report the required travel or choose a separately validated subdivision; do not steepen silently. Return the actual ramp endpoint/distance and use it for the finishing loop start.

Acceptance: NaN/infinity API inputs, `stepdown=1e-20`, no-progress subtraction, huge overlap and cancellation terminate predictably; a tiny contour with a shallow ramp never exceeds its configured descent limit. Preserve ordinary current depth sequences.

### P03 — Controller-compensation correctness

Create one qualified engagement/cut/disengagement sequence using post/machine capabilities. Validate the required linear approach/departure geometry and allowable compensation register; activate and cancel on qualified moves. Block or explain an inner lead rejected by the overshoot guard instead of silently generating an uncompensatable path. Require finish-only engagement when P08 is added.

Verification must account for the compensated cutter envelope. Until supported, a programmed-path backplot must explicitly identify the limitation and export policy must not treat it as fully verified physical motion. Retain rejection on GRBL/FluidNC/grblHAL/Estlcam; derive GUI eligibility from post capabilities instead of a second hard-coded list.

Acceptance: supported-post NC sequences for inside/outside, CW/CCW, reversed chains and valid/invalid leads; G40 precedes later traverses; unsupported posts fail clearly. Establish controller-specific entry requirements with fixtures before declaring qualification.

### P04 — Honest existing roughing behavior

Either honor signed wall allowance in the roughing island or reject unsupported negative values consistently before generation. Surface why requested roughing cannot be constructed; require an explicit single-pass mode rather than silently treating it as stock clearing. Align comments/help with the actual clearing result. Retain use of `clearing::clear`.

Acceptance: changing stepover does not silently change the target wall; invalid/missing stock cannot masquerade as successful roughing. Cover positive/zero/negative allowance and stock touching or crossing the selected contour.

### P05 — Shared availability and source identity

Provide one capability result from geometry kind, closed/open state, contour count, active options, tool and post. Use it for draft validation, numeric/enum availability and worker checks. Distinguish linked CAD restrictions from supported unlinked paths. Show a nearby explanation and preserve incompatible draft values until the user resolves them.

Detect repeated source entities and geometrically equivalent resolved contours across entries. Report which entries overlap and offer a deliberate resolution; do not silently discard geometry. An explicit repeat-finish request belongs to P08. Preserve paths that genuinely differ in side, topology or intent.

Acceptance: inspector eligibility matches execution for open leads, multi-contour tabs/roughing, CAD allowance/roughing and control compensation; duplicate face-plus-chain selections are explained before generation; OK/Cancel and undo remain transactional.

### P06 — Illustrated hover hints

Extend the existing delayed tooltip with structured content: title, short explanation, optional SVG, dimension legend and a limitation when relevant. Keep one renderer for field labels, pickers, checkboxes and geometry icons, with thin adapters for dynamic text. Reuse `climb-conventional.svg`; do not introduce another Climb or Direction control.

The detailed artwork and acceptance brief is below. Correct Inside wording to describe wall finishing, and distinguish flat lead geometry from downward ramping. Honor the existing tooltip toggle on every surface, including selection hints.

Acceptance: diagrams are explanatory at inspector scale; text alone conveys the same meaning; delay, focus help, toggles, scaling and popup placement work consistently. This increment can ship without new machining features.

### P07 — Open-chain entry and extensions

Reuse shared lead and descent primitives for open profiles while preserving endpoints and never adding an implicit closing cut. Model source start/end extensions separately from optional generated-fragment extensions. Share an extension calculation, define the order of application and show the effective result. Qualify intersection trimming before offering a topology-changing source extension.

Add alternating pass direction as a single cut-direction policy with clear climb/conventional implications; it must not also become an unrelated “reverse” switch. Retain chain reversal as source orientation. Keep a fixed-direction default compatible with existing projects.

Acceptance: asymmetric extensions on a line and curved chain; no doubled extension or closing chord; leads remain on the selected air side; ramped open paths reach depth without exceeding descent limits; alternating levels preserve side and endpoint safety.

### P08 — Finishing and allowances

Extend the existing Profile operation with one finish-pass specification: radial count/spacing, finish feed, final-path repeat count, finish-only-at-bottom and lead policy. Retain one wall allowance migrated from `offset`; add floor allowance separately from the selected geometric bottom. Explicitly define no-finish versus finish behavior: roughing reserve is not the same as intentionally retained final wall allowance.

Use a common pass representation with stage, contour, offset, depth and feed so roughing and finishing cannot emit the same nominal pass accidentally. Reuse existing overlap as closure overlap. Provide lead-out feed transition distance as a separate value only if implemented.

Acceptance: pass positions/counts, retained wall/floor material and feed stages are measurable in IR and simulation; repeat count alone repeats identical geometry; finish-only-at-bottom generates no earlier finish stage; control compensation is confined to eligible finishing cuts.

### P09 — Depth and contour scheduling

Add even roughing levels and explicit finishing-level size/count to the shared depth scheduler. Introduce one execution-order policy for contour-first, depth-first and radial-step-first behavior. Keep preserve-selection-order versus travel optimization as a separate ordering decision with a deterministic tie-break. “Rough final” becomes an explicit finishing-level policy rather than an independent overlapping depth algorithm.

Acceptance: depth 23 and maximum step 10 generates existing levels `-10,-20,-23`, or even levels near `-7.667,-15.333,-23`; two contours and two radial passes produce the documented schedule; no stage repeats or disappears. Confirm clearance transitions and cancellation between scheduled cuts.

### P10 — Stock-boundary roughing

Generalize the roughing boundary source from the existing rectangle to setup stock or a selected closed boundary, with explicit stock containment/intersection rules. Continue using the common clearing engine. Add selected roughing extent/step count without creating another stepover control for the same stage. Add open-chain boundary extension/trimming only after P07 is qualified.

Linked CAD needs explicit geometry and cutter-envelope qualification before its current restriction is lifted. Do not merely expose the existing stepover field there.

Acceptance: irregular boundary and island fixtures; no cut outside the declared permitted region; bounded partial stock intersections; no unreported single-pass fallback; DXF and qualified CAD paths agree for equivalent geometry.

### P11 — Holding tabs

Extend the shared layout with shape and placement policies: rectangle/triangle; distance/count; optional additional manual locations. Store generated and manual provenance so edits do not ambiguously convert all placements. Associate positions with stable contour identity before enabling multiple contours. Define seam clearance, overlap handling and tool-diameter expansion once.

Support combinations with ramps, closure overlap or roughing only in separately qualified increments. Until then, P05 must explain the restriction. A triangular retained tab requires cutter-envelope-aware rising/falling motion; changing its preview mesh alone is insufficient.

Acceptance: exact count per contour, distance limit, manual additions, seam wrapping, direction reversal, duplicate positions and invalid spacing; generated motion and preview consume the same validated result; every lower pass preserves the intended retained solid.

### P12 — Accuracy and corner treatment

Add operation cutting/fitting tolerances with a total error budget covering source projection, offsetting and arc refitting. Reuse `fit_arcs`; replace hidden fixed tolerances only where the budget is propagated. A finer requested tolerance must trigger re-resolution/reimport or an actionable error when the source is too coarse.

Add distinct corner geometry policies: outer roll/sharp/loop and inner minimum-radius blending, with independently qualified rough/finish smoothing deviations. These can change retained stock and must be visible in preview; fitting collinear/arc motion is a separate accuracy-preserving stage.

Acceptance: analytic circle/spline and sharp-corner fixtures bound actual deviation and retained material; no fitting crosses tab shoulders, lead junctions or feed-zone boundaries. Include coarse imported silhouettes and post rounding in the reported error budget.

### P13 — Compound leads and entry sources

Evolve the shared `Lead` definition, preserving old linear and 90-degree-arc meaning. Support line-plus-arc, sweep angle, perpendicular extension, vertical blend and an optional exit linked to entry. Reuse the same geometry for guarding, ramp sampling, preview and emission. Do not retain independently editable exit values while presenting them as linked.

Add per-contour preferred entries and predrill references with clearance validation against the tool envelope, depth and previous operations. Extend contour-ramp settings with maximum per-turn stepdown and start clearance. A predrill point alone is not evidence that the material is clear.

Acceptance: non-90-degree arcs, compound and mirrored exits, narrow inside features, unsafe/insufficient predrills and per-turn limits; preserve tangency and compensation engagement; tool-suitability checks remain effective.

### P14 — Wear and compensation allowance

Extend the P01 compensation method with qualified wear/inverse-wear conventions and an allowed tool-radius range. Reuse compensated nominal paths and P03's engagement sequence. Define register sign/radius-versus-diameter conventions per post/machine; show the operator expectation explicitly.

Acceptance: nominal and extreme permitted tool sizes preserve the part boundary; incorrect registers/modes are rejected; worn-tool envelope verification agrees with posted motion. No duplicate ControlLeft/ControlRight user modes return.

### P15 — Cutting data and local feed control

Provide surface-speed, chip-load and plunge-feed-per-revolution editors as linked views of canonical RPM/feed values, using actual tool diameter and flute count. Define the last edited quantity as the driver and recompute once. Add optional ramp RPM with explicit restoration before normal cutting. Extend the existing shared cooling model and post adapters if additional coolant types are needed.

Add corner feed zones based on direction/radius, lead-in distance and inner-only filtering; keep this separate from changing the corner's geometry. Use finish feed from P08 rather than introducing another finish-rate parameter here.

Acceptance: editing either representation roundtrips without feedback loops; missing flute count is explained; ramp RPM restores on every exit/cancellation boundary; feed-zone transitions survive arc fitting; unsupported cooling is rejected at application/post boundaries.

### P16 — Linking and rapid output policy

Expose bounded stay-down distance, lift height, safe distance and rapid-versus-fed retract as a common linking policy. Prove the swept link is clear of remaining stock, tabs, retained islands and fixtures; otherwise retract. Preserve existing conservative defaults.

Place rapid/high-feed conversion in the shared motion/post pipeline with one policy for axis classes and a configured high feed. Apply the same transformed motion to timing, validation, simulation and NC output; do not change only the displayed NC text.

Acceptance: safe links remain down; obstructed and long links lift; tabs survive; configured feed/retract behavior and diagonal/single-axis rapid conversion agree across IR, simulation and supported posts.

### P17 — Rest machining

Begin with an explicit previous-tool diameter/corner-radius geometric envelope and clearly name its approximation. A later previous-operation mode should consume revision-stamped residual stock, including earlier setup/WCS state. Reuse tool geometry and verification; do not treat a display heightfield as exact leftover stock without establishing resolution bounds.

Acceptance: a smaller tool cuts only qualified residual regions in a narrow-corner fixture; stale previous-tool/operation state invalidates the result; empty residuals produce no cuts; conservative approximation and inaccessible material are reported.

### P18 — Depth-dependent wall taper

Add depth-dependent offsets with an explicit reference plane, taper sign and tool-envelope clearance at every level. Use the common pass representation and offset engine. Make operator-defined slope distinct from geometry-certified wall shape and account for wall/floor allowance consistently.

Acceptance: analytic top/bottom dimensions and correct reference-plane behavior for positive/negative taper; clearance and retained material at every depth. Route chamfer requests to `ChamferOp` rather than treating a chamfer as a duplicate taper strategy.

### P19 — Paired thin-wall scheduling

Detect opposite sides within a declared wall-width threshold and schedule removal in pairs while preserving remaining support. Reuse the P09 scheduler with an explicit pairing constraint. Merely sorting nearby contours is insufficient; state what support/stiffness the planner can actually infer.

Acceptance: thin-wall fixtures demonstrate paired cut order and retained support at each stage; unpaired or ambiguous geometry is reported. Keep this independently prioritizable from taper and orientation.

### P20 — Indexed operation orientation

Reuse existing setup frame/WCS selection and post support for an optional indexed operation frame and origin. Qualify machine transitions, transformed heights, fixtures, simulation and posting; until then use the existing separate-setup workflow. Do not introduce another coordinate transform beside the shared frame machinery.

Acceptance: transformed heights, cutter envelopes and NC agree under a rotated frame; unsupported machine/post combinations are rejected. Keep continuous multi-axis machining outside this scope.

## Hover graphics design brief

Follow the shared [GUI style specification](gui-style-spec.md) for typography, popup surfaces, controls and optional sections. Reuse its existing style helpers when implementing the illustrated hints.

Illustrations should explain a spatial relationship or a before/after effect. Names, RPM and other self-explanatory scalar fields can keep text-only help. Reuse existing SVG/canvas support, bundle original artwork locally under `assets/help`, and avoid copying Autodesk illustrations. Source icons indicate actions; they do not substitute for explanatory diagrams.

| Control/concept | Useful graphic | Text that must accompany it |
| --- | --- | --- |
| Cutter side | Top view: closed outer/inner boundary and directed open chain; tool-center circles and left/right arrows. | Which material is retained; open sides follow chain orientation; an inner profile does not clear the whole pocket. |
| Compensation | Nominal edge, computed centerline and control-offset path side by side; label the active method and derived G code. | Who supplies the radius and when the actual cutter envelope is qualified. |
| Wall/floor allowance | Cross-section of finished wall/floor and shaded retained bands; signed dimensions. | Positive leaves material; negative cuts beyond the nominal target; scope restrictions. |
| Stepdown and stepover | Paired side/top views with depth levels and adjacent radial paths. | Axial versus radial distance; smaller last step or even-level option when supported. |
| Holding tabs | Top view of spacing/width and side view of retained height; rectangle versus triangle when available. | Retained width versus expanded cutter travel; height measured from the final floor. |
| Leads and overlap | Directed contour with entry, exit and highlighted repeated closure segment; dimensions along the relevant path. | Flat lead motion is distinct from downward ramping; overlap re-cuts the contour. |
| Plunge/ramp/helix | Simple side/isometric comparison, with descent angle and per-turn height. | Required cutting surfaces and any length/angle limit; predrill is validated separately. |
| Geometry entries | Small face-with-hole, directed edge chain and projected silhouette examples. | Source selection versus resolved machining boundary; inner/outer filtering. |
| Heights | Side view of the five labeled planes, datum and one reference-plus-offset dimension. | Reference, resolved absolute Z and which movements use the plane. Reuse existing height-plane colors. |
| Climb/conventional | Existing `climb-conventional.svg`, retained and brought into the shared help content. | Current spindle/side implications and both-sided engagement for on-contour slots. |
| Later features | Drawings for finish cuts, extensions, corner policies and stay-down obstacles when those controls ship. | Explain only supported behavior; help must not advertise a deferred feature as available. |

Proposed rendering contract: roughly 320–440 logical pixels wide, constrained to the available window; vector artwork; concise title and 2–4 sentences; labeled arrows, dimensions and distinct line styles as well as colors. Show units and define the view direction. Use the same semantic colors as the viewport where possible. Keep text readable at 100%, 150% and 200% scale and in the supported theme. Do not load artwork over the network.

Retain the 600 ms hover delay. Provide the same explanation through keyboard focus/F1 or an equivalent focusable help action, and keep explanatory text available to accessibility tooling. Hints must not commit edits, capture machining selection clicks or cover their trigger. Place/flip/constrain the popup near window edges and on narrow inspectors. Test all of these with the tooltip setting enabled and disabled.

Acceptance includes SVG parsing, focus/hover behavior, readable screenshots at the stated scales, and domain review of the drawing's tool side, arrows and dimensions. A parser check alone does not establish that the graphic explains the machining correctly.

## Delivery and validation gates

Every machining increment should include persisted defaults and schema migration where needed; one canonical value for each concept; model/strategy validation shared with GUI capability reporting; draft preview, OK/Cancel and undo; cache/fingerprint invalidation; and consistent toolpath, simulation and NC behavior. Never introduce a GUI-only machining setting.

Keep the existing operation variants and shared clearing, lead, height, arc-fitting, tool-profile and post infrastructure. Extract a common calculation only when its contract is shared; avoid another generic engine beside a working one. During review of each implementation, inspect callers and serialized fields as well as function names. Matching names alone do not detect duplicate behavior.

Existing source tests worth retaining and extending:

- `profile.rs`: side/spindle direction, named reversal, tab preview agreement, rectangular/manual tab protection, open offsets without closure, multi-contour atomic failure, ramp/lead continuity and roughing engagement.
- `profile_entries.rs`: filters/side rules and open-chain reversal without implicit closure.
- `plan.rs`: reference heights, feed-height motion and cache reuse.
- `gui_operation_tabs.rs`: transactional tabs/undo, cutting fields and compensation availability.
- `gui_operation_preview.rs`, `gui_profile_tabs.rs`, `gui_profile_sides.rs`: tab blocks, picking and contour-side interaction.
- `cam-app/tests/open_profile_blend.rs`: ignored saved-project test depends on `testfiles/wrong_z.ocam`, which is not a bundled portable fixture. Replace/add a self-contained fixture before counting it as regression coverage.

Suggested execution for implementations: `cargo test -p cam-model -p cam-toolpath -p cam-geometry3d -p cam-post -p cam-app`; use a focused filter while iterating. Run `cargo test -p cam-app --features gui` for inspector/help changes and native `gui,occt` qualification for changed CAD import/extraction behavior. Include controller-specific NC fixtures and stock-removal/clearance checks for changes affecting physical motion. Broaden to the workspace suite after shared contracts change.

Session evidence and limits: repository status was initially clean; baseline and schema were read from source; the user-supplied PDF was checked across all 37 pages and its tab/lead graphics visually inspected; the exact Autodesk GUID page and current reference were retrieved; source and existing tests were inspected. Rust/Cargo are not installed in this environment, so no Rust tests, GUI session, OCCT run or machine trial was executed. Findings are based on traced source branches, with reproduction/acceptance cases specified above. Documentation links and backlog identifiers were checked locally; that does not qualify machining behavior.
