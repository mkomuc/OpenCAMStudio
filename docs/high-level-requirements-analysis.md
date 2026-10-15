# High-level requirements: implementation impact

Reviewed **2026-10-14** against [high-level functions](high-level-functions.md),
the [tool-library reference](tool-library-implementation-reference.md), and the
current source. Delivery gates are in the [implementation plan](implementation-plan.md).
This is a planning update; it does not claim the new behavior is implemented.

The source document's imperative wording describes desired product behavior.
Its screenshots illustrate that behavior; they do not require every visible
Fusion control to be implemented. The requested operation editors expose
parameters supported by the current backend. The explicit eight-type tool limit
takes precedence over the reference's proposed 34-type catalog. Cloud libraries,
turning tools and full Fusion expression compatibility are outside this increment.

## Requirement coverage and gaps

| ID | Requirement | Current evidence / remaining work | Delivery gate |
| --- | --- | --- | --- |
| H01 | Start in Machining; model/drawing fills the main view with a dimensioned XY grid | `gui.rs` starts on Home and uses docked panes. Introduce workspace modes and a scaled, labeled grid; retain camera/picking behavior | M1a |
| H02 | Project tree overlays the left edge; rows carry visibility/status; commands use right-click; drag/drop controls execution order | Existing tree has inline include/reorder/actions and some context menus. Move commands to menus and implement undoable drag/drop with dependency checks; visibility must remain separate from machining inclusion | M1a |
| H03 | Fixed-axis three-axis setup first | Existing frame and planar strategies are reusable. Manual setup reorientation is supported; rotary machine motion, multi-axis tabs and turning remain later scope | M3a; M9/M10 later |
| H04 | Fixed and model-relative box stock | `Stock::FixedBox` and `RelativeBox` already exist. Preserve entered dimensions and independent six-side allowances | M3a |
| H05 | Stock from the previous setup | No remaining-stock variant; current simulation is a per-setup heightfield. Add an explicit upstream result dependency and transformed stock snapshot, never substitute the finished target model or its box | M7a |
| H06 | Select one or more machining bodies; single body/drawing has an unambiguous default; remaining bodies may be fixtures | `Setup::model_instances` and body/instance picking exist. Clarify automatic defaults and target/fixture roles for repeated instances | M3a/M3b |
| H07 | Per-axis min/center/max model placement plus offset in stock | Fixed boxes have `model_position` fractions and anchor controls, but no independent XYZ offset. Extend the stock placement contract and editor; keep this distinct from WCS orientation | M3a |
| H08 | WCS dropdown: model coordinates, point on model, point on stock; highlighted snapping to vertices and differently colored bounding boxes | Model frame, model edges and origin picking are reusable. Add explicit origin-source modes, model/stock bounds candidates, snap highlighting and persistent references | M3a |
| H09 | Edge-defined WCS orientation, second axis to resolve remaining freedom, visible triad with clickable 180-degree flips; fully constrained frame | Two orthogonal model-edge directions and frame validation exist. Add stock-edge references, viewport triad picking and clear unresolved-axis feedback. One selected edge alone leaves rotation unresolved | M3a |
| H10 | Material library on stock; material selects a tool cutting preset; operation overrides remain possible | `Setup::material` is a string; tools have nominal RPM/feed/plunge only. Add material IDs and explicit tool/material preset associations, with override provenance and missing-match feedback | M2a + M3a |
| H11 | Machine choice selects the post for all operations in that setup | `MachineLibrary::MachineEntry` already pairs machine and post, but `Setup` has no installation binding. Add setup-level selection/resolution without replacing local machine limits with project data | M3a; M2/M4 extend policy |
| H12 | Selected fixture bodies participate in collision detection | No per-setup fixture assignments or general body collision verification. Store roles/transforms first; route them into M5 swept checks and report checking coverage | M3b + M5 |
| H13 | Tools replaces the model in the main window; editing tabs live in the inspector; reuse existing preview and tool kinds | Existing library, kind-specific fields and generated preview are reusable; library currently docks left. Adapt browser placement and add transactional General / Tool / Shaft / Holder / Cutting Data / Postprocessor tabs | M1a + M2a |
| H14 | Import manufacturer Fusion 360 tool libraries | Existing library persistence/native exchange is insufficient evidence of Fusion compatibility. Add a fixture-backed import adapter, preview, unit/type mapping and explicit unsupported-record handling | M2b |
| H15 | Selection-first and operation-first creation; Ctrl-select multiple same-kind features | Both workflows and CAD multi-selection exist. Enforce homogeneous selection for an operation's geometry field and validate actual topology/strategy compatibility. Several selected features do not imply a multi-chain Profile backend | M1b |
| H16 | Operation inspector: Tool, Geometry, Heights, Passes, Linking; multi-axis later; groups vary by operation | Existing fields are mostly a flat inspector. Group actual model/strategy parameters into tabs with one shared draft; omit unavailable Fusion parameters and retain linked/manual height distinction | M1b |

