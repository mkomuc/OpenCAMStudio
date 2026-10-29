# STEP/IGES import robustness

## SolidCraft comparison

Reviewed SolidCraft commit
[`f49d5c2f29addc92a4f5720d9681a19df807c741`](https://github.com/tangency-labs/solidcraft/tree/f49d5c2f29addc92a4f5720d9681a19df807c741).
This review informed the implementation; OpenCAMStudio continues to use OCCT
7.9.3 and its existing topology snapshot, rather than importing another kernel.

- SolidCraft's [STEP reader](https://github.com/tangency-labs/solidcraft/blob/f49d5c2f29addc92a4f5720d9681a19df807c741/crates/kernel/src/step_in/mod.rs)
  carries representation units, file tolerance, bounded vertex snapping,
  placements and warnings. Unsupported entities can leave a partial result.
- Its [IGES reader](https://github.com/tangency-labs/solidcraft/blob/f49d5c2f29addc92a4f5720d9681a19df807c741/crates/kernel/src/iges_in.rs)
  sews trimmed surfaces and converts them to STEP for the same import pipeline.
- Its [surface recognition](https://github.com/tangency-labs/solidcraft/blob/f49d5c2f29addc92a4f5720d9681a19df807c741/crates/kernel/src/offset.rs)
  checks points and normals for planes, cylinders, cones, spheres and tori.
  [Construction geometry](https://github.com/tangency-labs/solidcraft/blob/f49d5c2f29addc92a4f5720d9681a19df807c741/crates/kernel/src/surface_kind.rs)
  also recognizes circular edges from samples. Recognition tolerances depend on
  body size.
- Its [healing code](https://github.com/tangency-labs/solidcraft/blob/f49d5c2f29addc92a4f5720d9681a19df807c741/crates/kernel/src/heal.rs)
  normalizes surfaces and edges, merges coplanar faces and collinear edges,
  preserves provenance when requested, and returns the input if healing fails.

## OpenCAMStudio changes

The OCCT bridge now preserves native ellipse, parabola and hyperbola trims as
exact rational Bezier spans. The original edge orientation, placements and
endpoints survive conversion; the converted spline's own parameter interval is
used. Profile extraction can use these boundaries through its existing bounded
curve approximation.

Spline extrusion recognition examines every control row. Higher degrees and
separable positive rational weights are allowed when the rows are monotone axial
translations of one profile. Circular sections additionally require a
positive-weight Bernstein residual bound for the complete curve and checks of
traversal, endpoints and the extrusion's perpendicular plane. Bulged walls and
elliptic sections retain their original representation.

Native cones and spheres retain analytic metadata. This improves inspection and
diagnostics; it does not add tapered Bore, conical Chamfer, or spherical feature
machining. Freeform faces retain the original CAD tessellation, and import
warnings identify missing analytic representations. Surface recognition and
operation compatibility remain separate decisions.

STEP and IGES share closed-surface solidification. Explicit B-rep solids and
their void shells are preserved. Free faces are sewn using the requested
fallback precision plus the OCCT-transferred local edge tolerances. Closed,
valid shells can form solids; open shells, stray wire geometry and ambiguous
nested free shells fail explicitly. Required components are never silently
dropped from a machining model.

Invalid transferred solids receive a repair attempt on an owned copy, bounded
by existing source tolerances and the requested fallback precision. The result
must remain one solid, pass OCCT validation, keep the counts of faces, wires,
edges and vertices, and preserve bounds within that budget. Otherwise the
original remains marked invalid. Neither display mesh tolerance nor body size
sets the repair budget. Repair results and representation limits are reported in
the existing import diagnostics.

Existing saved projects retain their embedded snapshots. Reimport or replace a
model, then review linked selections, to apply these changes.

## Regression coverage

`cam-kernel/tests/iges.rs` and `robust_import.rs` cover exact spline cylinders,
rotated geometry, degree elevation, conic equations and trims, cone/sphere
metadata, closed STEP shells, incomplete exports, and nested-shell rejection.
Barrel walls and elliptical holes exercise false-positive rejection. Fixtures
are authored by `fixtures/generate_iges.cpp` against OCCT 7.9.3.

Application tests exercise conic Profile geometry and Bore/Drill/Thread
recognition, including project save/reopen. An explicit local regression covers
`testfiles/ColdEnd.iges`, Face 343: diameter 4.2 mm, Z=12–18 mm.
