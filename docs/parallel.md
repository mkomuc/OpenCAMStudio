# 3D Parallel finishing

Create **Machining → 3D Parallel**, choose a ball nose end mill, then select solid bodies or individual faces. The independent CAD machining projection drives the path; display tessellation does not affect it. All bodies included in the setup remain collision obstacles.

On **Passes**, set horizontal **Stepover**, **Pass angle** (degrees counterclockwise from setup X), and **Tolerance**. **Both ways** alternates the travel direction on successive rows; disabling it makes every row run in the positive pass direction. Rows follow the model in Z. Cutting and plunge feeds are on **Tool**.

**Geometry** offers Inside/Center/Outside containment against the selected geometry's bounding rectangle in the pass coordinate system, plus an additional offset. Contact-only trimming splits rows outside selected face footprints. Positive Inside offsets shrink the rectangle; positive Center/Outside offsets expand it. Holes and gaps may split a row into multiple passes. Radial and axial stock allowances are supported; negative allowances are rejected.

Every pass approaches at clearance, feeds vertically to its first contact, cuts the surface, and retracts vertically to clearance. Clearance must exceed the model, allowances and stock. This is a finishing operation for pre-roughed stock. It does not calculate roughing entries or stock-aware stay-down links. The ball and cylindrical cutter envelope are checked against the CAD projection, including between contact samples. Wider shafts, holder collision checks, flat/bull nose cutters, silhouette/custom boundaries, slope filters, multiple depths, and up/down-only passes are not implemented.

Projects use schema 31; earlier projects migrate without changing their operations. The operation is also available through MCP as kind `Parallel`.

The supplied tray regression can be run locally:

```sh
cargo test -p cam-app --test parallel_project -- --ignored --nocapture
```

It opens `testfiles/tray.ocam`, uses a 6 mm ball nose, generates 1 mm parallel finishing rows, validates the motions, and saves a separate `testfiles/tray-parallel.ocam` review project.
