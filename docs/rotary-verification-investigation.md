# Stepped-shaft rotary verification investigation

The supplied `rotary_test.ocam` contains a shaft with a radius-15 band from
world Z=0 to 10 and a radius-10 shaft from Z=10 to 40. Its setup makes world Z
the A-axis direction. The blank has radius 16 and extends one millimetre beyond
each end. The tool is a diameter-6 end mill with 13 mm flutes and 57 mm length.

## Correctness

The operation selected both cylindrical faces, but its wrap reference had radius
10 and an entered depth of 4. A single developed pocket cannot use two different
radial floors. Previously, failure to derive a constant CAD floor silently used
the entered depth, cutting both selections below the model. Mixed cylinder radii
now produce an explicit diagnostic. The corrected project uses one operation per
diameter, with Selected Contours as each radial floor.

Stock Top now measures the actual blank radius. For this project its physical
radius is 16; retract and clearance radii are 21 and 31. World stock references
are resolved before the worker's geometry transform to prevent applying the setup
rotation twice. Model Top retains the selected cylinder's developed surface.

A periodic cylinder seam is not a pocket wall. Exact developed rectangular bands
replace densely sampled collinear seam geometry, and the periodic angular ends
are extended before planar cutter compensation. Axial ends remain bounded. This
lets the cutter traverse the complete circumference instead of leaving a wedge.

The collision tolerance previously also filtered material removal. It left thin
stock films that became scattered caps and walls in the mesh. Removal now checks
all remaining intervals independently of collision tolerance. Stock views also
omit overlapping solid CAD faces for opaque stock while retaining model reference
edges. Transparent stock keeps the solid model visible for comparison.

## Reproduction and independent tests

The embedded analytic fixture in `crates/cam-app/tests/fixtures/stepped_rotary.ocam`
requires neither the user's project path nor a native CAD SDK. The integration
tests reject mixed radii, resolve the rotated setup, verify R15/R10 floors,
check full angular coverage and clearance, inspect every stock side-surface
vertex within the tested shaft bands (0.3 mm tolerance at accuracy 0.5), and post
LinuxCNC output. A separate kernel test verifies a complete annular cut without
internal caps. Optimized and unbounded searches are compared for identical stock
meshes, collisions, removed volume and force samples for both A and B axes,
including long shanks, air moves and colliding indexes. A separate A/B toolpath
test checks periodic seam coverage.

```sh
cargo test -p cam-app --features gui --lib --test rotary_stepped --test rotary_wrap
cargo test -p cam-sim --lib
cargo test -p cam-toolpath --lib periodic_bands
cargo run -p cam-app --example rotary_verify_bench -- project.ocam --split-cylinders --accuracy 0.5
```

The benchmark loads and executes a project through the normal worker/controller,
reports wall time by worker phase, and can save a corrected copy with `--save`.
The original project is not modified.

## Where verification time goes

The original run reached the 180-second benchmark timeout in Verification.
Generation and mesh/render preparation are small compared with rotary stock
intersection and subtraction. These timings are cold runs, excluding compilation;
changing the selections also changes the toolpath, so this is not a same-program
speedup comparison.

Observed local development-build timings for the corrected two-operation job
(2,661 program steps, no diagnostics or reported collisions):

| Accuracy setting | Grid spacing | Generation | Verification | Render preparation | Total |
| --- | --- | --- | --- | --- | --- |
| 0.5 | 0.14 mm | 0.08 s | 8.31 s | 0.06 s | 8.46 s |
| 1.0 | 0.07 mm | 0.33 s | 92.03 s | 0.30 s | 92.66 s |

These are wall-clock observations on the shared development machine, not isolated
benchmark averages. Fine verification remains expensive: about 99% of the normal
accuracy run is Verification, rather than generation or mesh preparation. The
coarser setting is useful for iteration; its grid spacing is a different accuracy
tradeoff, not an equivalent verification result.

The kernel samples each motion and intersects vertical stock rays with the
rotated flute/neck/shank. Work is roughly proportional to the number of poses
times the number of candidate columns. Halving grid spacing approximately
quadruples column count and doubles pose count. Thus twice the accuracy can cost
about eight times as much, even when toolpath generation stays unchanged.

Implemented reductions:

- Compute rotation sine/cosine and the transformed ray direction once per pose.
- Clip the tool axis against the stock box expanded by the tool radius before
  projecting its search window; reject air-only poses.
- Bound angular travel using distance from the rotary axis, excluding axial
  shaft length from the pose spacing calculation.
- Reject non-cutting tool sections outside the stock's enclosing cylinder once
  per motion, before individual ray tests.
- Simplify periodic cylinder boundaries and avoid unnecessary passes above an
  incorrectly resolved stock top.

## More efficient methods

FreeCAD's legacy CPU simulator stores a single height per XY column, samples tool
motion and caches a radial tool profile. This is useful for ordinary top-down
machining, but does not by itself preserve the multiple material intervals needed
for general indexed and rotary stock. See [FreeCAD VolSim.h](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/Mod/CAM/PathSimulator/App/VolSim.h).

FreeCAD also has an OpenGL simulator that renders swept-tool segments using
depth/stencil passes. This is a useful reference for fast visual playback. Its
image-space result alone is not a replacement for our headless remaining-stock,
collision, stock-transfer and removed-volume queries. See [FreeCAD MillSimulation.cpp](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/Mod/CAM/PathSimulator/AppGL/MillSimulation.cpp)
and [SimShapes.cpp](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/Mod/CAM/PathSimulator/AppGL/SimShapes.cpp).

GPU triple-dexel simulation has published support for arbitrary tool orientations:
three orthogonal interval grids, cutter swept-volume generation, parallel stock
subtraction and display updates. See [Inui and Umezu, 2010](https://www.jstage.jst.go.jp/article/jjspe/76/3/76_3_361/_article/-char/en).
Their reported timings are for their own experiments, not a performance promise
for this application.

Recommended development order:

1. Intersect stock dexels with bounded cutter sweeps per motion segment, reducing
   the pose-count multiplier. Preserve conservative collision checks separately
   from cutting subtraction and validate flat/ball/bull/conical tools.
2. Use tiled column storage and pooled interval lists to reduce hashing and
   allocation in the subtraction loop. Parallelize independent columns within a
   time-ordered sweep, and remesh only affected tiles.
3. Evaluate GPU multi-dexel subtraction for full verification. Keep its collision
   and volume results comparable with the CPU reference tests.
4. Consider a separate GPU swept-volume display for interactive playback.
   Keep clear which results are visual and which have completed verification.

A polar radius grid could accelerate simple exterior cylindrical wrapping, but
cannot represent every cavity or cut reachable with indexed machining. It would
be a specialized backend rather than a general replacement for material intervals.
