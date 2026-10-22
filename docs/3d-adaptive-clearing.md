# 3D Adaptive Clearing and Stock to Leave

## Design

The reference screenshots describe a roughing operation that clears successive
Z levels with a controlled radial engagement and leaves a separate finishing
allowance on walls and floors. All application and code names use English.

The operation is named **3D Adaptive Clearing**. Its initial pass controls are:

| Control | Meaning |
| --- | --- |
| Tolerance | Maximum permitted geometry approximation error, in mm; also controls automatic frontier simplification |
| Optimal Load | Requested radial engagement, in mm |
| Stepdown | Maximum separation of primary roughing levels |
| Fine Stepdown | Intermediate levels for reducing the roughing staircase |
| Direction | Climb or conventional milling, interpreted with spindle direction |
| Stock to Leave | Enable separate radial and axial finishing allowances |
| Radial Stock to Leave | Material retained along model walls |
| Axial Stock to Leave | Material retained above model floors |

The existing operation Tool, Geometry, Heights, Passes and Linking tabs remain
the entry points. Selected geometry retains persistent CAD references; toolpath
generation must not depend on viewport tessellation or visibility.

To use the operation, import STEP/IGES solids, include the model bodies in the
setup, and choose **Machining → 3D Adaptive Clearing**. In Geometry, select the
bodies or faces to machine and choose a suitable end mill. Multiple references
may span model instances, and body and face selections may be combined. Selecting
a body includes its faces, so selecting one of those faces again adds no work.
Review the stock and height interval, then configure Passes.

**Stock to Leave** has its own section below the normal Passes controls for
Profile, Pocket, Face and 3D Adaptive Clearing. Its checkbox sits directly above
the radial and axial fields (Face uses axial only). Those fields remain visible
and are disabled while the checkbox is off. Apply the edit before
generating.

**Tolerance** is the only accuracy parameter in Passes. Frontier simplification
and corner detection automatically use one quarter of Tolerance: the default
0.1 mm gives 0.025 mm simplification. Smaller tolerances retain more detail;
larger tolerances suppress more short segments and small projection zigzags.
Older projects' separate simplification values are ignored and are omitted
when saving. Tolerance edits participate in Apply, Cancel, undo/redo and
toolpath cache invalidation.

## Selected machining areas

Selected faces limit machining to their projected XY footprints. Choose whether
the cutter stays Inside the boundary, its center follows the boundary, or it
travels Outside the boundary. A signed additional offset shifts the boundary;
positive values move it inward for Inside and outward for Center or Outside.
Complete vertical cylindrical faces use the analytic circle only when their
CAD topology has a full circular boundary and any axial seam cancels in the
closed wire. Partial arcs, unsupported vertical planes, and ambiguous/open
trims remain rejected. Full-solid obstacle shadows continue to protect all
setup bodies. Height overrides below the geometry-certified cutting interval
remain invalid because they cannot be certified against the selected geometry.

Selected bodies define local machining areas from their bounds in setup XY,
then apply the selected containment and signed offset. The stock clipping limit
allows the same outward runout, so Center and Outside can reach beyond stock
edges when the selected boundary reaches them. Inside with zero offset keeps
the cutter within the stock outline.
Entry helices must fit completely within the safe cutter-centre area. Where
there is insufficient room, the tool feeds straight down at the plunge feed
instead of swinging into the model or outside the containment boundary.
Before a layer is accepted, a separate safety check verifies every emitted
cutting move, entry and link against all setup bodies with the full cutter/shank
radius and CAD approximation margin. It checks entire segments and arc bulges.
Unsafe motions or a clearance plane below a setup body reject the operation;
their toolpath is not accepted for export. Frontier simplification receives its
own model clearance allowance.
For compatibility, selecting the only body in a one-body setup retains the
original full-stock clearing area; containment and offset still apply to that
stock boundary.

Every included setup body remains a protected obstacle, including bodies that
are not selected for machining. Moving, replacing or changing the setup's model
membership invalidates the adaptive geometry dependency key. Visibility changes
do not remove a body from machining protection.

Selected face floors receive a reachable cutting level just above their surface,
including axial stock allowance and the certified geometry/path margin. This
also clears the selected floor when the roughing stepdown spans the entire depth.

## Implementation sequence

1. Add a distinct adaptive operation and geometry extraction recipe. Persist its
   parameters with the project and participate in existing undo, caching,
   setup-coordinate conversion and export completion checks.
2. Resolve CAD geometry into conservative, height-dependent clearing regions.
   Clip the independent CAD machining tessellation above each tool-tip level and
   project the surviving triangles into XY. Their union protects the shadow of
   higher model material, including overhangs. Account for its recorded surface
   deflection, arc-refitting error and planar rounding before subtracting the
   shadow from stock. Reject unavailable or incomplete machining geometry.