The newer viewport requirements replace the older model-only center described in
[the setup UI draft](multi-setup-workflow-ui.md): stock, model bounds, WCS and
picking overlays now belong in the machining viewport. Setup/tool/operation forms
remain in the right inspector. The older implementation report remains a record
of shipped behavior, not an acceptance gate for the new layout.

## Backend parameter inventory for operation inspectors

Evidence: [`cam-model::document`](../crates/cam-model/src/document.rs),
the corresponding [`cam-toolpath` strategies](../crates/cam-toolpath/src),
[`plan.rs`](../crates/cam-toolpath/src/plan.rs),
[`cad_operations.rs`](../crates/cam-app/src/cad_operations.rs), and the existing
[`gui.rs` inspector](../crates/cam-app/src/gui.rs).
The table records backend support, not a claim that every field is currently in
the GUI or that every combination is qualified for STEP-linked operations.

All eight operations reference `tool`, `spindle_rpm` and `work_offset`.
The planner consumes RPM and datum selection; these are not strategy geometry
parameters. RPM zero means inherit the job default, rather than command zero RPM.
Work offset belongs to setup/post context; retain legacy per-operation origins
without confusing them with physical setup identities.

| Operation | Geometry | Heights | Passes | Linking / entry |
| --- | --- | --- | --- | --- |
| Profile / 2D Contour | One `chain`; `side` and `comp` determine path/compensation behavior | `depth`; linked top/bottom where available | `offset`, `stepdown`, `stepover`, `clearing.engagement`, `clearing.climb` | `start`, `lead_in`, `lead_out`, `lead_overlap`, `plunge` |
| Pocket | `boundary` and `islands` | `depth`; linked top/bottom where available | `clear.stepdown`, `clear.overlap`, `clear.offset`, `clear.clearing.engagement`, `clear.clearing.climb` | `start`, `clear.plunge`, `clear.lead_in`, `clear.lead_out`, `clear.lead_overlap` |
| Drill | Multiple `points`; qualified linked cylindrical bores | `start_offset`, `depth`; linked entry/bottom where available | Optional `peck` and `dwell` | Cycle approach/retract is determined by supported cycle semantics; no configurable lead/plunge style |
| Face | Rectangular `boundary` for the current facing strategy | `start_offset`, `depth`; linked top/bottom where available | `stepdown`, `overlap`, `overshoot`, `direction` (X/Y) | Fixed strategy linking; no user lead/plunge-style fields |
| Thread mill | Multiple `points`, `internal`, `major_dia` | `z_top`, `z_bottom`, `blind_allowance` | `pitch`, `hand`, `climb`, `passes`, `gradual`, `spring_passes` | `drill_clearance`; fixed helical entry/exit, no generic lead fields |
| Chamfer | One `chain`, `side` | `top`, `depth` | `width`, `step`, `gradual` | `start`, `lead_in`, `lead_out`, `lead_overlap` |
| Engrave | One `chain`, `closed` | `top`, `depth` | `stepdown` | `start`; fixed entry/exit, no generic leads |
| Carve | `boundary`, `islands`; optional clearing-tool binding | `top`, `depth` | `offset`, `ring_step`, `scallop`; optional clearing `ClearParams` | `start`, `plunge`, `stay_down`; clearing phase has its own `ClearParams` linking |

Tool-tab cutting data maps to `feed` and `plunge_feed`, except Drill which has
only `feed`, and Pocket which stores them in `clear`. Carve's clearing phase
has separate feeds; its current operation RPM must not be represented as an
independent clearing-tool RPM setting. Material/preset selection in M2a resolves
these existing values and preserves manual overrides.

`Lead` supports None, Linear(length), Arc(radius). `Plunge` supports Straight,
Ramp(angle), Helix(radius, pitch), ZigZag(length, angle). Show units and only the
parameters applicable to the selected variant. `ClearParams` is shared by Pocket
and Carve clearing; reuse its schema and validations.

