# 3D Parallel finishing

Create **Machining → 3D Parallel**, choose a ball nose end mill, then select solid bodies or individual faces. The independent CAD machining projection drives the path; display tessellation does not affect it. All bodies included in the setup remain collision obstacles.

On **Passes**, set horizontal **Stepover**, **Pass angle** (degrees counterclockwise from setup X), and **Tolerance**. **Both ways** alternates the travel direction on successive rows; disabling it makes every row run in the positive pass direction. Rows follow the model in Z. Cutting and plunge feeds are on **Tool**.

**Geometry** offers Inside/Center/Outside containment against the selected geometry's bounding rectangle in the pass coordinate system, plus an additional offset. Contact-only trimming splits rows outside selected face footprints. Positive Inside offsets shrink the rectangle; positive Center/Outside offsets expand it. Holes and gaps may split a row into multiple passes. Radial and axial stock allowances are supported; negative allowances are rejected.

On **Lead-in and lead-out moves → Linking moves**, enable **Keep tool down** to
allow short links between neighboring surface passes. Set **Lift height (mm)**
independently. **0 mm** continues the finishing cut between eligible passes,
following the model in Z and removing the intervening scallop without an added
lift or re-entry plunge. The transition uses the same surface-contact calculation,
tolerance, stock allowances and swept-cutter checks as the scan rows.
A positive value lifts
to the higher of the two pass endpoints plus this distance, traverses at cutting
feed, then descends at plunge feed before cutting resumes. The complete lift,
traverse and descent are checked against all setup bodies using the ball and
cylindrical cutter envelope, including obstacles between endpoints. Links are
limited to 1.5 times the horizontal stepover and two cutter-envelope diameters.
The link must stay over the selected surface. Gaps, split rows, long returns and
blocked links use full clearance. A lift reaching clearance uses the normal
clearance rapid. The first approach and final retract also use full clearance.
Raised transitions are non-cutting traverses. Verification checks them against
remaining stock; a blocked traverse is replaced with a clearance move and
controlled plunge, then verified again. Zero-lift surface transitions are cutting
moves, so they machine material between finishing rows instead of requiring that
material to have been cleared already. Genuine gaps and unresolvable surface
discontinuities still require clearance.

With **Keep tool down** unchecked, every pass retracts to full clearance regardless
of Lift height. The checkbox defaults to off in older projects. Both settings are
saved with the operation and support Apply, Cancel and undo. See
[lift height and stay-down moves](lift-height.md).

Clearance must exceed the model, allowances and stock. This is a finishing operation for pre-roughed stock. Zero-lift transitions continue surface finishing; raised links are non-cutting feed moves checked against remaining stock. This operation does not calculate roughing entries or rapid links below clearance. The ball and cylindrical cutter envelope are checked against the CAD projection, including between contact samples. Wider shafts, holder collision checks, flat/bull nose cutters, silhouette/custom boundaries, slope filters, multiple depths, and up/down-only passes are not implemented.

To generate sampled inward offsets instead of rows, choose **3D Parallel / Scallop**
and enable **Scallop (surface offsets)** on Passes. Stepover is the target distance
along the sampled cutter-center surface. The distance field uses an eight-neighbor
grid with at least four samples per stepover (and at most 0.5 mm spacing), so the
spacing is approximate and depends on grid direction. Contours begin half a stepover
inside the sampled boundary. The selected face footprint limits contact; the containment rectangle and stock allowances use
the same rules as Parallel. The program reports a warning because vertical walls,
small islands, gaps and narrow features can be missed by this height-field method.
Check material removal in simulation. It does not implement Fusion's rest machining,
slope filters, holder collision detection, climb/conventional direction, or its other advanced
Scallop settings. A ball nose cutter and pre-roughed stock are required.

When the last nominal offset grazes an enclosed boundary, it can break into many
tiny fragments. Where the sampled remaining area can still be covered within one
stepover, Scallop moves that final offset slightly away from the boundary to reduce
fragmentation. If that coverage bound cannot be met, the original offsets
are retained. This uses the same sampled approximation as the rest of Scallop.

On **Passes**, **Inside/Outside direction** orders the surface-offset passes:
**Outside → in** advances from the exterior boundary of each selected region;
enclosed holes do not create a competing inward front. **Inside → out** visits
those offsets in reverse. A separate selected island inside a hole has its own
outer boundary. **Shortest machining distance** keeps the outside-in stepover
sequence and uses a bounded greedy search to reduce estimated XY repositioning
among passes at the same level. It does not guarantee a globally shortest route.
When no lead-in position is specified, it may move a closed pass's start or
reverse an open pass to reduce travel. A selected lead-in position takes
precedence over that adjustment. Ordering does not relax the existing cutter,
selection, or stock-link checks, and it does not calculate chip load or rest stock.

On **Lead-in and lead-out moves**, pick a **Lead-in position** on the model. Each
closed contour starts at the sampled point nearest that setup-XY position; an open
pass starts at its nearer endpoint. This is a preference, not a forced plunge at
the selected point. Clear it to restore automatic placement.

**Retraction policy** controls links between Scallop passes. **Full retraction**
uses clearance for every pass. **Minimum retraction** uses a conservative raised
feed link above nearby protected CAD geometry, with the specified **Safe distance**.
**Shortest path** first tries a direct checked feed link; if it cannot clear the
model it tries the raised route. Either option falls back to full clearance if the
route cannot be certified or exceeds the local query limit. The first approach and
final exit always use clearance. These are feed moves, so they avoid diagonal G0
doglegs. Remaining stock is checked during job verification; blocked non-cutting
links are replaced by clearance moves and verified again.

On **Passes**, enable **Smoothing** and set its separate **Smoothing tolerance**
to reduce the generated motion count. The filter combines nearly straight 3D
segments and fits XY arcs on planar portions when selection coverage and swept
cutter checks certify them. Other portions remain linear. Filtering is bounded
against the original path by the smoothing tolerance; add it to the machining
tolerance when assessing total deviation. With smoothing off, all certified sampled
points are emitted. Small segments seen in the display therefore also appear in
the output toolpath; the display is not generating extra machining moves.

Projects use schema 36; earlier projects migrate without changing their operations.
The operation is also available through MCP as kind `Parallel`; the `scallop`
parameter selects sampled surface offsets. Its `scallop_order` parameter accepts
`outside_in`, `inside_out`, or `shortest`.

The supplied tray regression can be run locally:

```sh
cargo test -p cam-app --test parallel_project -- --ignored --nocapture
```

It opens `testfiles/tray.ocam`, uses a 6 mm ball nose, generates 1 mm parallel finishing rows, validates the motions, and saves a separate `testfiles/tray-parallel.ocam` review project.
