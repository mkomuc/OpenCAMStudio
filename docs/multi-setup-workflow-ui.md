# Multi-setup workflow and UI — discussion draft

**2026-10-14 planning update:** the
[high-level requirements analysis](high-level-requirements-analysis.md) and
[M1a/M3a milestones](implementation-plan.md) replace this draft's model-only
viewport and inline browser-action layout. The new plan requires a left overlay
tree, context commands/drag ordering and stock/bounds/WCS picking overlays in
Machining. Forms and transactional OK/Cancel remain in the right inspector.
The text below records the earlier agreed scope and its implementation baseline.

Agreed scope, 2026-10-13. Implemented in
[the multi-setup workflow](multi-setup-implementation.md). Confirmed scope: two box-stock modes and
separate NC output per setup; a right-column editor with Setup / Stock /
Postprocess tabs; edge-based X/Y/Z or model-coordinate-system WCS selection;
and a shared OK / Cancel transaction across all settings. Reference: the three supplied setup,
browser and stock screenshots.

## Workflow

Import the model once → create a setup → choose machining bodies, workpiece
coordinate system (WCS) and stock → select geometry → create operations →
generate and inspect → export that setup. Create or duplicate a setup for the
next physical orientation of the same part.

A setup represents one physical fixturing/orientation. A controller work-offset
number is a property of that setup; it is not its identity. Different physical
setups can reuse the same controller offset after the operator re-zeros it.
The project retains shared model geometry, while each setup has its own frame,
stock specification, safety heights and ordered operations.

## Layout and interaction

- Left browser: Models and Setups as separate expandable groups. Operations are
  direct children of their setup, in execution order, as in the reference.
- Setup rows: disclosure, visibility, generation status, name and explicit
  active marker. Operation rows: visibility, status, type, tool number and name.
  Visibility and inclusion in machining are independent controls.
- Center viewport: only the shared model and operation previews. Geometry
  hover/selection highlights remain on the model for picking. Do not place
  settings, selection filters, operation-creation buttons, stock overlays or WCS
  widgets in the center. Geometry controls and selection feedback belong in the
  right column; view/navigation commands can live in application chrome.
- Right editor: selecting a setup opens Setup / Stock / Postprocess tabs in the
  right UI column;
  selecting an operation opens that operation's parameters and geometry links.
- Selecting a setup activates it. Selecting an operation activates its parent.
  New operations always show their destination setup before confirmation.
- Setup actions: New, Rename, Duplicate, Reorder and Delete. Duplicating copies
  definitions and links, but requires regeneration. Deleting a setup deletes its
  contained operations as one undoable command.
- Operations can reorder within a setup. Copy/move between setups must re-resolve
  the source geometry and validate eligibility in the destination frame.
- All settings across the tabs share one draft. Switching tabs preserves that
  draft; it never applies changes. A persistent OK / Cancel footer stays in the
  right column. OK validates and commits all settings together as one undoable
  edit; Cancel discards all draft settings and restores the committed state.
  Model highlights can preview draft references without changing saved state.
  Generate/export uses committed settings only. Changing setup/operation with
  uncommitted edits requires resolving the draft with OK or Cancel first.
  WCS changes invalidate dependent results only when committed.

## Setup editor

**Setup tab:** name, selected model body instances, WCS and safety heights.

WCS offers two modes: **Model coordinate system** and **Selected model edges**.
Model coordinate system uses the selected model instance's coordinate system,
including its placement in the shared project. Default its origin to the model
origin, with explicit datum override available.

Selected model edges exposes separate X, Y and Z reference fields. For each
axis, click Select edge in the right column, then pick a straight model edge in
the viewport. Support clearing/replacing each reference and flipping each
direction. An edge determines a direction, not automatically the WCS origin.
An unspecified axis can be derived from two orthogonal selected directions and
must be labeled Derived. If all three are supplied, validate that they agree
with an orthonormal right-handed frame. Reject parallel, non-orthogonal,
degenerate, curved or inconsistent references with a field-specific message;
do not silently change the directions the user selected.
Choose the origin from model origin, stock-box points, model-box points, a model
vertex, or explicit coordinates. Show resolved directions and origin in the
right editor before committing; keep the center limited to model and operation
previews.
Changing the camera does not change the WCS.

