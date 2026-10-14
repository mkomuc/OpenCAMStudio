# CAD import and geometry selection for operations

Proposed feature plan, 2026-10-13. Source inspected: `f17b556`.
Implementation of the first fixed-axis STEP slice is tracked in
[cad-import-implementation.md](cad-import-implementation.md), including tested
capabilities and remaining release gates.

## Product outcome

Import a STEP part, select its faces, edges or holes in the viewport, and use
those selections as the geometry of machining operations. Preserve those links
when saving and reopening the project. Make inferred dimensions visible and
editable, and make unresolved geometry explicit after model replacement.

The first release uses 3D CAD geometry to drive the existing fixed-axis 2.5D
strategies. Arbitrary curved-surface machining, indexed setups and automatic
recognition of every manufacturing feature are separate work.

“Feature” has two levels here:

- **Selectable geometry:** bodies, faces, edges, closed loops and vertices.
- **Machining features:** a pocket, hole, planar machining region or profile
  derived from a selected set of geometric entities and the machining direction.

Do not depend on recovering the authoring CAD application's parametric history.
Recognition in this feature operates on imported geometry and topology.

## User workflow

1. **Import model:** choose a file; preview its units, dimensions, bodies and
   import diagnostics. Add it to the project as a geometry asset. Replacing an
   existing asset is a separate command. Failed or cancelled imports leave the
   current project intact.
2. **Place the part:** choose the machining top face, X direction and datum;
   preview the resulting axes. For the first release, place the part into one
   fixed machining frame. Define stock independently, with optional model bounds
   plus user-entered allowances.
3. **Select geometry:** hover highlights a complete face or edge, click selects,
   modifier-click toggles multiple entities, Escape cancels. Offer Body / Face /
   Edge / Loop / Vertex filters, hide/isolate controls, and a candidate chooser
   for overlapping entities. Geometry tree and viewport selection agree.
4. **Create operation:** either select first and choose a compatible operation,
   or start an operation and select into its Geometry field. Show incompatibility
   reasons rather than quietly projecting unsuitable geometry.
5. **Review parameters:** show source selections, boundary/islands, tool, heights,
   depth and allowances. Values show whether they are linked to geometry or
   manually overridden. Preview the derived machining region before confirming.
6. **Generate and inspect:** use the existing toolpath, simulation and export
   workflow. Selecting an operation highlights its source geometry; “Edit
   geometry” reopens the selection field. Undo/redo covers committed changes.

Example: import a plate with a blind pocket, a raised island and four holes;
select the pocket floor → Pocket; select the four cylindrical holes → Drill;
select an exterior loop → Profile. Save and reopen with all selections retained.

### Selection-to-operation rules

| Operation | Selection | Derived information and initial limits |
|---|---|---|
| Pocket | Planar floor face | Outer boundary, preserved regions and floor Z; certify a closed, accessible pocket with vertical walls and constant cross-section over the cutting interval. Reject undercuts, sloped walls, blended floors and ambiguous openings initially. |
| Drill | Cylindrical bore faces or validated hole features | Axis, centre, diameter, entry and termination; axis must align with setup Z. Group only holes with compatible entry/depth/cycle parameters. Circular edges alone do not establish a hole or its depth. |
| Profile | Closed coplanar edge loop | Ordered boundary, winding and selectable cutting side; derive depth only when supporting topology establishes the intended wall extent. Validate the side swept by the cut against the selected body. |
| Face | Planar target face | Target Z and selected machining area; user reviews stock to remove and boundary expansion. Adjacent bosses or other protected material must not be crossed. |
| Chamfer / Thread / Engrave / Carve | Compatible edges, holes or planar regions | Follow-up adapters after the core four. Thread pitch/form remain explicit user parameters unless separately supported metadata supplies them. No inference of thread specification from bore diameter. |

Inspect adjacency and solid occupancy to distinguish a raised island from a hole
in a pocket floor. Preserve excluded regions conservatively; do not treat every
inner wire as interchangeable. Split cylindrical seam faces must resolve to one
hole. Blind-hole drill-tip allowance and through-hole breakthrough remain
explicit machining choices. Different elevations produce separate operation
groups until the strategy supports varying per-feature heights.

Automatic “select similar holes” is a later convenience over a reliable manual
selection flow. Preview the proposed group before assigning it.

