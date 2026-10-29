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

Projects use schema 31; earlier projects migrate without changing their operations. The operation is also available through MCP as kind `Parallel`.

The supplied tray regression can be run locally:

```sh
cargo test -p cam-app --test parallel_project -- --ignored --nocapture
```

It opens `testfiles/tray.ocam`, uses a 6 mm ball nose, generates 1 mm parallel finishing rows, validates the motions, and saves a separate `testfiles/tray-parallel.ocam` review project.