**Stock tab:** stock geometry and material are separate fields. Confirmed first
increment: fixed-size box and box relative to model bounds. Fixed box has X/Y/Z
dimensions and model positioning along each axis. Relative box has independent
allowances on all six sides. Compute model dimensions in setup axes; show
dimensions and containment conflicts in the right column. Material names describe the
blank; material-specific cutting data can follow separately.

Further modes from the reference: fixed/relative cylinder, fixed/relative tube,
stock from a selected solid/bounding solid, and stock from a previous setup.
Previous-setup stock needs an actual remaining-material representation, a
dependency on the upstream result, and transformation into the new frame.
The present heightfield simulation cannot stand in for arbitrary rotated
remaining stock without additional work.

**Postprocess tab:** work-offset number, program name/number and export scope;
inherit machine/post defaults from the project initially. Confirmed default:
one NC program per physical setup. Optional combined output requires explicit
re-fixturing stops and supported post behavior, never an implied automatic flip.

## STEP selection and operation creation

Keep the existing Body / Face / Edge / Loop / Vertex filters, semantic hover,
multi-selection, overlapping-candidate chooser and select-through controls.
Tree and viewport selection must agree. Setup-edit pickers temporarily restrict
selection to the kinds appropriate for the edited field.

Support both selection-first and operation-first creation. Compatible operations
are prominent; unsupported operations give a concrete reason. Eligibility uses
topology, setup-relative orientation and the strategy's supported geometry,
not just entity kind. Initial linked operations remain Pocket, Drill, Profile
and Face with the existing qualifications:

- Body: choose machining target and stock bounds; body selection alone does not
  establish a pocket, hole or freeform machining strategy.
- Face: qualified planar pocket floor → Pocket; qualified planar top → Face;
  qualified cylindrical bore faces parallel to setup Z → Drill.
- Loop: qualified closed coplanar exterior loop with supporting walls → Profile.
- Edge: direction references; collecting a contour requires a complete validated
  chain. Chamfer/engrave bindings are later strategy work.
- Vertex: datum/point reference. A vertex alone is not evidence of a bore.

The operation editor retains linked source selections, derived boundaries,
islands and cutting heights, and distinguishes linked values from manual
overrides. Highlight sources when selecting an operation. Model replacement,
missing geometry or changed WCS invalidates the affected result; unresolved
included operations cannot generate/export.

## Current implementation and technical work

`Document` schema 14 owns a single `setup`; it already stores CAD assets,
instances and persistent operation bindings. `Setup::extra_origins` stores
translation-only datums. It does not provide independent rotational setups.
STEP semantic picking and linked operation creation already exist in
`cam-render` / `cam-app`; extend those paths rather than replace them.

Move to ordered `setups` with stable setup and operation identities. Keep active
setup and temporary selections in view state. Keep CAD source placement shared
and immutable during WCS edits. Resolve exact geometry into setup-local axes for
toolpath planning, and transform preview/simulation results consistently. Apply
the setup origin/rotation once; controller offset selection must not cause a
second translation. Heights and stock bounds use the same setup frame.

Migration preserves old physical cutting coordinates, operation order and
existing NC behavior. A legacy single-origin job becomes one setup. Legacy
extra-origin jobs need explicit compatibility handling: do not infer rotations
from datum positions or silently regroup interleaved operation sequences.
References, history, caches, generation stamps, completion/export gates and
project round-trip must all become setup-aware.

## Delivery and acceptance

1. Multi-setup model, migration, active setup and expandable browser. Existing
   projects retain output; save/reopen and undo preserve hierarchy and IDs.
2. Independent WCS, selected bodies, box stock and setup editor. A top/bottom
   fixture verifies the physical orientation, origin, bounds and cutting heights.
   Exercise edge references for all three axes, model-coordinate-system mode,
   axis flips and invalid reference combinations. Switching tabs preserves draft
   values; OK commits all tabs together; Cancel restores all tabs. Center content
   remains limited to model and operation previews throughout editing.
3. Setup-aware picking and linked operation resolution. The same model can
   produce valid operations on different sides; unsuitable geometry explains its
   rejection; changing one WCS invalidates the correct results.
4. Per-setup generation, simulation and export. Verify actual cutting coordinates
   and offsets; switching the active setup cannot export another setup's cache.
5. Additional stock modes, qualified remaining-stock transfer and later linked
   strategies as independently verified extensions.

Confirmed layout: docked right-column editor, preserving viewport access while
picking geometry. Setup / Stock / Postprocess tabs and OK / Cancel stay in that
column. Center shows model and operation previews only.
