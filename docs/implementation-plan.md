# Implementation plan: machining workflow and qualified milling

Updated: **2026-10-14** after reviewing [high-level functions](high-level-functions.md).
The architectural review baseline remains **2026-10-13** at
[`5c510b504fe39f4e40700ff3b1e331d0309fcad3`](https://github.com/pr0m1th3as/OpenCAMStudio/tree/5c510b504fe39f4e40700ff3b1e331d0309fcad3).
Read the [requirements analysis and backend parameter inventory](high-level-requirements-analysis.md),
[architecture assessment](architecture-analysis.md) and [validation evidence](architecture-validation.md).
Work below is proposed except the explicitly reported completed packages and reusable components.
Historical test counts below are evidence from those packages, not a new test run.

**Progress on the fork (2026-10-13):** the four clearing regressions have been diagnosed and fixed on `codex/fix-adaptive-clearing`; the headless suite reports 677 passed and no failures, and clippy passes with warnings denied. See the [fix report](adaptive-clearing-fix.md).

The completed package on `codex/block-incomplete-job-export` addresses A01: explicit
job completion now propagates through planning, controller and GUI, and cancelled,
incomplete or failed runs cannot be exported. Validation: 688 passing headless
tests (23 ignored), 183 passing GUI-feature tests and both clippy configurations.
See [completion-aware export](job-completion-export.md).

The completed package on `codex/validate-motion-and-arc-bounds` corrects A02 and
malformed-motion A04: controller-neutral IR/machine validation, rounded NC
preflight and analytic full/partial/helical arc bounds. Validation: 707 passing
headless tests (23 ignored), 184 passing GUI-feature tests and both clippy
configurations. See [motion validation and arc bounds](motion-validation-and-arc-bounds.md).
Continue with shared drill-cycle traversal and verification (A03), then controller
tool-change/TLO policy. Tool/register reference resolution and broader document
input validation remain open parts of M0 item 2.

**Reusable workflow baseline (reviewed 2026-10-14):** schema 15 already provides
ordered independent setups, model-body selection, model/edge frames, fixed and
relative box stock, shared setup drafts, STEP picking and four qualified linked
operations, and separate setup NC output. See [CAD import](cad-import-implementation.md)
and [multi-setup implementation](multi-setup-implementation.md). These satisfy
parts of M1/M3, not their entire acceptance gates. Fixture roles/checks,
material-linked presets, setup installation binding and previous-setup stock
remain open. The current library/preview supports eight cutter kinds.

## 1. Product scope and sequencing

Target the first qualified release at **fixed-axis three-axis milling**, including multi-tool/ATC jobs on configured installations, programmable posts, CAD/mesh surface machining, fixtures and holder collision checks, and independently checked exported NC. Start with a small declared controller/machine matrix. Do not claim every controller or arbitrary macro is supported.

The existing 2.5D workflow must continue to work during development. Preserve built-in strategies, integer planar geometry, headless tests, the neutral IR and existing post-family implementations. Use schema migrations and adapters instead of a new application rewrite.

Prioritize demonstrable end-to-end slices. Full 5-axis, turning and mill-turn follow separate gates. Native third-party plugins are deferred until the model is stable; user-programmable post definitions can be delivered earlier through a controlled runtime.

The next product increments make **Machining the default workspace**, with a
full main viewport, dimensioned XY grid and left project-tree overlay. Rows expose
visibility/status; commands live in context menus and execution order changes
through drag/drop. Setup, tool and operation forms stay in the right inspector.
Tools replaces the main model view with the library and reuses the existing tool
visualization. Stock, model bounds, snapping and WCS overlays are now required in
Machining; this supersedes the earlier model-only center in the setup UI draft.

Keep the existing eight cutter kinds. Adapt the richer
[tool-library reference](tool-library-implementation-reference.md) to those types,
local/document libraries and manufacturer Fusion library import. New cutter
families and cloud providers are later scope. Operation tabs expose supported
backend parameters; copying every visible Fusion control is outside this increment.

Milestone dependencies:

```mermaid
flowchart TD
    M0[M0: export correctness and green baseline] --> M1[M1: domain contracts and background jobs]
    M1 --> M2[M2: assemblies and ATC installation policy]
    M1 --> M3[M3: setups, fixtures and geometry assets]
    M2 --> M4[M4: programmable posts and NC interpretation]
    M3 --> M5[M5: fixture and holder verification]
    M2 --> M5
    M4 --> M5
    M3 --> M6[M6: fixed-axis 3D machining and rest stock]
    M5 --> M7[M7: volume stock and machine simulation]
    M6 --> M7
    M4 --> M8[M8: qualified milling release]
    M5 --> M8
    M6 --> M8
    M7 --> M8
    M8 --> M9[M9: indexed machining]
    M9 --> M10[M10: simultaneous axes and further processes]
```

The diagram retains the industrial integration milestones. The following smaller
gates add the requested workflow without renumbering M0–M10 or replacing the
repository's historical P0–P8 phases:

| Gate | Deliverable | Dependencies | Requirement coverage |
| --- | --- | --- | --- |
| M1a | Machining/Tools workspace and overlay tree | Existing commands; relevant M1 command/history contracts | H01/H02/H13 |
| M3a | Complete fixed-axis setup editor, placement, WCS picking and setup machine/post binding | M1a; existing M3 frames/assets; M2a for material preset integration | H03/H04/H06–H11 |
| M1b | Backend-driven operation tabs and both selection workflows | M1a; M3a frame/picker contracts; M2a for preset selector integration | H15/H16 |
| M2a | Eight-type tool aggregate, inspector tabs, material presets and assembly preview | M1a; M1 identity/draft contracts; M2 length/assembly contracts | H10/H13 |
| M2b | Manufacturer Fusion library import | M2a storage/type/quantity contracts | H14 |
| M3b | Persistent fixture body assignments | M3a; M3 asset/transform contracts; checking completes at M5 | H06/H12 |
| M7a | Verified stock transfer from previous setup | M3a; M1 dependency/invalidation contracts; M0 motion traversal; stock snapshots/backend capability | H05 |

H identifiers map to the [requirements analysis](high-level-requirements-analysis.md).
M1a starts with the reusable controller; it does not wait for every industrial
M1 service. Likewise, M2a catalog editing does not wait for full ATC qualification.
Independent UI/schema work can proceed while remaining M0 fixes are completed;
M0 remains mandatory for release/export qualification. M7a is extracted from the
stock portion of M6/M7 and does not depend on full-machine simulation.
These sub-gates do not declare the parent milestone complete.

Recommended delivery order: **M1a → M3a core → M1b core**, alongside
**M2a → M2b**; then integrate material/preset selectors into M3a/M1b and deliver
**M3b → M5**. Define M7a snapshots early, then qualify its stock backend before
enabling previous-setup stock. Estimates follow measured capacity, stock/collision
spikes and the actual qualification machine matrix.

## 2. Target module boundaries

Create a new crate when it establishes a real independently testable boundary, not for every type below. Initial module extractions can remain in existing crates.

| Boundary | Responsibility | Suggested placement |
|---|---|---|
| Domain project | IDs/revisions, geometry references, setups, operations, assemblies, stock definitions, manufacturing suppression | Evolve `cam-model`; split submodules first |
| Geometry assets and queries | Mesh assets/BVH, face/edge selection, cutter-contact queries; optional CAD handles behind an adapter | New `cam-geometry3d`; add `cam-kernel` only for actual CAD backend |
| Tooling | Cutter/holder/assembly definition, envelope generation, materials/cutting defaults | `cam-model::tooling` initially; library persistence adapter outside domain |
| Installation/kinematics | Machine definition, joint/link tree, limits, transforms, tool assignment and controller/post binding | New `cam-machine` when execution needs it |
| Planning and linking | Strategy dispatch, operation dependencies, tool-use scheduling, transition generation | Extract `cam-plan` from `cam-toolpath::plan` when M2 lands |
| Neutral program | Controller-independent cutter poses, curves and process intents with source IDs | Evolve `cam-cldata` in versioned steps |
| Canonical execution | Cycle expansion, state transitions, motion traversal, interpolation and bounded approximation | New `cam-motion`; no GUI dependency |
| Posting | Profile validation, post events/scripts, central writer, source mapping, lowered output | Evolve `cam-post`; keep compiled first-party emitters |
| NC interpretation | Supported-controller subset → independently reconstructed execution; unknown-state diagnostics | New `cam-nc` |
| Stock removal | Heightfield and volume-capable stock backends; residual/gouge comparison and stock snapshots | Evolve `cam-sim` |
| Collision verification | Swept assembly/fixture/machine-body checks, policies, distance/coverage reporting | New `cam-verify` when M5 lands |
| Rendering | Scene instances, meshes/LODs, playback presentation, picking/highlighting | Evolve `cam-render`; consume verification data |
| Application services | Job scheduler, document commands, file/config adapters, cache/invalidation, export gate | Modules/services beneath `cam-app`; GUI remains presentation |

Avoid a dependency cycle between the domain and post system: installation profiles can bind IDs/configuration in the composition layer; the domain should not serialize a Rust `PostKind` as the only possible future post identity. Keep CAD backend and GPU types out of strategy/domain/verification interfaces.

### 2.1 Essential data contracts

The names below are illustrative design sketches, not APIs promised to downstream consumers.

```text
Project
  schema_version, project_id, revision
  geometry_assets[]: stable asset ID + revision + source units + content hash
  cutter_definitions[], holder_definitions[], assembly_snapshots[]
  setups[]: SetupId, rigid transforms, WCS origin/axis references,
            target body instances, stock/placement/material, fixtures,
            installation binding, ordered operations
  manufacturing suppressions, operation dependencies, intended output targets
  provenance: authoring machine/post/config fingerprints

MachineInstallation
  MachineDefinition: joints, links, frames, limits, machine collision assets
  ControllerProfile: firmware/options, post ID/version/config, supported modes
  ToolAssignments: AssemblyId -> T number, H/D registers, pocket, measured offsets
  Placement: setup-to-machine transforms and initial state, or explicit unknown
  ToolChangePolicy: manual/ATC, approach sequence, pre/postconditions

JobRequest
  request_id, project_revision, immutable resolved inputs, cancellation, progress

JobResult
  completion: Complete | Cancelled | Failed
  per-operation artifacts, source-tagged program, diagnostics, input fingerprint

VerificationReport
  status: Pass | Fail | Incomplete | Unsupported
  coverage: bodies/motions/controller features checked + omissions
  accuracy: tolerances, stock resolution, conservative bounds
  events: source step/NC line, bodies, time interval, position, clearance
  fingerprints: project + installation + post + program + verification settings

ExportArtifact
  exact NC bytes + hash, source map, setup/tool sheets, report, provenance

ToolRecord
  stable ID/revision, metadata, one of the eight supported cutter kinds
  dimensioned geometry, shaft segments, holder/assembly snapshot
  named cutting presets + material associations + raw/resolved quantities
  catalog post metadata, independent of local installation assignments

OperationToolBinding
  resolved tool/preset revisions, explicit manual overrides and input fingerprint

PreviousSetupStock
  source SetupId + completed result fingerprint + remaining-stock snapshot
  source-to-destination transform, representation/accuracy, dependency edge
```

Use named IDs rather than interchangeable integers for tool, assembly, setup, body and operation identities. Revisions and content hashes prevent stale results from silently reusing changed assets. Distinguish nominal modeled geometry from measured machine offsets; do not overwrite controller measurements with library defaults.

## 3. Milestones and acceptance criteria

### M0 — Establish a trustworthy export baseline

**Dependencies:** none. **Addresses:** A01–A07 in the assessment. **Files:** `cam-cldata`, `cam-post::{lib,dialect,writer,grbl}`, `cam-sim::lib`, `cam-toolpath::{plan,clearing}`, `cam-app::controller`.

Deliver small focused changes:

1. Preserve `Complete/Cancelled/Failed` at strategy aggregation and controller level; reject incomplete runs from both in-memory and file export. Clear/reject stale outcomes when inputs change.
2. Add an IR validator for finite coordinates/feeds/RPM, positive rates, valid tool/datum references, consistent arc start/end radii and planes, legal dwell/cycle parameters, and valid machine/options. Validation should be headless and return source-tagged diagnostics.
3. Compute full swept arc bounds, including quadrant extrema, helices and full circles; retain the current travel-span precheck but label it accurately.
4. Introduce a canonical cycle-expansion/traversal service. Simulate approach, drilling/pecks, retracts and links, retaining final position. Replace unknown-tool fallback with a diagnostic/error for verification. Resolve controller compensation effects where modeled; otherwise mark those motions unsupported rather than implying they were fully checked.
5. Fix Grbl M6 declaration/lowering and specify manual/ATC/TLO policy for each currently supported profile. Do not assume that every Grbl descendant shares tool-change behavior.
6. Centralize transition state sufficiently to ensure both operation-boundary and internal Carve changes meet defined pre/postconditions. Require tested spindle/coolant/offset semantics; macro assumptions must be profile data. Invalidate post-writer position after M6 unless the installation guarantees its return pose, and explicitly restore a verified approach before descent.
7. Investigate the four failing clearing tests. Compare supported toolchain/platform behavior, actual strategy dispatch, certification rejection reasons and fallback diagnostics. Fix the cause or update genuinely obsolete expectations with documented evidence; do not simply suppress tests.
8. Add regressions for the four reproduced probes and controller-policy findings. Update architecture/roadmap claims about actual modules, synchronous jobs, saved provenance and post count.

**Acceptance gate:** the complete supported headless test suite is green; all four documented counterexamples are rejected/detected as intended; Grbl NC is accepted by its declared supported parser/profile; two-tool LinuxCNC and one industrial profile explicitly satisfy offset/change policy. Existing representative 2.5D geometry remains unchanged except intentional documented NC corrections. Invalid/cancelled jobs cannot obtain a passing export artifact.

**Demo:** import a drawing, run a two-tool job with appropriate change policy, verify, export; cancel the same job and demonstrate that export is unavailable.

### M1 — Domain contracts, services and asynchronous jobs

**Status:** parent domain/services/job package implemented 2026-10-14. See
[M1 implementation and acceptance evidence](m1-domain-services.md): schema-16
suppression and identity marks, bounded history/cache, immutable revision-stamped
workers, cancellation/progress and stale-result rejection. Validation: 778
headless tests, 227 GUI-feature tests, and both strict clippy configurations.
M1a/M1b core workflow implementation is recorded in
[M1a/M1b acceptance evidence](m1-workspaces-operation-tabs.md). DXF/native STEP desktop smoke checks pass; remaining M0 qualification is still
required for release.

**Dependencies:** M0. **Addresses:** A11, A14–A16; foundations for A09/A10/A12.

Extract controller services for document commands, geometry selection, job execution and export. Split GUI modules by coherent views and inspectors, reusing shared operation metadata to reduce repeated dispatch. Keep typed first-party operation parameters; a registry need not replace every exhaustive match immediately.

Introduce stable IDs/revisions and immutable asset handles. Define result completion and diagnostic codes, source references, cancellation and progress contracts. `Diagnostic.message` remains display text, not the only programmatic meaning. Use bounded/coalesced history and shared geometry assets; measure before replacing all snapshot history.

Move generation, verification and render preparation onto worker tasks. GUI submits an immutable `JobRequest`; edits increment revision and cancel superseded jobs. Accept a result only if request ID and input fingerprint still match. Add per-operation caching and explicit dependencies; any reordering, stock change, tool assembly change, setup transform, post policy or machine assignment must invalidate the appropriate artifacts.

**Acceptance gate:** a deliberately long job leaves UI interactions responsive; cancellation stops all involved services; an old job finishing after a new edit never replaces current geometry or enables export. Headless controller tests remain possible without a graphics stack. Undo/redo restores manufacturing state and invalidates exactly the affected results.

#### M1a — Machining and Tools workspace shell

**Status:** implemented 2026-10-14; automated acceptance and DXF/native STEP
workspace smoke checks pass. See [implementation evidence](m1-workspaces-operation-tabs.md).
**Dependencies:** as listed in the workflow gate table. **Addresses:** H01/H02/H13.

Start in Machining. Render the model/drawing across the main viewport, with a
dimensioned XY reference grid and a left overlay browser. Preserve navigation,
semantic selection and orientation cube; overlay hit testing must not trigger
model picking or camera movement underneath it. Keep forms in the right inspector.

Move tree commands (create/edit/duplicate/delete, inclusion/suppression, transfers
and related actions) to context menus. Apart from disclosure/selection, rows expose
only visibility and status. Implement setup/operation drag/drop through document
commands, preserving stable IDs, undo and execution order. Reject invalid targets
and, once stock dependencies exist, dependency-breaking reorders. A cross-setup
move must use the existing re-resolution/validation path.

Switching to Tools shows the library table/preset browser in the main area and
the existing generated tool visualization with editing in the right inspector.
Returning to Machining restores setup, camera and selection. Retain a coherent
OK/Cancel draft policy when switching contexts.

**Acceptance gate:** fresh launch opens Machining; DXF and STEP appear with labeled
grid units; overlay clicks never pick through. Context actions remain reachable;
dragging an operation changes actual planning order, persists and is undoable.
Visibility never changes machining inclusion. Tools occupies the main area and
returning restores machining context.

#### M1b — Operation tabs backed by supported parameters

**Status:** supported backend core implemented 2026-10-14; automated acceptance
passes. Material presets remain M2a integration. See
[implementation evidence](m1-workspaces-operation-tabs.md).
**Addresses:** H15/H16. **Dependencies:** workflow gate table.

Implement **Tool / Geometry / Heights / Passes / Linking** inspector tabs with
one transactional draft and persistent OK/Cancel. Use the
[backend inventory](high-level-requirements-analysis.md#backend-parameter-inventory-for-operation-inspectors)
to define fields, units, inheritance, validators and availability for each of the
eight operations. Omit the Multi-axis tab until M9/M10. Separate setup-level
clearance/retract from operation cutting heights, and derived CAD values from
manual overrides. Do not add unused per-operation height or feed controls.

Support select features → choose ribbon operation and choose ribbon operation →
pick features. Ctrl permits several features of the same kind for the active
geometry field; validate their combined topology and strategy eligibility. Retain
platform Command equivalents. Do not assume that selecting unrelated chains is
supported by a single-chain backend. Preserve the current STEP qualification
restrictions; unsupported bindings explain their rejection.

**Acceptance gate:** each displayed editable field round-trips to its real backend
parameter and affects the resolved job where applicable. A representative edit
for each operation family generates the expected semantic change. Tab switches
retain drafts; Cancel discards all tabs; OK is one undoable change. Both creation
orders yield equivalent inputs, incompatible/mixed selections receive a reason,
and unresolved geometry cannot generate/export. Integrate material presets after
M2a without overwriting manual feed/RPM overrides.

### M2 — Tool assemblies and installation-specific ATC

**Dependencies:** M1. **Addresses:** A06/A10, requested tool library + ATC.

Migrate `Tool.number` references to stable cutter/assembly references while retaining legacy import/export adapters. Separate library catalog definitions, immutable project snapshots and machine assignment. Existing geometry-based reconciliation can remain an explicit import-assistance operation; it must not establish physical assembly identity automatically.

Add holder/arbor/extension geometry, gauge-line and cutter-tip frames, stickout and measured assembly parameters. Support native parametric envelopes plus imported meshes with known units and validation. Add search/tags/catalog metadata and material-specific cutting data without turning machine pocket numbers into catalog identity.

Introduce fixed/random magazine assignment semantics, T/H/D register mapping, station capacity, missing/duplicate assignment validation, and preselection support where configured. Keep PLC/ATC handshake execution on the controller. Represent relevant change paths/envelopes and postconditions in the installation policy for verification.

Refactor compound operations to emit typed tool-use sections or requests that the scheduler expands into transitions. Keep strategy authority over cutting order; put machine-dependent change clearance, offsets and spindle/coolant sequencing in one service. Do not globally reorder by tool without honoring operation/stock/setup dependencies.

**Acceptance gate:** one cutter in two holders creates two distinct assemblies; renumbering/pocket reassignment does not change operation identity; absent assemblies and ambiguous offsets block export. Qualify manual, fixed-pocket ATC and random-pocket profiles as separate configurations. A clearing-tool → V-bit Carve job uses the same validated transition mechanism as adjacent single-tool operations.

**Demo:** a job reopens on another shop installation, reports provenance mismatch, binds assemblies to that installation and regenerates verified NC with its T/H/D/pocket mapping.

#### M2a — Eight-type tool library, assemblies and material presets

**Status:** proposed; the native library, cutter types and preview already exist.
**Addresses:** H10/H13. **Dependencies:** workflow gate table.

Add a versioned tool aggregate with metadata, supported typed cutter geometry,
ordered shaft segments, optional holder snapshot, named cutting presets,
material associations and catalog post settings. Define overall length, stickout,
shoulder and gauge references before migration; the current `Tool.length` is
stickout. Keep library records, immutable project inputs and installation T/H/D
assignments distinct. Migrate nominal cutting defaults into a default preset.

Place **General / Tool / Shaft / Holder / Cutting Data / Postprocessor** tabs in
the inspector. Reuse and extend the current visualization/geometry builder rather
than introducing another preview model. Add local/document scope, search/filter,
tool/preset selection and the reference's validation/Problems feedback. All tabs
share a draft; only a successful complete save announces success.

Use dimension-aware, bounded expressions and explicit driving values for derived
surface speed/chip load. Add stable material IDs and tool/material preset links.
When selecting a tool, prefer its matching stock-material preset and show the
fallback when none matches. Preserve operation overrides and snapshot revisions;
source changes require explicit application and invalidate affected results.

Store the reference's coolant choices and post metadata with capability-aware
application. The current planner's fixed flood coolant and shared operation RPM
require adapter work before promising independent motion feeds, coolant behavior
or clearing-tool RPM. Holder geometry feeds M5; preview alone is not verification.
Additional cutter kinds and cloud synchronization remain deferred.

**Acceptance gate:** all eight existing kinds retain their geometry/guards after
migration. A flat mill with metadata, a shaft segment, optional holder and two
independent presets survives save/reopen. The 6 mm/3-flute reference fixture
recalculates correctly; invalid expressions block commit and can be corrected
without losing drafts. Stock material preselects an associated preset; explicit
overrides survive material changes. Write failure commits no partial aggregate.
No unsupported machine setting silently becomes executable NC.

#### M2b — Manufacturer Fusion tool-library import

**Status:** proposed. **Addresses:** H14. **Dependencies:** M2a.

Inspect real manufacturer Fusion library files and document a supported format
subset/version before writing mappings. Stage import with provenance, unit
normalization, supported type mapping, geometry, shaft/holder, cutting presets and
catalog metadata. Present accepted/rejected records, unsupported fields and
duplicate handling before an atomic commit. Do not coerce new types into existing
ones or claim full Fusion expression/format parity. Preserve native exchange.

**Acceptance gate:** at least two independent manufacturer fixtures cover supported
types, units and presets, with independently checked dimensions. An unsupported
type and malformed record produce actionable diagnostics and cannot corrupt the
library. Imported supported fields survive native save/reopen and seed an actual
operation; unsupported expressions/coolant/post effects cannot execute silently.

### M3 — Multiple setups, fixtures and geometry assets

**Dependencies:** M1; coordinate contracts shared with M2. **Addresses:** A09/A12/A13.

The schema-15 setup collection and legacy migration are implemented; reuse them.
Complete the remaining rigid part/setup/fixture/machine transform and typed-frame
contracts. Distinguish WCS selectors from setup transitions and physical
re-fixturing. Preserve legacy origins without inventing orientation data: ambiguous
historical reorientation semantics require an explicit assumption/diagnostic.

Add target part assets, mesh/primitive stock definitions, fixture/clamp bodies and per-setup enabled states. Persist manufacturing exclusions and setup choices; keep highlighting, viewport visibility and selection as presentation state. Preserve local machine selection versus authoring provenance; project placement/config requests must resolve against the local installation rather than silently installing foreign safety limits.

The optional OCCT STEP adapter, exact topology, display tessellation and semantic
selection already exist for a qualified subset; complete the documented native
release/format qualification work rather than repeating backend selection.
Add mesh import with units, scale, normals, degeneracy/nonmanifold diagnostics,
spatial indexing and selection where needed. Keep geometry queries backend-neutral
and separate display tessellation from machining/collision geometry. Do not
implement a general CAD editor first.

Create crash-safe saves: temporary file in the destination directory, validated serialization, platform-appropriate flush/rename and recovery handling. Surface critical project/library save failures. Store large immutable assets outside per-edit snapshots, either in a versioned package or content-addressed project assets; keep `.ocam` migration support.

**Acceptance gate:** a two-sided part has explicit orientation transforms and independently placed clamps; operations select the intended surfaces after reload. Units cannot silently change scale. Unknown datum/asset references fail validation. Interrupted save preserves either the previous complete project or the new complete one. Legacy projects reopen through tested migrations.

#### M3a — Complete three-axis setup editing and viewport picking

**Status:** partial baseline exists; new requirements remain proposed.
**Addresses:** H03/H04/H06–H11. **Dependencies:** workflow gate table.

Keep fixed-axis three-axis milling as the available setup type. For one body or
one drawing choose the obvious target; for multi-body files allow one or more
explicit body instances. Retain exact setup-axis bounds and shared source placement.
Extend fixed-box min/center/max positioning with independent XYZ offsets; retain
relative-box six-side allowances and show containment feedback.

Add origin dropdown modes **Model coordinate system / Point on model / Point on
stock**. Model-point picking includes vertices and differently colored model
bounding-box points; stock-point picking includes stock corners/bounds. Highlight
and snap candidates. Persist origin-source references, so edits recompute or
invalidate them rather than leaving a stale coordinate.

For point modes require edge-defined orientation, using model or stock edges.
One direction leaves rotation unresolved; a second independent perpendicular
direction resolves it, with the third derived and labeled. Preserve optional third
axis consistency validation. Render stock/model bounds and a WCS triad; clicking
an arrow reverses that axis direction by 180 degrees and resolves a valid
right-handed frame or reports the conflict. Never accept a silently left-handed
or underconstrained frame. Camera movement does not change setup coordinates.

Select stock material from M2a's library. Add a setup-level local machine binding
that resolves to that machine's configured post for every operation in the setup.
Keep post/machine references out of the domain dependency cycle and preserve
local limits versus project provenance. Missing installations require resolution;
setup changes invalidate affected paths/NC/reports. Preserve separate NC per setup.

**Acceptance gate:** single/multiple-body defaults work; anchor plus offset moves
the stock/model relationship exactly once; fixed/relative stock survives reopen
and undo. Model/stock point snapping and edge picks resolve the same coordinates
in the preview, planner and NC. Invalid/underconstrained/left-handed frames block
commit; arrow flips preserve a valid frame or give a specific error. Two setups
can choose different locally resolved machine/post bindings without cache leakage.
Material defaults integrate without changing explicit operation overrides.

#### M3b — Fixture body assignments

**Status:** proposed. **Addresses:** H06/H12. **Dependencies:** M3a.

Add per-setup fixture roles/references for imported body instances, including their
placements. Distinguish fixtures from machining targets and viewport visibility.
Validate role conflicts and missing/replaced sources; undo/save/reopen preserve
assignments. Changing a fixture invalidates verification. Feed these bodies into
M5, rather than treating assignment or rendering as completed collision checking.

**Acceptance gate:** a multi-body import uses one body as the target and another
as a clamp, with independent assignments for the next setup. Hidden clamps still
appear in the verification input. Replacement/deletion produces a resolvable
reference error; assignments and transforms survive save/reopen and undo.

**Later integration gate (M5):** swept holder/cutter checks demonstrate an actual
fixture collision, including with a hidden clamp. M3b's assignment gate can close
before M5; collision detection is only complete when M5 passes.

### M4 — Programmable posts and independent NC interpretation

**Dependencies:** M1/M2 and M3 frame semantics. **Addresses:** A05 and programmable G-code support.

Extend controller capabilities from booleans to structured constraints. Define post packages with stable IDs, versions, firmware/options, configuration schema, machine compatibility, unit/arc/feed modes and supported process intents. Keep compiled posts as known baselines and allow package selection without editing `PostKind` everywhere.

Start with declarative formats and typed hooks for program start/end, tool-section start/end, spindle/coolant, datum changes, motion and cycle handling. Then add constrained scripting for controller-specific logic. A Rhai-style runtime, WASM or isolated process are candidates to evaluate; choose based on bounded execution, portable packaging and the event contract. Scripts receive explicit context and owned data and should have no file/network access by default. Version scripts/configuration and bound instructions, memory and output size.

Keep central facilities for numeric formatting, modal state, unsupported-feature errors and source mapping. Typed macro events should declare known motion/state effects where possible. Arbitrary inserted NC with unknown effects must downgrade verification to incomplete/unsupported; it cannot inherit a full verification claim.

Add a supported-subset NC parser/interpreter independent of the emitter's implementation. Reconstruct modal states, WCS, units, offsets, arcs, cycles, stops, tool changes and configured macro effects. Compare intended execution to posted/reinterpreted execution within tolerance, and run collision verification against that output. Distinguish unsupported NC from invalid NC. General user-authored controller macro languages can be a later separate feature.

**Acceptance gate:** a user installs a post package and changes safe-start/number formatting without rebuilding Rust; unsupported intent errors identify the operation and package. Every supported baseline post has golden fixtures plus semantic interpretation. Rounding, offsets and cycle lowering remain within the allocated error budget. Changed post script/config invalidates prior NC verification.

**Demo:** identical neutral machining intent produces qualified output for two configured controllers; a deliberately broken script, unsupported macro and stale post version cannot yield a passing artifact.

### M5 — Fixture and holder collision verification

**Dependencies:** M2/M3/M4. **Addresses:** A03/A08; requested clamping/collision detection.

Use one canonical execution timeline for stock removal, collision checking and playback, while retaining an independent NC interpretation layer to detect post defects. Include tool-section changes, positioning, cycle motions and setup transition checkpoints. Initial poses and unknown machine/offset state must be explicit.

Implement a body/transform scene and collision policy matrix. Include cutter, flute/non-cutting neck/shank, holder, stock, target part and fixtures. Intended cutting contact with stock is permitted only for cutting surfaces and correct process states. Holder-stock, assembly-fixture and cutting into protected target material are forbidden. Known mounting/assembly contacts require explicit allowed pairs; do not disable all machine-machine contact by default.

Add broad-phase BVH/AABB filtering and conservative swept checks. Select analytic shapes, convex decompositions or triangle methods according to accuracy/performance measurements. Account for the whole segment/sweep so a collision between sampled endpoints is detected. Keep visual mesh LOD separate from verification proxies and document conservativeness.

Integrate target gouge and residual checks in the application. Report body IDs, source operation/segment/NC line, time interval, collision location, minimum clearance and checking tolerance. Add stop-on-first and collect-all modes, local replay and highlighted bodies.

**Acceptance gate:** detect a holder hitting a clamp while the cutter is clear, a shank rubbing remaining stock, a rapid crossing a thin fixture between endpoints, and a gouge into the target. Known-clear cuts remain clear within the reported error bound. Unresolved tool/fixture geometry yields incomplete verification, not a clean result. The UI displays actual coverage.

**Demo:** a clamp collision is traced to an NC line; moving the clamp changes the input fingerprint, invalidates the old report, and triggers re-verification.

### M6 — Fixed-axis three-dimensional machining and rest stock

**Dependencies:** M3; integrate with M5 before shipping. **Addresses:** requested 3D toolpaths and general stock-aware planning.

Implement accelerated triangle/surface queries and first deliver ball-nose raster finishing over a bounded accessible surface. Compute cutter center from contact geometry; trim paths to boundaries and collision/reach limits; calculate collision-safe links and entry/exit. Keep output as XYZ motions in the current IR where possible.

Add Z-level roughing, waterline finishing and constant-scallop/steep-and-shallow coverage in separate validated slices. Implement per-axis/wall/floor allowances, stock-aware links and rest machining from prior operation results. Treat in-process stock as an input/output artifact with an operation dependency graph, not just a display mesh regenerated at the end.

Define a single machining error budget allocating CAD tessellation, cutter contact approximation, arc/path fitting, NC rounding and verification error. Specify feed modes and permitted dynamic smoothing. No strategy may improve speed by silently exceeding the requested tolerance. Adapt refinement to geometry/tolerance rather than arbitrary fixed samples.

**Acceptance gate:** plane, ramp, sphere, filleted pocket, steep wall and saddle fixtures meet declared surface deviation/scallop bounds for supported cutters. Compare at least one oracle/reference geometry independently from the path generator. Rest machining removes material missed by a larger previous cutter without recutting the whole part. M5 verification detects deliberate collision/gouge injection in each new strategy family.

**Demo:** imported 3D part → roughing → large-tool finish → small-tool rest finish → holder/fixture verification → posted NC and setup sheet.

### M7 — Volume stock and full-machine simulation

**Dependencies:** M3/M5/M6; machine/controller integration from M4.

Keep the heightfield for supported fixed-axis preview/verification, but introduce a `StockBackend` abstraction with removal, occupancy/distance queries, target comparison, snapshot/restore and setup transformation. Prototype multi-direction dexel and sparse voxel/volume approaches using memory, accuracy and replay benchmarks. Choose a backend from evidence; no single resolution is adequate for all parts and all claims.

Implement a machine link/joint tree with axis homes/limits, reference frames, table/spindle geometry and assembly mount frames. Start with a Cartesian three-axis mill. Forward kinematics drive display and collision transforms; resolve setup placement into machine coordinates and validate joint limits over full trajectories. Add rotary-chain variants only after this works.

Model tool-change and fixture transition motions that affect checking; when the controller macro is unavailable, report the excluded motion and do not claim verified ATC clearance. Add collision pairs for machine/fixture, spindle/holder/fixture, machine self-collision and assembly/travel envelopes. Improve execution timing using rapid policy and axis speed/acceleration data; characterize jerk/dynamics separately if supported.

Add simulation playback, pause/scrub, stock checkpoints and deterministic replay. Store results keyed by exact execution/configuration fingerprints. Render machine CAD/meshes through scene instances; animated appearance must be driven by the same poses verified by the checker.

**Acceptance gate:** full-body three-axis machine playback detects a spindle-body/fixture collision and a placed WCS exceeding an absolute axis limit even though the path span fits. Stock survives a defined setup reorientation with reported approximation error. A collision during an explicitly modeled ATC path is detected. Unknown controller-motion segments appear as coverage gaps. Scrubbing reproduces the same stock and events.

#### M7a — Stock from a previous setup

**Status:** proposed; independently deliverable before full-machine simulation.
**Addresses:** H05. **Dependencies:** workflow gate table.

Persist an explicit source setup and its completed remaining-stock snapshot,
result fingerprint, source/destination transform and approximation accuracy.
Do not use the finished CAD target as remaining stock. Reject dependency cycles
and invalid reorderings. Upstream tool/operation/stock/frame edits invalidate all
dependent stock, paths, simulation, verification and NC; stale or incomplete
results cannot supply stock for generation/export.

First qualify an aligned heightfield transfer subset with clear restrictions.
Arbitrary setup rotation, overhangs and inaccessible surfaces require a
volume-capable backend from M7's stock work. Keep unsupported transformations
unavailable with a reason until their material representation is qualified.
Define snapshot persistence/cache recovery so reopening cannot silently use a
different upstream run.

**Acceptance gate:** setup B consumes actual remaining material from completed
setup A; an independent residual comparison verifies transfer within the reported
error. Upstream edits, cancellation, failure, deletion and cycles cannot leave B
with exportable stale stock. Save/reopen retains the dependency and either
restores a matching snapshot or requires regeneration. A flipped setup passes
only once volume-capable transfer is qualified; unsupported transforms are
explicitly rejected.

### M8 — Qualified milling release and operational reliability

**Dependencies:** M4–M7 integration gates; M0 remains mandatory.

Pick explicit supported machine/controller/firmware/post configurations and maintain a qualification manifest for each. Gather manufacturer/control reference programs and validate emitted output in available controller interpreters/offline environments, followed by supervised dry-run and controlled cutting trials. Qualify each profile; do not generalize one Fanuc machine's acceptance to every Fanuc-like installation.

Deliver setup sheets containing stock, part/fixture/WCS diagrams, tool assemblies, T/H/D/pocket assignments, program revision and fingerprints. Add material/cutting calculators, realistic spindle power/torque/feed constraints, hole-making families, probing where explicitly supported and actionable diagnostics. Overrides need reason/provenance and must not pretend an unchecked condition passed.

Make export one transactional artifact: verify exact final NC bytes and their interpreted execution, include coverage/tolerance report and post/installation snapshots, then write those same bytes. Any geometry, clamp, tool, machine, post, offset, suppression or verification-setting change invalidates the report. Verification failed/incomplete/unsupported states have distinct handling and cannot automatically become a full pass.

Add autosave/recovery, portable issue bundles, memory/time budgets, crash reporting with opt-in data handling, release signing/reproducibility work where practical, and supported-platform GUI regression checks. Pin an actual tested Rust version for release qualification; `channel = "stable"` alone is a moving baseline. Keep a current-stable CI job as a separate compatibility signal.

**Acceptance gate:** a published declared milling capability matrix, passing regressions and machine/post qualification records; reproducible exported artifact and setup sheet; recovery preserves prior work; operator understands exactly which motions/bodies were verified. Performance targets use documented reference hardware, meshes, path sizes and tolerances.

### M9 — Indexed 3+2 machining

**Dependencies:** M8 and validated rotary machine variants from M7.

Add oriented tool poses and index transitions, rigid setup orientation, rotary forward/inverse kinematics, solution selection, axis limits, safe indexing and unwinding. Support controller workplane/TCP conventions as explicit profile capabilities. Preserve part-space cutter intent while representing machine-space execution separately.

**Acceptance gate:** tilted-face machining on supported table/head configurations matches intended geometry, verifies index/retract transitions and rotary limits, and survives nonzero tool length/pivot offsets. Different kinematic solutions are selected deterministically and reverified.

### M10 — Simultaneous axes, turning, mill-turn and extension API

**Dependencies:** M9; separate product/qualification decisions.

Simultaneous milling needs orientation interpolation, inverse kinematics along curves, singularity handling, collision-driven orientation, rotary velocity/acceleration constraints, TCP and inverse-time/feed semantics. Implement one defined machine topology/profile first.

Turning needs spindle synchronization, X diameter/radius semantics, CSS, tooling/turret and turning insert/holder geometry, cycles and process-specific stock/removal. Mill-turn adds spindle/channel synchronization, transfer/chucking states and competing machine resources. These do not fit the current flat milling operation/environment model without new contracts.

Only now freeze third-party strategy/post ABI after first-party contracts have been exercised. Choose explicitly between a versioned C ABI with owned buffers, a serialized subprocess protocol, or WASM components. Define capability negotiation, cancellation/progress, allocator ownership, package compatibility and crash containment. Preserve static first-party and headless operation.

**Acceptance gate:** process-specific correctness and qualification suites, explicit supported machine topology, bounded failure behavior and version compatibility tests; plugin failure cannot corrupt the active document or silently produce unchecked NC.

## 4. Cross-cutting verification program

| Area | Required evidence |
|---|---|
| Geometry | Degenerate/self-intersecting/near-tangent fixtures; large/small coordinates; unit conversion; transform invariance; fuzzing for crashes/resource exhaustion |
| Toolpaths | Complete coverage, target deviation, engagement/allowance bounds, correct entry/link ordering and stock dependencies; independent oracles where practical |
| Motion execution | Arc extrema/full circles/helices, cycle expansion, tool/datum changes, first-motion assumptions, unknown state, cancellation and resumability |
| Posts | Golden output plus independent semantics; modal reset/offset behavior; supported parser/control acceptance; unsupported features refused |
| Collision | Intended/forbidden contact matrix, whole swept motion, thin obstacles, holder/shank/fixture cases, false-negative characterization and conservative bounds |
| Persistence | Historical migration fixtures, future-version refusal, asset hashes, damaged/partial files, atomic-save fault injection and manufacturing suppression round-trip |
| Application | Responsive workers, stale-result rejection, memory bounds, deterministic replay, source-linked diagnostics and exact-artifact export |
| Release | Locked dependencies/toolchain, supported-platform checks, documented qualification matrix, reproducible reports and change impact reviews |

Existing golden NC should be updated only after the semantic behavior change is explained and checked. A passing golden comparison cannot establish controller acceptance by itself. The independent interpreter should not call the emitter's own lowering functions for its expected behavior, or it will reproduce the same defect.

## 5. Initial issue-sized backlog

Use the following order for issue-sized work. Status is based on the documented
packages and source review; a completed issue does not complete its whole parent
milestone. Each implementation issue carries the applicable acceptance evidence.

| Order / issue | Status / deliverable | Gate / blocks |
| --- | --- | --- |
| 1. Clearing regressions and incomplete jobs | Completed packages: retain their documented regression coverage | M0 baseline |
| 2. IR and bounds validation | Motion/arc package complete; tool/register references and broader document validation remain open | M0 reliable checks/posts |
| 3. Canonical drill execution | Shared cycle traversal, final position, unknown-tool errors and coverage | M0; M5/M7a |
| 4. Controller change/offset policies | Explicit Grbl/manual/ATC/TLO state and two-tool transitions | M0/M2 qualification |
| 5. Workspace shell | Implemented: default Machining, main Tools view, dimensioned grid and overlay hit testing; desktop smoke checks pass | M1a |
| 6. Tree commands and drag/drop | Implemented: context menus, visibility/status rows, undoable stable-ID ordering and transfer validation | M1a |
| 7. Setup placement and WCS references | Extend existing schema-15 setup model with offsets, origin modes, stock/model snap candidates and triad flips | M3a |
| 8. Setup installation resolution | Per-setup machine/post binding, local limits and cache isolation | M3a; M2/M4 integration |
| 9. Operation metadata and tabbed draft | Implemented: inventory-backed fields, units, inheritance, qualified geometry predicates and both creation orders; M2a presets deferred | M1b |
| 10. Tool aggregate ADR + migration | Eight-type identity, length/gauge conventions, snapshots, shaft/holder geometry and material/preset IDs | M2a; M2 ATC/holder checks |
| 11. Tool inspector and preset integration | Six tabs, expressions/validation, reusable preview, material matching and override preservation | M2a; finishes M3a/M1b presets |
| 12. Fusion vendor import | Real file fixtures, format/type/unit mappings, staged diagnostics and atomic installation | M2b |
| 13. Fixture assignments | Persistent body roles, transforms and invalidation | M3b; M5 |
| 14. Generation/verification workers | Completed M1 package: bounded immutable revision-stamped jobs, progress, cancellation and stale-result rejection; see [acceptance evidence](m1-domain-services.md) | M1; large paths/simulation |
| 15. Tolerance/coverage and collision spike | Error budget, reference bodies and swept holder/clamp prototype | M5/M6/M7 qualification |
| 16. Previous-setup stock contract and spike | Snapshot dependency graph, transform restrictions, residual oracle and backend benchmark | M7a; remaining-stock transfer |
| 17. Programmable post and NC contract | Typed events/packages, writer/source map and independent interpretation | M4 |
| 18. Stock transfer and release integration | Qualified M7a subset; M5–M8 collision/surface/volume/machine gates remain required | Previous stock and qualified milling release |

Write short architecture decision records for tool identity, frames/setup transitions, stock representation, geometry query API, post runtime, verification/export provenance and extension protocol. Each should record alternatives, compatibility/migration impact and a testable decision, not just an aspirational diagram.

## 6. Delivery risks and scope controls

- **Geometry correctness:** prototype with difficult real parts before choosing a CAD backend. Mesh import/tessellation does not itself solve cutter contact or safe linking.
- **Controller diversity:** ship a small qualified matrix and add profiles individually. Require explicit macro/offset assumptions rather than generic family-level optimism.
- **Verification cost:** measure error/memory/runtime tradeoffs on named datasets; separate preview speed from verification accuracy. Detect budget exhaustion as incomplete.
- **Domain migration:** introduce stable IDs and transforms with fixtures and migration tests before implementing feature-specific UI around the old model.
- **Duplicated semantics:** reuse canonical intent execution for planning/simulation/playback, but keep NC interpretation independent to catch lowering defects.
- **Scope expansion:** complete and qualify fixed-axis milling before simultaneous axes or other processes. Programmable posts and ATC can deliver value well before an unrestricted plugin ecosystem.
- **UI/backend mismatch:** expose only consumed parameters for the selected operation/input/post; shared setup heights, single-chain geometry and fixed coolant must not be presented as unsupported independent controls.
- **Vendor library compatibility:** qualify real manufacturer files and publish the mapped subset; reject unsupported types/expressions and preserve import diagnostics instead of claiming universal Fusion parity.
- **Remaining stock:** a target part mesh or transformed display heightfield is not evidence of material remaining after a setup. Enable transfer only for a qualified representation and invalidate every dependent result.

Completion is defined by the milestone acceptance gates and the declared qualified capability matrix. “It renders,” “the NC looks plausible,” or “there is a trait for it” are not sufficient completion criteria for the requested industrial functionality.
