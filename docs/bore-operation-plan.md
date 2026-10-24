# Bore operation — implementation plan

Status: proposed, 2026-10-23. Source baseline: `8417102dfb75f4804aec6ca06eaa2d81f0aeba9d`, document schema 25. This document plans implementation; it does not add a machining strategy or claim machine qualification.

## Goal and reference

Add a distinct **Bore** milling operation that helically machines internal holes and external bosses with straight or tapered circular walls. Use an end mill, derive geometry and cutting limits from selected CAD faces, and preserve the application's setup, work-datum, transactional editing, simulation and export workflows. Bore pitch describes axial travel per revolution; it does not describe a thread form. Keep Drill and Thread behavior unchanged.

The behavioral reference is the supplied 20-page **Fusion Help | Bore reference | Autodesk.pdf**, printed 2026-10-23. Its public source is [Autodesk Bore reference](https://help.autodesk.com/view/fusion360/ENU/?guid=GUIDAC5A8A1B-4259-4183-9E45-24320E513CAF). Page numbers below refer to the supplied PDF. The PDF links out for detailed height semantics and does not specify defaults, a taper allowance formula, stock-entry proof or a complete pass-generation algorithm. Those details below are OpenCAMStudio design decisions, not claims about Fusion's implementation.

### Reference coverage and delivery scope

| Reference behavior | PDF pages | Planned delivery |
| --- | --- | --- |
| Helical milling of straight/tapered holes and bosses; face-derived heights | 1, 3–4 | Cylinders in M1–M4; qualified cones in M5 |
| Coolant, spindle, cutting/ramp/plunge and lead feeds | 2–3 | Existing preset snapshots, `Cooling` and `EntryFeeds` in M1–M4; separate ramp RPM deferred |
| Faces or inclusive diameter range; holes/bosses/both | 3–5 | Associative query and explicit face modes in M2/M4 |
| Same diameter, optional same depth and same top | 5–7 | Per-seed matching in M2/M4; regeneration includes newly matching features |
| Multiple/nested containment boundaries | 7 | Closed boundary selection with even-odd containment in M2/M4 |
| Selected, optimized, inside-out, X, Y order; reverse | 8 | Deterministic feature ordering in M2/M4 |
| Tool orientation and origin choices | 8–9 | Existing setup frame and work datum; operation-specific indexed/rotary orientation deferred |
| Clearance, retract, top and bottom heights | 9–10 | Reuse all five current planes, including Feed, with per-feature cutting limits |
| Chord tolerance; pitch or maximum ramp angle | 11–13 | Cylinder helices in M3; tolerance-bounded taper segments in M5 |
| Computer/control/wear/inverse-wear compensation; control only on finishing | 13–14 | Computer compensation first; the other three modes deferred behind explicit qualification |
| Multiple passes, rough stepovers, finishing stepovers and repeat finish | 14–15 | Explicit radial pass schedule in M3/M4; pitch remains the axial increment |
| Climb/conventional direction | 15 | M3, honoring spindle direction and internal/external material side |
| Signed radial/axial stock to leave | 15–17 | Cylinder allowances in M3; taper allowance policy in M5 |
| Rapid/high-feed policies, safe distance, horizontal/linear/vertical leads, lead to center | 17–20 | Safe distance and horizontal/linear/center leads in M3/M4; rapid conversion and vertical smoothing deferred |

The first usable release is M1–M4: fixed-axis, complete cylindrical CAD features, flat end mills, computer compensation and top-to-bottom cutting. M5 extends it to reachable conical walls. M6 records broader Fusion-style options and drawing-based creation as future work. Show only implemented choices as editable controls; reject unsupported values loaded through files or APIs instead of silently substituting behavior. This is staged coverage, not full Fusion parity.

## Current implementation seams

| Area | Existing code and implication |
| --- | --- |
| Model and persistence | `crates/cam-model/src/document.rs` has eight `Operation` variants and schema 25. Add `BoreOp` and update identity, tool, datum, depth, transform and comparison accessors. Extend `validate.rs`, `geometry.rs`, `operation_heights.rs`, `tool_presets.rs`, reconciliation and migrations. |
| CAD topology | `CadSurface` in `cam-model/src/geometry.rs` includes cylinders but no cones. `cam-kernel/native/bridge.cpp` exports analytic cylinders. Cone support requires a native export, snapshot validation, versioning and reimport handling. |
| Feature extraction | `cam-geometry3d/src/lib.rs` already validates cylindrical hole/boss rims, seam faces, setup-Z alignment and terminations for `ThreadCylinders`. Reuse recognition helpers, not Thread's common diameter/depth restrictions. `ResolvedInput.holes` currently stores only center/diameter plus one shared `heights`. |
| Strategy and planning | Add `crates/cam-toolpath/src/bore.rs`, export `BoreStrategy` in `lib.rs`, and integrate dispatch, cutting heights and feed-plane handling in `plan.rs`. Adapt helix and lead primitives from `thread.rs`/`leads.rs`; do not inherit thread hand, thread-form depth or thread-mill gates. |
| Neutral motion and posts | `cam-cldata::Step::Arc` already represents XY circular/helical motion using an absolute center and endpoint Z. `cam-post` writes it as G2/G3 with incremental I/J. There is no current dialect-level helical fallback policy; qualify each post rather than assuming all controller variants accept the same output. |
| Application | Extend `OpKind` and creation in `controller.rs`, `cad_operations.rs`, `cad_creation.rs`, selection/drawing drafts, commands, inspector metadata/tabs, geometry editing, picking, tree labels and previews. Reuse revision-stamped jobs and committed-operation export. |
| Downstream | Existing cache, patterns, setup transforms, tool presets, stock simulation and export validation must consume Bore without special alternate job paths. |

## User workflow

1. Offer **Bore** wherever cutting operations are created: machining toolbar/menu, setup context menu and eligible CAD face context menu. Open the existing transactional right-column operation draft.
2. On **Tool**, select a flat `EndMill` and apply the material-linked preset snapshot. Seed RPM, cutting/plunge feeds, ramp/lead overrides and the existing None/MMS cooling choices. Surface speed/feed per tooth remain derived views where supported; do not duplicate independent stored feed values. Do not map a library `stepdown` to Bore pitch without an explicit Bore-specific preset contract.
3. On **Geometry**, choose Faces or Diameter range. Show eligible and rejected features, resolved count, each diameter/type/top/bottom, query filters, containment and final order. Allow holes and bosses with different dimensions in one operation. Require a nonempty resolved set to accept or generate.
4. On **Heights**, initialize Top/Bottom from each selected feature and reuse the current reference-plus-offset editors for Clearance/Retract/Feed. Show per-feature limits when they differ; never present a union interval as if every feature shared it. Explicit Top/Bottom overrides apply to every target and must validate against every target's certified geometry.
5. On **Passes**, choose Pitch or Angle, climb/conventional, rough radial steps, optional finishing passes, repeat final finish, stock to leave and tolerance. Display the resolved pitch/turn count and ordered pass radii for the previewed feature. Default to computed compensation, climb, no extra roughing, one target-wall pass, zero stock and no repeat; require a positive pitch or angle before OK. Initial tolerance is the existing 0.01 mm machining chord tolerance, explicitly editable and subject to machine/output precision checks.
6. On **Linking**, expose safe distance, separate horizontal lead radii, linear lead length and Lead to center for internal holes. Validate against tool-center free space before accepting. An external boss must never get a lead through its center.
7. Preview uncommitted geometry and paths with existing draft ownership. **OK** commits parameters, binding/query and height definitions as one undo step; **Cancel** discards them. Recompute, simulate, suppress, duplicate, pattern and post/export follow existing operation actions. Dirty drafts do not alter committed exportable paths.

Keep advanced unsupported options out of the active inspector. Explain in documentation why a tilted face needs another setup, why a tool does not fit, or why a cone/entry/lead cannot be certified. Report feature-specific errors without silently omitting failed targets.

## Model, persistence and resolution contracts

Add the following conceptual types; finalize Rust names during M1 without changing their ownership:

```text
Operation::Bore(BoreOp)
BoreOp {
    id, tool, work_offset, spindle_rpm, tool_preset, cooling,
    feed, plunge_feed, entry_feeds,
    targets: Vec<BoreTarget>,
    ramp: Pitch { mm_per_rev } | Angle { degrees },
    climb, tolerance,
    roughing: Option<{ stepovers, stepover }>,
    finishing: Option<{ passes, stepover, repeat_final }>,
    radial_stock, axial_stock, entry_conditions,
    linking: { safe_distance, horizontal_in_radius,
               horizontal_out_radius, linear_length, lead_to_center }
}
BoreTarget {
    center_xy, internal, top, bottom,
    radius_at_top, radius_at_bottom,
    termination: Blind | Through | External,
    source_faces / stable source identity when CAD-derived
}
ExtractionRecipe::BoreFeatures {
    selection: Faces { same_diameter, same_depth, same_top }
             | DiameterRange { min, max, feature_type },
    containment, order, reverse
}
ResolvedInput.bores: Vec<ResolvedBoreFeature>
```

- Store explicit target geometry for standalone/drawing operations. For CAD-linked operations, targets are the materialized snapshot of the binding; recipe, seed references and query scope remain authoritative. Generation requires a matching resolved fingerprint. Persist no viewport triangles or generated paths as machining authority.
- Keep operation height definitions and geometry bindings in their existing maps keyed by stable operation ID. Extend Bore height resolution with **per-target** cutting intervals. Selection-derived Top/Bottom mean the individual feature extrema plus offsets; other references resolve normally and must fit all targets. Clearance/Retract/Feed may be common but must clear the highest target. Extend both the application resolver and `compute_at_heights`; do not overwrite every target with a single aggregate interval.
- Explicit numeric targets support standalone backend use and tests. Drawing-based creation is deferred: the current planar selector fits circles to sampled points, so it needs an explicit recognition/error contract or preserved analytic circle provenance before supplying Bore dimensions. A later drawing workflow also needs a hole/boss choice, user-specified top/bottom and declared entry conditions; a circular drawing alone establishes no physical opening.
- Scope associative discovery to the active setup's explicitly selected model bodies/instances. A query stores that scope and the selected seed faces, not just today's expanded face list. Different material sides can coexist; each target carries `internal`. A single feature is emitted once even if matched by multiple seeds or seam faces.
- Include all targets, settings, query results and certified entry dependencies in resolution/cache inputs. `OperationCache` already keys on the concrete operation, tool, heights, stock bounds and spindle; extend dependencies where needed for actual stock/entry geometry. A changed topology revision, setup frame, scope, filter, allowance, lead, tool diameter or feed must not reuse stale motion. Pattern-only changes can continue reusing seed motion.
- Use schema 26 if 25 remains current, otherwise the next unused version. Add the migration step, serde representations and complete enum coverage. Old projects gain no Bore operations and retain their output. Readers that cannot understand the new schema must refuse it. Round-trip explicit and linked operations, filters, heights and preset snapshots through the normal project loader.
- Preserve source IDs and provenance through duplication, setup copy, undo/redo, suppression, deletion and pattern membership. Reconcile missing/revised CAD seeds through `NeedsReselection`; no nearest-face guessing. New matching query features appear on regeneration. Missing topology needed for cones requires reimport/reselection, not mesh fitting.

## Exact feature selection and ordering

Implement `cam-geometry3d/src/bore_features.rs` around exact topology in setup-local coordinates:

1. Transform instance geometry into the setup frame once. Recognize complete coaxial cylindrical/conical wall groups, including split seams, and derive exact circular rims and trimmed Z intervals. Classify hole/boss from oriented solid topology. Check complete angular coverage, adjacency, accessibility, planar entry and blind/through termination; a bounding box alone is not proof of a valid bore.
2. Accept axes parallel to setup Z in either sign, normalizing their parameterization. Reject tilted axes, open/partial walls, intersecting ambiguous features, unsupported blends and noncircular trims with a reason. Mixed heights and diameters are valid and remain separate targets. Do not weaken Drill/Thread extraction checks while sharing helpers.
3. Apply diameter limits inclusively within recognition tolerance. For cones, define query diameter as the diameter at the feature's geometric **top rim**, independent of subsequent machining offsets. Show both rim diameters in the inspector. This definition is a product choice because the PDF does not specify which tapered diameter is compared.
4. Same-diameter expansion matches any seed's top diameter within the existing 1e-5 mm recognition tolerance. If enabled, same-depth and same-top comparisons must match that same seed, not attributes taken from different seeds. Retain material side when expanding a seed. Match by setup coordinates, before allowance or height overrides.
5. For containment, persist closed CAD/sketch boundary references. Use projected feature centers and an even-odd rule: odd nesting depth includes, even excludes; multiple disjoint outer boundaries include their interiors. Include centers on a boundary within tolerance, visibly. Reject open/self-intersecting or unresolvable boundaries. This filters locations and does not certify tool/holder clearance.
6. Selected order preserves seed pick order, with each seed's discovered additions sorted by stable instance/body/feature identity. Range mode starts from that stable order. Optimized uses deterministic nearest-neighbor XY travel with stable tie breaks; label it a heuristic. Inside-out sorts by distance from the candidate-set XY centroid; X/Y orders sort their named coordinate then the other. Reverse the final feature list, never reverse a helix to implement feature ordering.

## Toolpath and machining semantics

### Cylinder path and pass schedule

Implement a pure, cancellable `BoreStrategy` that validates all targets and parameters before emitting an exportable result. Use millimeters internally. For model radius `R`, tool radius `r`, signed radial allowance `s`, and axial allowance `a`:

```text
internal final tool-center radius = R - s - r
external final tool-center radius = R + s + r
effective bottom = resolved target bottom + a
pitch mode: Z advance per turn = p
angle mode on a cylinder: p = 2*pi*tool_center_radius*tan(angle)
```

Positive stock leaves material; negative stock intentionally cuts past nominal geometry. Require `abs(negative radial stock) < r`, a positive tool-center orbit with margin, a valid remaining cutting interval, and a certified bottom/clearance volume after allowance. A negative axial value cannot extend through a blind floor merely because the parameter accepts negative numbers. Use only flat end mills initially; defer ball/corner-radius allowance rules and associated cutter contact models.

Define radial passes independently of helical pitch. If roughing specifies `n` stepovers of maximum size `d`, emit `n` rough helices on the material-free side of the final finishing region, then the target helix/finishing schedule. Let `q=-1` for internal, `q=+1` for external, `rho` be the final tool-center radius, and finishing count `m>=1` with spacing `f` when enabled:

```text
first finishing radius = rho + q*(m-1)*f
rough radii            = first_finishing_radius + q*k*d, k=n,...,1
finishing radii        = rho + q*j*f, j=m-1,...,0
repeat final finish    = append rho once, when requested
```

Without finishing enabled, use one final target helix after the rough passes; disable Repeat final finish. Require positive spacing for multiple passes and validate every orbit/transition. Zero rough steps means no extra rough helices. Every helix traverses the full target cutting interval. Display actual passes/counts; do not interpret these radial settings as axial depth layers. The PDF's “multiple depth cuts” wording is ambiguous: page 14 illustrates axial direction but does not fully define radial pass sequencing. The schedule above is an explicit product contract based on its Number of Stepovers controls.

- For an M3 spindle, internal climb is CCW and external climb is CW; conventional reverses them, and M4 reverses the mapping again. Use the existing spindle convention. Initial Bore cutting always descends: no thread hand/chirality rule forces axial direction. Page 14 shows both descending external and ascending internal examples; ascending cutting is deliberately deferred to an explicit Cut-up option requiring certified access to the start depth.
- Emit constant-radius helices as XY `Step::Arc` moves split into at most 180-degree sweeps. Derive coordinates from analytic parameters, not accumulated deltas; support fractional final turns and land exactly at the effective bottom without overshoot. Add a complete constant-Z cleanup revolution at the bottom of each cylinder pass so the last ramp does not leave an axial wedge. Distinguish this required cleanup from repeating the final finishing helix.
- Use `entry_feeds.ramp(feed)` for the descending helix and cutting feed for constant-Z cleanup; use independent lead feeds and plunge feed on their corresponding segments. These are tool-center path feeds; automatic bore wall-feed correction is deferred. Keep RPM/coolant/tool state in the normal planner assembly.
- Angle is a finite maximum in `(0, 90)` degrees. Pitch is finite and positive. Compute angle-derived pitch separately for every target/pass radius. Validate against any available cutter/machine limits; do not treat either parameter as a safe material removal recommendation.

### Entry, exit and links

The selected final wall does not prove that the tool can reach the first orbit. In particular, an internal helix can leave a central core, and a boss may be surrounded by uncut stock.

- Add per-target entry conditions: a cleared opening radius and Z interval for holes, or a cleared radial annulus/approach region and Z interval for bosses. Record whether this comes from a supported stock-state check or an explicit user declaration. Persist declarations in `entry_conditions`, display them in Geometry as setup assumptions, and bind them to stable target identity. They describe existing material removal, never an automatically inferred void from the final CAD model.
- Check the complete tool swept volume of approach, lead, helix, cleanup and exit against the intended cut and surrounding part/stock. Start with analytic 2.5D clearance checks against those entry regions and exact feature boundaries. Consume prior-operation stock state only where the existing verifier can establish the required region; include its fingerprint in cache dependencies. Unsupported clearance geometry fails with a specific reason. A declaration is an explicit assumption, not measured or simulated proof, and must be reported as such in job diagnostics.
- For the initial implementation, require an established internal opening that clears the cutter/lead and no residual central core, or an external access region that clears the approach. Solid-stock hole opening and general pocket clearing are outside Bore's first release. Reject paths that need them and direct the user to an appropriate clearing operation.
- Traverse between targets above the common clearance plane: retract vertically, move XY, then descend at the destination. Observe per-target tops, Feed/Retract, machine bounds and the existing tool-change/datum rules. Do not rapid through a wall or leave a tool embedded while moving between radial passes.
- Construct tangent horizontal arc/linear leads in the free side of the boundary. Lead to center retracts within a certified internal opening and is disabled for bosses. Safe distance is measured from the allowance-adjusted surface and includes the cutter envelope; validate it after signed stock changes. Oversized leads produce errors rather than automatic clipping.
- Exit into a certified free region before vertical retract. For external features, the region must account for nearby shoulders, floors, islands and fixtures. Reuse existing verification capabilities accurately: do not claim holder/fixture or full 3D collision certification from heightfield stock simulation. Until a required clearance can be established, that case remains unsupported.
- Validate finite fields, positive feeds/tolerance, integer bounded counts, nonempty targets, tool kind/diameter, flute/reach/shank constraints, height ordering, clearances and machine/output limits in both model/API and generation paths. Lead dimensions and safe distance are nonnegative; zero lead dimensions disable those lead components. Require a positive actual retract clearance from the adjusted surface. Attribute diagnostics to the target and operation.
- Check cancellation between targets, passes and segment batches. Preflight checked target/pass/turn/segment counts before allocation. Propose caps of 10,000 targets, 100,000 turns per operation and 1,000,000 generated motion steps, also respecting the existing 5,000,000 expanded job/pattern limit. Qualify these limits under representative loads; excess work errors explicitly, without partial success or caching cancelled results.

### Cones and numerical tolerance (M5)

Add `CadSurface::Cone` with an oriented axis, reference origin/radius, signed semi-angle and inward flag. Export exact OCCT parameters and validate finite, nondegenerate radii throughout the trimmed interval. Version the topology snapshot contract and update all exhaustive surface matches, including inspection, fingerprints, transforms and supported extraction paths. Existing cached Unsupported cone surfaces need reimport; persisted exact cone snapshots must subsequently reopen without OCCT.

Resolve a linear radius profile between exact rims. A varying-radius conical spiral is not a constant-radius XY arc: emit tolerance-bounded `Step::Linear` segments. Subdivide adaptively using a conservative bound on the complete 3D curve deviation, including radial and Z change. In Angle mode use local XY path length so `abs(dz)/ds_xy <= tan(angle)` everywhere; the cylinder formula alone is insufficient when radius changes. Preserve exact terminal depth and bound cancellation/resource cost.

Define tapered allowances in the axial/radial section by offsetting the nominal wall along its unit normal into free space, with distance `sqrt((radial_stock*n_r)^2 + (axial_stock*n_z)^2)` for nonnegative allowances; the floor is separately offset axially. This gives radial stock on a vertical wall and axial stock on a horizontal floor, with a documented transition on slopes. Start M5 with zero/nonnegative stock only; mixed-sign or negative tapered allowances remain disabled until their signed offset/intersection rules are separately specified and qualified. Do not claim this is Autodesk's undocumented interpolation formula.

Use the flat cutter's complete swept envelope to solve contact and reject unreachable tapers, neck/shank interference and unsafe transitions. A tip-only `R(z)-r` calculation is insufficient where the upper cutter intersects a narrowing wall. Bound leftover wall scallops as well as path chord error: a finer chord tolerance cannot compensate for excessive pitch on a taper. Initially reject cone/tool combinations that cannot meet the requested wall tolerance; do not approximate them as cylinders. Constant-Z cleanup must use the qualified taper contact solution rather than an assumed cylinder orbit.

## Planner, postprocessing, simulation and application integration

- Materialize CAD queries/heights in the existing job snapshot, then pass certified per-target geometry into the strategy. Keep machine transitions in `plan.rs`; the strategy emits ordinary tagged CL motion. Stamp setup/operation and pattern-instance ownership normally. A feature failure makes the operation/job incomplete and blocks export; visible partial preview is not success.
- Reuse pattern XY rigid transforms after seed generation. Rotate/translate all Bore motion endpoints and centers; preserve Z, feeds and arc direction. Test nonzero circular pattern centers, multiple datums and transformed setup frames. Keep source order stable and avoid duplicate identity execution.
- Qualify Fanuc, Okuma, GRBL and Estlcam output using existing post tests and rounded-motion validation. Explicitly declare dialect helix support for the supported controller profile or provide a tolerance-bounded linear fallback shared with simulation/preview. Unsupported combinations must fail posting. Avoid full-circle endpoint ambiguity and retain G17/helical Z/I/J semantics appropriate to each dialect. Do not emit drilling/boring canned cycles for this milling operation.
- Feed the same CL motion through arc bounds, machine validation, backplot and stock simulation. Check final diameter, depth, stock allowance, residual core/scallops and links against analytic expectations. Record heightfield/holder/fixture limitations in release evidence instead of overstating collision coverage.
- Audit every exhaustive operation match, including tool library filters/defaults, preset application, operation-height editing, setup transforms/copying, reconcilers, drawing creation, CLI labels, GUI metadata, field validation, draft merge, selection overlays and tree actions. A successful headless compile alone does not cover GUI-gated matches.

## Milestones and acceptance gates

| Milestone | Work and dependencies | Required evidence before proceeding |
| --- | --- | --- |
| M1 — Model and contracts | Add Bore types, next schema/migration, validation and complete operation accessors. Specify per-target height and entry-dependency interfaces. | New/legacy project round trips; invalid fields rejected; undo/redo and setup-copy IDs preserved; headless and GUI compilation. |
| M2 — Cylinder recognition and queries | Depends on M1. Extract exact features, keep independent heights, associate face/range queries, boundaries and ordering; resolve explicit backend targets and entry regions. | Split seams deduplicated; mixed type/diameter/depth/top retained; scope/filter endpoints deterministic; reimport/query expansion and unresolved seeds handled; no display-mesh dependency. |
| M3 — Cylinder strategy and verification | Depends on M1/M2. Implement pass schedule, pitch/angle, allowances, direction, cleanup, leads, entry proof, cancellation and planner integration. | Analytic geometry/feed tests, documented radial schedule and invalid-entry cases pass; output bounded; no unsafe rapid; failure/cancellation blocks completion/export. |
| M4 — Usable operation and cylinder release | Depends on M3. Complete five-tab draft UI, tool presets, previews, all actions, patterns, simulation and four posts. | Controller/GUI tests, manual creation/edit/cancel workflow, dialect goldens and rounded-motion checks; documented preview/simulation and controlled machine dry-run evidence for claimed controller profiles. This is the cylinder release gate. |
| M5 — Reachable tapered features | Depends on M4. Native cone export/versioning, taper queries, contact/allowance construction, adaptive segments and scallop limits. | Native STEP/IGES cone fixtures, setup-axis reversal, reopen without OCCT, geometric error bounds and envelope rejection tests; post/simulation qualification with explicit supported taper/tool limits. Only then enable cones. |
| M6 — Optional extensions | Depends on qualified earlier milestones. Separate ramp RPM, operation-level orientation, cut-up, expanded cutter types, control/wear modes, high-feed conversion, vertical lead smoothing and drawing-based creation. | Each option has its own model/IR/post, safety and regression contract before exposure; none is implied by completing M4/M5. |

Implement as reviewable commits following these dependency boundaries; update this document with actual delivered scope and qualification evidence. Do not wait for M6 to ship the supported cylinder workflow.

## Validation matrix and implementation checks

| Area | Meaningful cases and assertions |
| --- | --- |
| Cylinder geometry | Internal/external, blind/through, split seams, reversed axis; exact radius/depth, complete cleanup and no duplicate feature. Reject tilted/partial/capped/blended/ambiguous walls. |
| Per-target heights | Different tops/bottoms in one operation; default selection heights remain individual. Explicit common override outside any target interval fails. Verify setup/world/datum transforms apply exactly once. |
| Numeric example | Hole Ø20, tool Ø6, Top 0, Bottom -10, zero stock gives orbit radius 7. Pitch 2 gives five descending turns plus bottom cleanup. Angle 5° gives approximately 3.848 mm/rev. Radial stock +0.2 gives radius 6.8; axial +0.5 stops at -9.5. Boss Ø20 with that tool gives radius 13, or 13.2 at radial stock +0.2. |
| Pass scheduling | Internal radius 7, two rough stepovers of 1, two finish passes spaced 0.2 gives radii 4.8, 5.8, 6.8, 7; repeat appends 7. Mirror offsets outward for bosses. Check infeasible inner orbits, counts and finishing-disabled behavior. |
| Direction and feeds | Internal/external × climb/conventional × M3/M4, always descending initially; angle-derived pitch varies by pass radius. Fractional turns and very shallow cuts finish exactly. Check separate ramp, cleanup, lead and plunge feeds. |
| Selection queries | Inclusive min/max, two seed diameters, same-seed depth/top predicates, mixed types, nested/disjoint boundaries and boundary centers; stable order/ties/reverse. New matching CAD feature appears after regeneration and excluded bodies do not. |
| Entry and links | Solid hole stock, residual core, uncleared boss surround, insufficient reach, oversized lead, narrow pre-clearance, negative stock at blind floor and nearby shoulders fail as specified. No XY rapid below clearance; cancellation never produces an exportable job. |
| Cones | Both taper signs and material sides, small/large radii, angle and pitch mode, split surfaces, cone-apex exclusion, flange/shank interference, allowance offsets and scallop limits. Measure whole-curve error, not only sample endpoints. |
| Persistence/cache | Old files unchanged; Bore round trip; missing seeds and older topology require resolution. Tool/stock/topology/filter/height/allowance/lead edits invalidate affected paths; pattern-only edits reuse seeds; cancelled/failed results are never reused. |
| Integration | Suppression, operation/setup duplication, undo/redo, tool removal, mixed-tool job, multiple datums/setups, both pattern orders and nonzero pattern center. Four dialect goldens, precision-induced collapse and machine-travel failure. |

During implementation, run focused tests for the changed milestone, then the repository's normal gates:

```sh
cargo test -p cam-model -p cam-geometry3d -p cam-toolpath -p cam-cldata -p cam-post -p cam-sim
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy -p cam-app --features gui -- -D warnings
cargo build -p cam-app --features gui
cargo test -p cam-app --features gui
```

For M5, use the tested OCCT 7.9.3 SDK and environment setup in [the kernel build reference](../crates/cam-kernel/README.md), then run native kernel/application tests with `occt` enabled and add cone import fixtures to those suites. Keep non-native tests runnable without that SDK. Follow CI's advisory formatting policy. Record manual GUI, native import, simulated stock and physical dry-run evidence separately; passing Rust tests alone does not qualify a machine. These are future implementation checks, not checks claimed as run by this documentation change.