`Heights` currently supplies setup-level `clearance`, `retract` and
`top_of_stock`; CAD bindings additionally retain derived top/bottom and explicit
overrides. The Heights tab must label scope and inheritance. The screenshots'
independent feed plane, arbitrary reference dropdowns and per-operation
clearance/retract do not yet have equivalent backend contracts.

Important qualification boundaries:

- STEP-linked generation currently qualifies Pocket, Drill, Profile and Face.
  Chamfer, Thread, Engrave and Carve remain available for existing planar inputs;
  adding their CAD bindings is separate backend work.
- Profile radial clearing currently applies to outside/computed compensation.
  The STEP Profile slice is further restricted to the qualified exterior
  silhouette without radial roughing or controller compensation. UI availability
  must depend on input kind and post capabilities, not just serialized fields.
- Multiple selected features can form one validated chain, boundary/island set or
  hole group. Multiple unrelated contour chains need a later multi-chain contract
  or explicit separate operations; do not silently discard all but one selection.
- `clearing.engagement = 0` disables its cap; `clear.stepdown = 0` clears the
  full depth in one pass. Label these modes rather than presenting zero as a
  commanded feed or cutting depth.
- Current planning emits flood coolant; operations have no selectable coolant
  field. Nine catalog coolant values may be stored by M2a, but applying them to
  NC requires a supported operation/planner/post adapter in M2/M4.
- There are no independent lead-in/out/transition/helix feeds, ramp RPM, finish
  feed/pass count for Profile, separate wall/floor allowance, smoothing controls,
  holding tabs, lift height, arbitrary lead angles, vertical lead radii or general
  preferred entry/exit positions. Do not copy these screenshot controls into the
  first inspector increment. Surface speed/chip load can be derived in M2a;
  they are not independently consumed operation parameters today.

## Tool-library scope adapted from the reference

Keep **EndMill, BallMill, BullNose, ChamferMill, Drill, FaceMill, ThreadMill,
VBit**, including existing type-specific dimensions and compatibility filters.
Reuse the current generatrix/preview builder; extend it with ordered shaft
segments and optional holder components through one geometry contract.
The current `Tool.length` describes stickout, despite an overall-length UI label;
map overall length, projection, shoulder and assembly gauge length explicitly
before migrating records or importing vendor geometry.

Apply reference B01–B10 and B14–B17 within this eight-type scope: metadata,
local/document browsing, tabs, quantities/expressions, validation, shaft/holder
editing, independent presets, persistence, help and operation binding. Split
B18: portable/Fusion exchange is near-term M2b; cloud synchronization and live
supplier providers remain deferred. B11–B13 catalog expansion is deferred.
Existing library commands define their semantics; inferred Fusion toolbar icons
are not a reason to add unverified behavior.

Use stable material IDs for preset matching; a free-form name such as “Alu” alone
does not establish an aluminum material association. New operations prefer a
matching preset, otherwise show the explicit fallback. Changing stock material
or library data must not silently overwrite operation overrides or approved
snapshots. Unsupported post settings can be retained as catalog metadata, but
must be rejected when applied to a machine that cannot execute them.

Fusion import acceptance must use real manufacturer files. Record supported
format versions, units, type mapping, shaft/holder and preset coverage. Show
accepted/rejected records and unsupported fields before commit; never coerce an
unsupported cutter into an EndMill or treat an NC register as a physical length.
Validate the staged import and commit it atomically; duplicate/provenance policy
must be explicit. Native library JSON round-trip is a separate acceptance case.

## Dependency consequences

The workspace and operation-tab refactors can reuse existing commands and do not
need the complete industrial machine simulator. Setup placement and WCS need
domain/picking changes. Material defaults need the new preset model; fixture
selection needs persistent roles before collision checking; vendor import needs
the tool aggregate before mapping rich records.

Previous-setup stock is actual material remaining after upstream machining. Store
the source setup, result fingerprint, stock representation, transform and accuracy.
Reject missing, failed, cancelled or stale snapshots and dependency cycles.
Upstream edits invalidate downstream stock, paths, verification and export.
First qualify a declared aligned heightfield subset; arbitrary reorientation
requires volume-capable stock. M7a can deliver stock transfer before full-machine
simulation, but it cannot use a rotated heightfield as a general solid.

Remaining design choices are issue-level work: documented Fusion format subset,
tool length/gauge conventions, material taxonomy, parameter-expression grammar,
and the qualified stock-transfer subset. They do not require expanding the
current product scope or delaying the workspace milestone.
