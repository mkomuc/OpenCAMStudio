# Setup operation patterns — feature plan

Status: implemented with partial qualification, 2026-10-18. Source baseline: `9633e29`, document schema 22.
Runtime behavior and verification limits are recorded in [operation-patterns-implementation.md](operation-patterns-implementation.md). This plan remains the broader acceptance specification; unqualified items are not claimed as complete.

## Goal and scope

Repeat one operation or an ordered group of operations inside one physical setup
using a rectangular or circular pattern. Generate each seed operation once and
transform its toolpath for the instances. Seed edits update every instance;
duplicated CAD bodies and independent operation copies are unnecessary.

The supplied [Fusion CAM pattern discussion](https://forums.autodesk.com/t5/fusion-manufacture-forum/fusion-360-cam-pattern/td-p/8729045)
illustrates creating patterns through an operation context menu and reusing paths
through translation or rotation. It also discusses nested and component patterns.
This first increment covers translation in setup XY and rotation about setup Z.
Nesting, mirrors, component detection, arbitrary axes, individual instance
suppression and rotary-axis machining are deferred. Circular patterns rotate
cutting coordinates without changing the tool axis, physical setup or work datum.

## Workflow and context menus

1. Right-click a setup and choose **New pattern → Rectangular… / Circular…**.
   Open a right-column draft with an ordered source-operation checklist. Preselect
   eligible operations already selected in that setup; otherwise start empty.
   Require at least one source before OK.
2. Offer **Add to new pattern → Rectangular… / Circular…** on operation rows too.
   Right-clicking an already selected operation preserves the selected group;
   right-clicking an unselected operation selects only it. Members must belong to
   the same setup and work datum and be outside existing patterns. Mixed or
   ineligible selections show a reason instead of silently dropping operations.
3. Show the name, ordered sources, parameters, execution mode, instance count
   including the original, and total operation executions in the inspector.
   Preview transformed paths and distinguish the original. If no current seed
   path exists, show placement markers clearly labeled as such until generation.
   Draft previews never enable export.
4. **OK** validates and commits settings and grouping as one undo step. **Cancel**
   discards the draft. Dirty setup/operation/pattern drafts must be resolved before
   changing selection, restructuring the browser or generating committed paths.
5. Display an expandable pattern beneath its setup and work-datum group, with
   original editable operations as children. Derived instances do not become
   document operations. Selecting a group highlights all its instances; selecting
   a child highlights that source's instances.

Creation inserts the group at the earliest selected operation. For noncontiguous
selections, collect members there in their current order, preserving the relative
order of unselected operations. Show the resulting sequence before OK. Never sort
members by ID or tool number.

Pattern context actions: Edit, Recompute, Simulate, Post process and export,
Suppress/Include, Dissolve pattern, and Delete pattern and operations. Dissolve
retains the original operations at the group position without repetition. The
delete action explicitly names its effect on sources. Children retain operation
editing and suppression. Visibility remains independent of manufacturing inclusion.

## Parameters and execution semantics

Every pattern includes exactly one identity instance, the original. Counts include
the original. Default to two instances and display a placement preview; require
valid active spacing/angle values before OK.

| Type | Fields | Transform in setup coordinates |
| --- | --- | --- |
| Rectangular | X/Y counts `Nx, Ny`, signed X/Y pitch `dx, dy` in mm | `T(i,j)(x,y,z) = (x+i*dx, y+j*dy, z)` |
| Circular | Center `(cx,cy)` in mm, count `N`, full circle or partial sweep, sweep angle in degrees, CW/CCW | `T(k)(p) = c + Rz(k*delta)(p-c)`, Z unchanged |

Rectangular counts are integers >= 1 and `Nx*Ny >= 2`. An axis with count > 1
requires finite nonzero pitch; negative pitch reverses that axis. An inactive
axis has effective pitch zero. Iterate Y rows, then X columns: `(0,0), (1,0), …,
(0,1), …`. The original's position is the anchor. Centering, staggering,
arbitrary direction picking and serpentine ordering are deferred.

Circular count is an integer >= 2. Default the center to the active work datum's
XY position expressed in setup coordinates; allow numeric entry and picking a
setup-relative point. The axis passes through this center parallel to setup Z.
CCW is positive viewed from +Z toward the XY plane. For a full circle use
`delta = sign*360°/N`, avoiding a repeated identity at 360°. For a partial sweep
`0 < A < 360°`, use `delta = sign*A/(N-1)`, including both endpoints. Rotate the
complete source footprint and orientation; translate-only circular copies are
outside this increment.

Offer two explicit execution modes, preserving member order:

- **By instance** (default): `A0, B0, A1, B1, …` for sources A and B.
- **By operation**: `A0, A1, …, B0, B1, …`.

Different tools, RPM, feeds and heights are allowed. The mode controls actual
simulation and NC ordering, including multi-tool Carve sources. Do not introduce
automatic tool sorting.

Validate finite parameters and transformed coordinates and use checked arithmetic
for expansion. Initially cap a pattern at 10,000 instances and a run at 5,000,000
expanded steps, checked before allocation. These proposed resource limits need
qualification. Warn about coincident/overlapping footprints without silently
deduplicating them. Expanded motion must pass existing machine, motion and stock
verification/export gates.

## Model, persistence and commands

Keep the eight cutting `Operation` variants unchanged. A pattern is a setup-level
container referencing source operation IDs, not a ninth cutting strategy.

```text
Setup.patterns: Vec<OperationPattern>
OperationPattern {
    id: u64, name: String, members: Vec<u32>,
    kind: Rectangular { nx, ny, dx, dy }
        | Circular { center_xy, count, distribution, direction },
    order: ByInstance | ByOperation,
    suppressed: bool
}
Document.pattern_id_floor: u64
```

Allocate globally unique pattern IDs with the existing high-water-mark convention.
Save membership, name, parameters, mode and suppression; keep drafts, selection,
expansion and visibility in GUI state. Instance identity is derived as
`(setup_id, pattern_id, instance_index, source_operation_id)`, without allocating
new operation IDs, bindings, height records or preset snapshots.

`Setup.operations` remains the canonical flattened source order. Group members
form a contiguous block in that vector in exactly the order stored in `members`.
Add a shared `execution_items()` view: yield a pattern at its first member, skip
its remaining members at the top level, and yield ungrouped operations once.
The browser and planner consume this view inside the existing outer work-datum
ordering. All group members share a `work_offset`. Emit seeds only inside their
groups, including the identity instance, to avoid double cutting the original.

Validate nonempty membership, unique IDs, existing same-setup sources, matching
member order, contiguous blocks, disjoint membership and a common datum, both
for commands and project loading. Nested patterns are invalid in this schema.
Pattern edits that add/remove members use the same ordering rules as creation;
removed members remain ordinary operations immediately after the group in their
previous relative order. Reject a membership edit that would leave no sources.

Reorder a pattern as one block within its datum; reorder children within the
group by updating both representations atomically. Reject drag/drop that splits
a group; membership editing belongs in the pattern inspector. Deleting a source
updates membership atomically; deleting the last source removes the group.
Moving a member to another datum/setup requires dissolving the group first.
Cross-setup pattern transfer is deferred. Duplicating an entire setup remaps
pattern/operation IDs, membership, height/link records and suppression together.

Use schema 23 if 22 remains current at implementation; otherwise use the next
unused version. Add a versioned migration with empty pattern collections and an
allocation floor. Preserve existing source order, origin grouping, coordinates
and NC output. Older readers must reject the new version rather than silently
omit repetition. Cover round-trip and malformed input through the normal loader.

## Planning, transforms and verification

The existing planner computes `Planned` fragments and assembles them through
`emit_instance`. Extend this seam to repeat source fragments before job-level
linking, rather than duplicating a completed program.

1. Validate groups against the complete committed document. Resolve included CAD
   sources and parametric heights in setup-local coordinates. Keep original
   geometry bindings and source topology IDs.
2. Compute/cache each included seed fragment once. Transform the final fragment
   after height/feed-plane adjustments and before global transition assembly.
   Retain exact seed cache dependencies; pattern parameters change assembly,
   simulation and preview without invalidating unchanged seed fragments.
3. Expand `execution_items()` in the selected mode and instance order. Pattern
   suppression skips all members/instances; member suppression skips that source
   in every instance; disabled datums skip their entire groups. Intentionally
   all-suppressed groups follow the existing empty-job rules.
4. Add a typed XY rigid transform in `cam-cldata`: transform Rapid/Linear
   destinations, Arc endpoints and absolute centers, and all Drill cycle points.
   Preserve Z, retract/bottom/feed planes, helix pitch, arc handedness, feed,
   dwell and cutter-compensation side. Handle every `Step` variant explicitly;
   never rewrite NC strings. Preserve internal multi-tool control steps.
5. Rebuild approach/retract, boundary travel, tool changes, spindle/coolant and
   shutdown through assembly. Before crossing XY, lift vertically to at least
   both adjacent operations' clearance planes, then descend at the destination.
   Re-establish a known tool-change approach when needed. Copies share one datum
   and introduce no re-fixturing stop. Do not replicate job preambles, datum
   changes or final shutdowns.
6. Validate expanded neutral motion, simulate every copy against the same setup
   stock in execution order, prepare backplots and post through existing gates.
   Render setup coordinates in world space as today and subtract the work datum
   once at posting. Controller rotation commands/subprograms are unnecessary.

Seed reuse repeats its computed footprint. Audit `JobEnv.stock` dependencies,
especially Face's stock-sized sweeps and stock-clipped strategies. Display the
actual seed footprint and repeated extents: a stock-wide Face sweep repeats that
whole sweep, not an inferred part boundary. Do not independently reclip copies
while claiming exact seed reuse. Reject source modes that require destination-
specific regeneration or predecessor stock until separately qualified. This
feature does not add rest-machining or fixture/holder verification.

Extend motion `Tag` and diagnostic `SourceRef` with optional pattern/instance
ownership while retaining the original operation ID. Carry this through cycle
expansion, transitions, simulation collisions, scene strips, output selection
and post validation. Existing nonpattern tags have absent ownership. An instance
diagnostic selects its group/member and identifies the instance, without creating
another editable operation.

## Integration and invalidation

Paths below are relative to `crates/`.

| Code | Planned work |
| --- | --- |
| `cam-model/src/document.rs`, `migrate.rs`, `validate.rs`, `history.rs` | Records, identity allocation, structural/order validation, migration and undo |
| `cam-app/src/commands.rs`, `setups.rs` | Atomic pattern lifecycle/reordering and setup duplication |
| `cam-app/src/gui_project_tree.rs`, `gui.rs` | `TreeNode::Pattern`, context actions, preserve `focus_ops` on right-click, group rendering |
| New `cam-app/src/gui_pattern_editor.rs` | Transactional draft, source/order previews, numeric validation and center picker |
| `cam-app/src/job_controller.rs`, `jobs.rs`, `gui_jobs.rs` | Run scope, immutable inputs, progress, cancellation and publication |
| `cam-toolpath/src/plan.rs`, `cache.rs`, `lib.rs` | Expansion, seed reuse, transitions and instance diagnostics |
| `cam-cldata`, `cam-render`, `cam-sim`, `cam-post` | Rigid transforms, provenance and verification of expanded motion |

Audit every direct mutation/iteration of `Setup.operations`: creation, duplication,
reinitialization, deletion, transfers, origin changes, tool reconciliation,
duplicate warnings, CAD resolution, snapshot filtering and CLI generation. Share
group-aware helpers. Intentional derived copies must not trigger duplicate-source
warnings.

Specifically, `job_inputs` and `start_toolpath_run` currently filter the source
operation vector. Introduce immutable run scope (Setup, Pattern or Operation),
resolving inclusion through the validated execution view instead of leaving
dangling membership in partially filtered documents. A member run includes that
member at every pattern position; a pattern run includes its included members;
a setup run includes all groups and ungrouped operations. Retain scope in result
provenance so partial results remain clearly identified.

Pattern edits and undo/redo increment the revision, cancel obsolete workers and
clear assembled simulation/NC. Include definitions, suppression, order and scope
in the job fingerprint. A seed edit invalidates its fragment and every dependent
instance. Pitch/count/center/order edits reuse matching seed fragments but rebuild
all expanded output and verification. Preserve request ID/revision/fingerprint
publication checks. Check cancellation during expansion as well as generation;
report seed generation and instance assembly progress separately.

## Delivery and acceptance gates

1. **Model and commands:** records, migration, shared execution view and atomic
   commands. Prove save/reopen, undo/redo, noncontiguous grouping, suppression,
   identity allocation after undo branches, membership edits/deletion, setup
   duplication and malformed-input rejection. Pattern-free migrated fixtures
   retain byte-identical NC output.
2. **Headless generation:** both types and order modes, using asymmetric sources
   with arcs/helices and drill cycles. Independently check that `(3,2)` rotated
   90° about `(1,1)` becomes `(0,3)`; a 3-by-2 grid with pitches `(20,-15)` has
   six instances including one identity; four full-circle instances use
   0/90/180/270°; three instances over a 90° partial sweep use 0/45/90°.
3. **Pipeline qualification:** all eight operation families, open/closed profiles,
   stock-derived footprints, mixed tools/RPM/heights, two-tool Carve, drill
   lowering, compensation and every supported post. Instrument computation to
   prove one generation per included seed, independent of instance count. Check
   actual NC coordinates with a nonzero datum and rotated setup, cycle end
   positions, swept arc bounds, boundary travel and stock removal at every copy.
   An invalid/colliding later instance blocks export; cancellation or resource
   exhaustion never publishes a complete job.
4. **Browser and inspector:** both context entry points, preserved multi-selection,
   source checklist, placement/order previews, dirty-draft guards, OK/Cancel,
   group/child selection, visibility versus suppression, scoped runs and
   dissolve/delete. Desktop smoke tests cover one/multiple DXF and linked STEP
   operations; message-loop tests alone do not qualify picking/visual previews.
5. **Regression and evidence:** stale-worker rejection after edits, scope changes,
   setup switches and undo. Run workspace tests, GUI tests and Clippy using the
   repository's current commands, plus native STEP checks in the configured OCCT
   environment. Record actual results and limits in an implementation note.
   Change proposed status only after qualification; this planning commit does
   not claim those tests were run or that patterns are shipped.

Build on the existing M1/M1a/M1b and multi-setup services. This feature does not
require the plugin ABI, programmable posts or rotary-axis milestones. Existing
export qualification requirements continue to apply.