## File-format scope

| Format | Delivery | Meaning |
|---|---|---|
| STEP `.step`, `.stp` | First end-to-end slice | B-rep solids/surfaces; preserve exact curves/surfaces, topology, body identity and component placements. Qualify AP203/AP214 and selected AP242 fixtures; do not claim the whole standard. |
| IGES `.iges`, `.igs` | Next increment | Same selection pipeline where topology is valid. Surface-only or unsewn models remain inspectable and report machining limitations. |
| DXF / DWG | Preserve existing support | Existing planar contour workflow; adapt to common asset/reference infrastructure without requiring a solid kernel. |
| STL / OBJ | Later mesh import | Display/reference geometry first, with unit handling. Facets do not automatically become exact CAD faces, circles or holes. Mesh machining and inferred feature fitting require separate qualification. |
| Native CAD / Parasolid / ACIS | Deferred | Format-specific adapters and dependency evaluation; not implicitly included in “similar files.” |

Assemblies may be imported and inspected in the STEP slice, preserving repeated
instances and transforms. Machining initially targets one explicitly selected
body instance. Other visible bodies are not automatically fixtures or verified
collision obstacles. External component dependencies must be resolved and
embedded, or reported as missing.

## Current code and changes required

| Current implementation | Change |
|---|---|
| `cam-import::Import` contains regions, open chains and warnings | Add a separate solid-import result and format dispatch; retain the drawing API. |
| `AppController::import_cad` calls `install_import`, replacing geometry and document history | Introduce transactional add/replace asset commands and background import jobs. |
| `LoopRef` uses vector indices; operation construction copies contours | Persist entity references and derivation recipes, with resolved geometry supplied to strategies. |
| GUI picks an XY point on the stock-top plane | Add camera rays, depth-aware body/face/edge selection and semantic highlights. |
| `cam-render` has line scenes and a solid-stock mesh renderer | Extend mesh rendering with model instances, face identity and edge overlays. Reuse existing GPU infrastructure. |
| `.ocam` is self-contained JSON; geometry lives outside `History<Document>` | Add a versioned asset store and put asset bindings and manufacturing selections under undoable project state. |
| `cam-geo` is planar; no `cam-kernel` crate exists | Introduce the CAD backend behind a small boundary; preserve kernel-independent planar strategies. |

This is a focused subset of M1/M3 in [the broader implementation plan](implementation-plan.md).
It does not require waiting for ATC, programmable posts, a plugin ABI or full
multi-setup machining. The older architecture's proposed truck kernel is an
aspiration, not an existing dependency that this feature must preserve.

## Geometry backend decision

