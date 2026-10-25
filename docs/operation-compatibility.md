# Operation compatibility and current limits

Source review and regression work: 2026-10-25. This describes the current
implementation, including the rounded-entrance and thread-generator fixes.
It is a capability inventory, not a promise of arbitrary 3D surface machining.

## Why Thread rejected `thread.ocam`

The selected feature is inward cylindrical Face 3: centre (-30, 10), diameter
30 mm, straight wall from setup Z0 to Z16. Face 2 is a coaxial quarter-torus
with a 4 mm entrance radius, opening to a planar rim at Z20. It does not cap or
undercut the bore.

Bore already certified that rounded entrance analytically. Thread and Drill
used a separate, older switch that required a planar face directly against the
cylinder's upper rim. They rejected the torus before toolpath generation.
All three now use the same cylinder/access certificate. Their cutting interval
remains Z16 to Z0; the torus supplies access, not a thread or bore cutting surface.
Incomplete, covered, noncoaxial, narrowing and obstructed entrances, and rounded
bottoms, remain rejected.

The saved project contains an 8 mm **end mill**, tool 2. Thread needs an actual
**thread mill** with a suitable cutting diameter, reduced neck and reach. The
thread specification is also explicit: the selected 30 mm bore does not establish
a nominal thread size or pitch. The current UI seeds major diameter from the
cylinder diameter. Review it: if the bore is already 30 mm, a nominal 30 mm
internal thread cannot restore the missing flank material. Select the intended
thread specification and an appropriate prepared bore. No standard thread size
was inferred automatically in that initial fix. Geometry now offers optional
standard-size matching and an explicit preset dropdown; see
[Thread geometry and standard presets](thread-presets.md). Existing operations
retain their saved values until a preset or matching mode is chosen.

## Operation and feature matrix