3. Build primary Z levels and intermediate levels. After each deep roughing level,
   work upwards only where intermediate levels expose additional material beyond
   the remaining tolerance budget, then move to the next deep level. Unchanged
   vertical walls retain the primary stepdown. Disconnected regions are compared
   separately so a slope elsewhere does not repeat an unchanged pocket. Skipped
   changes accumulate against the last emitted pass. Protect every model shelf with the axial allowance, rather
   than only raising the operation's final bottom. Constrain tool centres by the
   cutter radius and retain the requested radial wall allowance.
4. Reuse the existing planar adaptive engine per region. Keep its coverage,
   engagement and gouge certification gates. If it falls back to concentric
   clearing, retain an explicit diagnostic; the requested load is not a universal
   guarantee in a full-width channel.
5. Add shared **Stock to Leave** settings to 2D Profile, Pocket and Face. Profile
   and Pocket retain wall and floor allowances; Face exposes the applicable
   axial component. Disabled settings preserve legacy toolpaths and existing
   operation offsets.
6. Integrate English creation commands, parameter fields and validation. Verify
   persistence, geometry selection, cache invalidation and complete-job export.

## Acceptance checks

- Different model heights produce different clearing regions.
- Cutting levels respect top/bottom bounds and leave the requested material.
- Geometry is invariant under changes to the display mesh.
- Unsupported, stale, malformed and unresolved geometry fails before export.
- Allowances reject negative and non-finite input; disabling them preserves
  existing output.
- Stock settings survive save/reopen, undo, duplication and setup moves.
- Cancellation marks a job incomplete and prevents posting it.
- Existing headless tests and GUI compilation continue to pass.

## Further reference controls

The screenshots also show minimum cutting radius, machine cavities, slot
clearing, flat-area detection, minimum stepdown, axial engagement and region/depth
ordering. Each requires a separate geometric or motion contract. They should only
be exposed once their behavior is implemented and tested. Freeform finishing and
rest machining are separate follow-up strategies.

## Geometric limits

The projected shadow approach is conservative three-axis roughing. It does not
attempt to reach material underneath an overhang. A region omitted by that
protection can retain extra material. Toolholders and machine components require
separate collision qualification; a clear cutter path alone does not qualify an
entire machine assembly. The current heightfield simulator also cannot represent
undercuts.

Repeating an Adaptive operation through an operation pattern is currently
rejected: a copied path would bypass the geometry checks at its new position.
Select multiple bodies/faces or create a separate operation at each location.

Selected faces and bodies, and the other included setup bodies used as obstacles,
require the independent machining projection embedded by the native importer.
Older projects without it require a model reimport. Display tessellation is never
substituted.
Tolerance must cover the native machining deflection, existing 0.01 mm arc
refitting, and planar-grid allowance. Settings below this budget are rejected.

## Verification

Rejected planar frontier segments are linked directly to the next cutting
segment's start. Intermediate rejected points and trailing repositions do not
produce plunge/retract cycles. The supplied `3d_toolpath_calculation.ocam`
regression checks that every retract follows actual cutting; this removes
3,272 empty cycles from that project while preserving every cutting segment
and the existing engagement, coverage and gouge certification gates.

Corner relief detection uses one quarter of the operation's Tolerance (default
0.025 mm) to ignore small projection zigzags. The original cutter-centre boundary still
guards motion and the resulting path still passes engagement, coverage and
gouge certification. This prevents microscopic facets on rounded hole entrances
from producing spurious corner relief loops. The supplied project regression
uses 0.08 mm Tolerance (0.02 mm simplification) and requires one retract per
cutting level inside its round pocket. It also checks that the full-depth primary
passes remain and that fine passes for the exterior slope do not repeat the
unchanged vertical bore.

Regression coverage includes changing model heights, overhang protection,
intermediate shelf allowances, deep-then-upward fine-level ordering, display-mesh
independence, cancellation, insufficient cutter reach and unavailable machining
geometry. Selection tests check local body bounds, sampled line/arc motion inside
face boundaries, protected neighboring bodies, mixed references, reachable floor
passes and rejection of inaccessible selections. Shared 2D allowance tests check
actual XY/Z motion changes, additive
legacy offsets, disabled-output equality and cache invalidation. App tests cover
staged edits, cancel/apply/undo, lifecycle copies and project persistence.

Reproduce the combined GUI/native checks with the local OCCT SDK configured:

```sh
cargo test --workspace --features cam-app/gui,cam-app/occt --locked --offline --no-fail-fast
cargo clippy --workspace --all-targets --features cam-app/gui,cam-app/occt --locked --offline -- -D warnings
cargo test -p cam-app --features occt --test native_step adaptive_ -- --ignored
```

The native smoke test imports `pocket-island-holes-mm.step`, creates and runs the
operation, exports NC, and checks identical output after saving and reopening.
It uses a 3 mm end mill, 2.9 mm optimal load and one 10 mm roughing slab with
10 mm fine stepdown. Fine-level ordering and shelf allowances have separate
geometry regressions. Desktop visual review and physical machine qualification
are outside this automated coverage.

Native selection tests also machine a selected component with another setup
body present, machine multiple selected component instances, and machine a
selected upward pocket floor. They check NC export, preserved face/body
references, and project reopening; the floor case asserts cutting below stock
top so an air-only entry does not qualify as successful machining.