**Recommend evaluating OCCT first**, behind a narrow Rust-facing adapter. Its
STEP/XDE interfaces explicitly cover shape topology, assembly structure and
associated names/colours; its IGES translator provides a related import path.
These capabilities fit the requested workflow. See the official
[STEP guide](https://dev.opencascade.org/doc/overview/html/occt_user_guides__step.html),
[XDE guide](https://dev.opencascade.org/doc/overview/html/occt_user_guides__xde.html)
and [IGES guide](https://dev.opencascade.org/doc/overview/html/occt_user_guides__iges.html).

Keep [truck](https://github.com/ricosjp/truck) and
[truck-stepio](https://github.com/ricosjp/truck/tree/master/truck-stepio) as the
Rust-native comparison in a bounded prototype. Decide from the same fixture
corpus, checking import completeness, trimmed faces, analytic cylinders, units,
assembly placements, tessellation ownership and packaged builds. Documentation
alone does not establish reliability for the application's parts.

If OCCT passes, pin a tested release, use a small owned C++ bridge with exception
translation and explicit handle lifetimes, and keep CAD calls in a dedicated
service. Do not promise concurrent access to arbitrary backend objects. Verify
licence policy, notices and native dependency packaging on Linux, Windows and
Apple silicon before committing to the backend. Default headless planar builds
must remain available without the native CAD dependency.

## Architecture and contracts

```mermaid
flowchart LR
    Files[STEP / IGES] --> Import[cam-import dispatch]
    Import --> Kernel[cam-kernel: CAD adapter]
    Kernel --> Assets[Geometry assets and topology]
    Assets --> Display[Tessellation + entity mapping]
    Display --> Pick[Viewport selection]
    Pick --> Binding[Operation geometry binding]
    Assets --> Resolve[Feature resolver]
    Binding --> Resolve
    Resolve --> Input[Validated contours / holes / heights]
    Input --> Existing[Existing strategies and posts]
```

- **`cam-model`:** plain serializable IDs, asset manifests, instance placement,
  operation geometry bindings and revision state. No CAD handles or GPU types.
- **`cam-kernel`:** actual backend ownership, file translation, topology queries,
  analytic surface/curve access, validation, bounded healing and tessellation.
- **`cam-geometry3d`:** immutable asset views, topology index, spatial index and
  feature resolution. Use the boundary already proposed in M3; keep its early
  implementation narrow, without a general modeling or boolean API.
- **`cam-app`:** import jobs, asset transactions, selection service, operation
  inspector, persistence and stale-result rejection. Put new services in modules
  rather than extending the controller and GUI monoliths indefinitely.
- **`cam-render`:** model mesh instances and selection overlays; semantic IDs
  map triangles to faces and displayed curves to edges. CPU ray/BVH picking is
  the initial implementation; add GPU ID picking only if profiling warrants it.
- **`cam-toolpath`:** consumes resolved inputs; no CAD traversal inside strategies.

Proposed records (names illustrative):

```text
GeometryAsset: AssetId, revision, source hash, source units,
               import options, backend version, topology manifest, payload
GeometryInstance: InstanceId, asset/body reference, rigid placement
EntityRef: AssetId, asset revision, InstanceId, BodyId, entity kind, EntityId
GeometryBinding: selected EntityRefs, selection roles, extraction recipe,
                 linked values, explicit overrides, resolution status
ResolvedInput: planar regions / hole groups, cutting heights,
               dependency fingerprint, diagnostics
```

Hover/temporary selection belongs to view state. Confirmed selections belong to
the operation and participate in undo, persistence and generation. Legacy
operations retain their embedded contours as explicit geometry; migration must
not guess which imported entities they came from. New linked operations generate
from a matching resolved snapshot, never a stale cached contour fallback.

### Coordinates, heights and tolerances

Normalize input units to millimetres once; retain the source units and conversion
in provenance. Unknown units require a choice with a dimension preview. Preserve
asset-local coordinates; compose component and user placement transforms into
the fixed machining frame. Use f64 for geometry and transforms, f32 only for
render buffers, with camera-relative rendering if required for distant geometry.

The current `PocketOp` and `ProfileOp` use `floor = -depth`, while Drill uses
`top_of_stock + start_offset - depth` and Face uses `start_offset - depth`.
**Do not map a selected face's Z directly into these differing depth fields.**
Introduce a shared explicit top/bottom height contract for the first four
adapters and migrate old parameters preserving their exact prior world-space
cutting planes. Review flute/reach guards, descent, linking and simulation
against the actual cutting interval. A translated model with a floor above
world Z=0 must produce the same physical cut as the equivalent model at the origin.
Continue applying the existing work-datum translation exactly once at posting.

The initial placement aligns the part into the existing machining axes; it does
not reinterpret extra work origins as rotational setups. Multi-orientation work
requires the broader setup/frame migration.

Separate import/healing tolerance, display tessellation tolerance, feature
recognition tolerance and machining approximation tolerance. Generate machining
contours from exact curves under a declared error budget, never from a coarse
display mesh. Include planar-grid quantization and downstream arc fitting in that
budget. Changing display quality must not change a toolpath.

### Persistence, replacement and background jobs

Use IDs stable within an immutable asset revision. A face number or mesh triangle
index is not a durable identity across a new CAD export. Preserve instance paths
to distinguish repeated copies of the same part.

Store imported source data, a versioned topology-preserving snapshot and the ID
manifest in a self-contained project package, with regenerable render caches.
Continue opening existing JSON `.ocam` files through format detection and schema
migration. Measure package size and load time in the prototype before freezing
the layout. A backend upgrade that cannot preserve identity triggers explicit
re-resolution; it must not silently attach operations to different faces.

Model replacement creates a new revision, invalidates dependent results and
offers reattachment suggestions using source metadata, geometry and adjacency.
For the initial release, require review of replacement mappings. Missing, split,
merged or ambiguous entities show **Needs reselection**. Preserve manual overrides
but flag conflicts with changed geometry. Regenerate before export; an unresolved
included operation blocks generation/export under the existing completion gate.

Import stages are read → translate → validate/heal → index → tessellate → preview
→ commit. Report stage progress and structured per-body diagnostics. Bound repair
tolerances and record changes; partial imports must list skipped bodies. Reject
invalid solids for feature derivation while allowing clearly marked inspection.
Use request IDs/revision checks so an obsolete job cannot commit after a new edit.
Support cooperative cancellation; if the backend cannot interrupt a long call,
discard its result and keep the UI responsive. Evaluate a helper process if hard
cancellation or fault isolation proves necessary.

Asset blobs are immutable and shared across history entries. Asset removal,
replacement, placement and operation bindings must undo together. Save via a
validated temporary package and atomic replacement; copied projects must reopen
without the original STEP file or original folder path.

## Delivery sequence and acceptance gates

| Phase | Deliverable | Acceptance gate |
|---|---|---|
| 0 — Backend prototype | OCCT and truck fixture comparison; native packaging spike; measured import/tessellation/memory costs | Exact topology, dimensions, units and placements verified on the corpus; one deployable backend chosen with written limitations. Set performance budgets from representative parts. |
| 1 — Import and inspect | Assets, STEP import, placement, shaded geometry, tree, face/edge/loop selection, project round-trip | Select the correct face on a rotated model and the correct repeated instance; cancel without losing work; reopen after removing the source file. |
| 2 — First machining slice | Persistent binding, shared cutting heights, pocket-floor resolver and existing Pocket strategy integration | STEP pocket with island → selected floor → reviewed boundary and depth → toolpath → existing simulation/export; reopen with identical binding and output. Reject unsupported pocket geometry. |
| 3 — Core operation coverage | Drill groups, profiles and facing; editing linked geometry; undo/invalidation | End-to-end plate example works; holes at different levels are separated; manual overrides survive edits; stale or unresolved operations cannot export. |
| 4 — Broader input and updates | IGES qualification, replacement/reselection UI, larger-part tuning | Invalid/surface-only inputs report limits; changed/split/deleted faces cannot silently retain the old cut; packaged builds pass on all supported desktop platforms. |
| Later | Chamfer/thread/engrave/carve bindings, similar-feature selection, STL/OBJ, freeform surfacing | Independent capability gates; same asset and binding infrastructure. |

Phase 2 is the first useful release candidate, not merely a file viewer. Keep
one machining body and one orientation for this milestone. Estimate implementation
effort after phase 0 exposes native-build and fixture failure costs.

## Verification and release criteria

Maintain checked-in, redistribution-safe fixtures with known dimensions and
expected topology. Include planar and curved parts, a pocket with island, blind
and through bores, split cylindrical faces, multiple bodies, repeated instances,
inch/mm versions, translated/rotated parts, trimmed spline surfaces, open shells
and malformed files. Record fixture provenance and permitted use.

- **Import:** dimensions, solid validity, body counts, transform composition,
  unit conversion, missing assembly components and partial-success diagnostics.
- **Picking:** front-most visible entity, occlusion, filter behaviour, tiny edges,
  cylinder seams, repeated instances and identity across tessellation changes.
- **Feature resolution:** topology-derived boundaries, protected material,
  nonplanar/undercut rejection, analytic hole sizes and consistent Z calculations.
- **Associativity:** save/reopen, undo/redo, model replacement, ambiguous mapping,
  override conflicts, schema migration and backend-cache invalidation.
- **Machining:** compare imported-feature results with independently constructed
  equivalent 2D fixtures; inspect actual cutting coordinates and existing post
  semantics, not just screenshots. Display tolerance changes must leave NC intact.
- **Application:** responsive imports, cancel/discard, stale job rejection,
  interrupted saves, missing native backend and packaged desktop smoke tests.

The existing heightfield simulation is not general B-rep collision verification.
Feature eligibility checks must reject geometry outside the qualified 2.5D
subset, and the UI must not imply that displaying an imported solid adds fixture,
holder or arbitrary overhang checks. Full 3D verification remains the broader
plan's M5/M6 work.

Definition of done: a user can import a supported STEP part, select supported
features, create linked operations, review derived parameters, generate and
inspect the cut, save/reopen, and explicitly repair links after model replacement
without silently changing units, cutting heights or machining targets.