| Operation | Supported geometry and controls | Current restrictions and practical choice |
|---|---|---|
| **Profile** | Drawing contours; CAD Face Contours, connected planar Chains, and body Silhouettes; per-entry side and direction; axial passes, finishing and spring passes, leads, ramps, tabs and compatible stock allowances. | Auto/Closed CAD chains need certified vertical supporting walls. Explicit Open mode accepts a planar path adjoining curved surfaces, with operator-defined depth; this does not finish the adjacent blend. Silhouette follows the projected envelope, including projected through-openings. Linked CAD profiles reject radial roughing, controller compensation and taper. Drawing roughing is limited to one closed outside contour with computed compensation and valid enclosing stock. Tabs require compatible closed/computed/no-roughing/no-ramp settings. Use Pocket to clear an internal region and Bore to finish a cylindrical wall. |
| **Pocket / 2D adaptive clearing** | Closed drawing regions with islands; one upward CAD floor with complete vertical constant-section walls; layered clearing, offset or adaptive paths, entry choices, feeds, leads and radial/axial stock to leave. | Sloped or blended walls/floors, unequal opening elevations, intermediate openings, a hole through the selected floor and undercuts fail feature certification. A rounded mouth is not a license to clear an arbitrary blended pocket. Use 3D Adaptive for supported varying model sections. |
| **Drill** | Circular drawing holes or CAD inward cylindrical faces; compatible multi-hole groups, peck and dwell; posts emit their supported cycles or expanded motions. Qualified outward-opening rounded entrances are accepted. | CAD axis must be parallel to setup Z; equal diameters and top/bottom heights; cutter diameter must match the selected bore. Circular edges are resolved to their adjacent cylinder, not accepted as independent evidence of hole depth. Conical tips, arbitrary blends, counterbore stacks and automatic breakthrough are not inferred. A centre-cutting end mill can plunge with a warning; use a drill for drilling. |
| **Face** | Drawing boundary, upward planar CAD target, or body silhouette; layered X/Y passes, overlap, overshoot and axial stock to leave. CAD face selection uses a conservative rectangular body envelope above the target, including space above holes. | Target must be at the body's highest accessible Z; raised protected material blocks facing below it. CAD overshoot cannot be negative. Radial stock to leave is unsupported. This is stock facing, not finishing a freeform face. |
| **Chamfer** | Drawing/open or closed planar paths; connected CAD edges or loops; modeled inclined planar bevel faces with upper/lower boundaries. Width passes, gradual sizing, bands limited by stepdown/tool flank, leads and explicit side. | Requires a suitable V-bit/chamfer cutter; modeled bevel and cutter angles must agree. A rounded fillet is not a planar chamfer. General conical/freeform bevel faces and nonplanar edge following are unsupported. Open paths retain endpoints and do not receive closed-loop overlap. |
| **Bore** | Equal circular drawing loops or certified CAD cylinders/bosses; qualified rounded internal entrances; helical pitch or ramp angle, roughing/finishing radial passes, spring pass, stock, leads, hole ordering and direction. | Wall milling assumes an accessible/prepared feature; it does not clear a solid hole. CAD side, diameter and straight wall follow the cylinder. Drawing taper is supported only in the non-undercutting direction; conical CAD faces are not exposed by the current importer. Mixed diameter/depth groups need separate operations. Compensation is computed in CAM. |
| **Thread** | Circular drawing centres or certified CAD bores/bosses, including qualified rounded internal entrances; explicit major diameter/pitch, RH/LH, climb/conventional, radial/gradual and spring passes; single-form or matching-pitch full-profile thread mill; independent lead feeds. | Current form is a cylindrical 60° thread; no taper or other thread forms. Cutter must fit both the actual selected CAD bore and the basic minor bore; a single-form neck must clear the radial thread depth; requested length must fit configured reach. Full-profile pitch must match and interval must allow at least one pitch of travel. Blind clearance/allowance is explicit, not inferred from geometry. Fixed semicircular leads; no adjustable lead radius or hole-order control. CAD does not check the prepared bore against a thread-standard tolerance/class or infer whether the entered nominal diameter removes enough material. |
| **Engrave** | Drawing paths or connected planar CAD edges/loops, open or closed; top/depth, axial stepdown and feed. A V-bit follows the path centre. | Requires a cutting V-bit tip; an ordinary end mill or flat noncutting chamfer tip is rejected. Groove width follows cutter geometry and depth. No projected lettering/text generation or following nonplanar surfaces. Operator-defined depth does not establish material/access certification for an arbitrary adjoining surface. |
| **Carve** | Closed drawing region or one planar CAD face/loop, with inner face loops retained as islands; V-bit variable-depth rings, depth cap, ring spacing/scallop, stay-down links and optional separate clearing cutter. | Requires a V-bit and valid closed region. Depth is a maximum that narrow features may never reach. The V pass has no axial stepdown schedule; its optional clearing pass has its own stepdown. No arbitrary sculpted-surface or multi-plane carving. |
| **3D Adaptive Clearing** | Whole setup bodies, selected positive-area XY face projections, or both; complete setup-model obstacles; changing projected sections, primary/fine stepdowns, optimal load, tolerance and radial/axial stock to leave. | Conservative three-axis roughing using independent machining projection data. Vertical faces with zero-area XY projection are not individual machining domains. Undercuts remain protected/unreached. Old assets lacking qualified projection require reimport. Tolerance must cover the approximation budget. Pattern repetition is rejected because copied paths would bypass geometry checks at the new location. This is not general freeform finishing or rest machining. |

## Features that must work together

All operations participate in the document/setup/tool/height system, undo and
redo, transactional edits, save/reopen, exclusion, generation completion and NC
export. A compatible geometry selection alone does not establish cutter
suitability or valid cutting parameters; those are checked during generation.

- **Geometry and setup:** planar strategies use setup XY; cylinder axes use setup
  Z. Ordinary linked operations select one body instance and compatible entity
  kinds per field; named Profile entries and Adaptive have their own collection
  contracts. Set up another fixed orientation for tilted features. Display mesh
  quality must not change these analytic machining inputs.
- **Heights and stock:** linked top/bottom and overrides must respect the
  certified feature. Reference planes, actual cutting interval, reach and safe
  retract must agree. Generic Stock to Leave applies to Profile, Pocket, Face
  (axial only) and Adaptive; Bore has its own allowances. Moving the stock or
  changing a height does not make an unqualified wall safe.
- **Tools and feeds:** each strategy checks its required cutting surfaces and
  reach. Embedded tools and material presets supply operation data; cooling,
  spindle and entry feeds only act where the operation emits those motions.
  A thread helix uses the thread cutting feed; the separate lead feeds govern
  its entry and exit arcs.
- **Associativity:** re-resolution refreshes centres/boundaries and linked
  dimensions while retaining machining settings and explicit thread size/pitch.
  Stale, replaced or missing included CAD links block export instead of using an
  old embedded path. Excluded operations do not block the included job.
- **Patterns and multiple origins:** rigid XY path replication preserves motion
  semantics, not independent model-feature certification at the destination.
  Adaptive is explicitly excluded. Compound Carve patterns and fixture/holder
  clearance still need separate qualification.
- **Simulation and posting:** neutral arcs, helixes and drill intents go through
  existing simulation/post pipelines. Heightfield simulation cannot validate
  thread flank form or undercuts and is not a holder/machine collision model.
  Controller compensation remains dependent on operation and post capability.

## Changes and verification

1. Removed the Bore-only rounded-entrance switch: Drill, Bore and Thread now
   share the exact cylinder/open-access checks and operation-neutral errors.
2. Full-profile thread pitch mismatch now blocks generation. A requested
   interval shorter than one pitch cannot silently compress one revolution
   into the wrong axial travel.
3. Both full-profile Z directions now sweep the same bottom-anchored pitch.
   The cutter comb extends upward from its tip; starting a downward cut at the
   thread top missed the lower thread.
4. Linked Thread checks that the cutter fits the actual selected bore even
   when a larger nominal thread diameter would otherwise pass its minor-bore
   gate. Invalid edits are rejected atomically.
5. Thread rejects nonfinite dimensions/centres, invalid blind allowances,
   unsafe retract ordering, collapsed thread roots and excessive motion counts
   before allocating pass/motion buffers.
6. Thread geometry help explains rounded access, the required tool and the
   difference between selected bore diameter and nominal thread specification.

Regression coverage in `crates/cam-app/tests/bore_project.rs` uses the existing
tracked project containing the same cylinder/torus as `thread.ocam`. It checks
the compatible feature/cutter workflow for all ten operation families, rounded
entry failure cases for all three hole operations, and thread generation,
fractional-turn pitch, passes, save/reopen and every post. Strategy tests cover
single/full-profile direction, pitch matching, short intervals and path limits.
The carving workflow faces the stock and clears the recess before finishing; a displayed CAD pocket alone does not remove stock, and the export gate correctly rejects rapids into its uncut corners.
These are headless tests; native desktop interaction and physical machining are
outside their evidence.

Remaining feature work is explicit: general blended/sloped pocket recognition,
conical CAD feature descriptors, per-hole varying dimensions/heights, thread
tolerance-class and prepared-bore engagement validation, automatic blind-hole semantics,
thread-form simulation, and independently revalidated pattern destinations.
Those need their own geometry/tool contracts rather than disabled rejection
checks. See [Bore](bore-operation.md), [Profile](profile-operation-implementation.md),
[3D Adaptive](3d-adaptive-clearing.md) and
[patterns](operation-patterns-implementation.md) for detailed controls.

Validation commands:

```sh
cargo test --workspace --offline --no-fail-fast
cargo test -p cam-app --features gui --offline --lib
cargo test -p cam-app --test cad_features --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

Results: workspace 1,054 passed / 25 ignored; GUI library 393 passed / 4 ignored;
final CAD integration rerun 32 passed. Strict workspace Clippy and whitespace
checks passed. The final CAD and GUI reruns include the prepared-bore cutter-fit
check added after the workspace run began.

The supplied local `thread.ocam` was also opened and exercised with the thread
regression: generation, save/reopen and all eight posts passed after selecting a
thread mill and explicit nominal thread parameters. The source project was not
rewritten.
