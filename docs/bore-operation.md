# Bore milling

Bore mills the wall of an accessible circular hole or boss with an end mill.
Choose **Bore** in the milling ribbon, select circular drawing loops or qualified
CAD cylindrical faces, and choose a cutter smaller than the hole. It uses the
standard Tool, Geometry, Heights, Passes and Linking tabs. CAD selections supply
the diameter, internal/external side, top and bottom; reselection refreshes those
dimensions without discarding cutting settings. Multiple selected features must
share diameter, side and cutting heights.

A cylindrical hole may have a rounded entrance when its exact topology proves
a complete coaxial quarter-torus widening upward to an open planar rim. Bore
machines only the cylinder beneath that entrance. For example, the Ø30 hole in
`testfiles/3d_toolpath_calculation.ocam` has a 4 mm entrance radius above Z16;
its Bore cutting interval is Z16 to Z0, while the opening reaches Z20. Capped,
incomplete, noncoaxial or obstructed entrances and rounded bottoms are rejected.
Drill and Thread use the same rounded-entrance qualification. Thread still
requires a thread mill and an explicit thread specification; see the
[operation compatibility review](operation-compatibility.md).

The operation is based on the supplied Autodesk *Fusion Help — Bore reference*.
It is implemented as a separate `BoreOp` / `BoreStrategy`, not a drill cycle or a
thread-milling operation. Saved projects use schema v26; older projects migrate
without changing their operations.

## Cutting controls

- Pitch specifies axial travel per revolution. Use ramp angle instead to limit
  slope at the smallest compensated orbit, including roughing passes and taper.
- Roughing stepovers add radial helices before the final wall pass. Finishing
  stepover reserves material for an additional full-depth helix. Repeat final
  pass adds a spring pass at the final radius.
- Positive radial/axial stock leaves material on the wall/floor. Negative stock
  cuts beyond the nominal geometry; negative radial stock must be smaller than
  the cutter radius. CAD-linked operations cannot extend below the certified
  face interval with negative axial stock.
- Climb/conventional changes orbital direction, accounting for internal versus
  external features and spindle direction. Helices cut from top to bottom.
- Selected, nearest-neighbour, inside-to-outside, X and Y ordering are available,
  each with an optional reversal. Inside-to-outside uses distance from the mean
  feature centre; nearest-neighbour starts at the first selected feature.
- Straight walls use half-turn helical arcs. Drawing operations can specify a
  taper: positive angles narrow holes downward; negative angles widen bosses
  downward. Opposite slopes would undercut with a cylindrical tool and are
  rejected. Tapered spirals use linear moves bounded by the chosen chord tolerance.
  A smaller pitch improves the stepped tapered-wall finish.

Every helix completes a full circle at its bottom, including fractional final
turns. Tangent semicircular leads enter and leave the wall. Hole leads can reach
the centre; boss leads stay outside the wall. The tool retracts vertically to
clearance before any move between passes or features. Safe distance must fit
between the feature top and the retract plane. Cutting, ramp, lead-in, lead-out
and plunge feeds use the existing operation feed system.

## Current boundaries

This is wall milling in an accessible feature, not clearing a hole from solid
stock. The generator checks tool suitability, flute reach, orbit fit, finite
dimensions, safety heights and path-size limits. Existing simulation and posting
process the resulting controller-neutral motions.

Compensation is calculated in CAM. Controller/wear compensation, automatic
diameter-range or same-diameter searches, nested containment, independent
vertical/linear lead controls, rapid-to-high-feed conversion, and per-operation
tool-axis overrides are not implemented. Setup WCS orientation is used.
Conical CAD faces are not exposed by the current topology importer, so CAD Bore
selection supports certified cylinders only; it does not infer cones from the
display mesh. Use separate operations for features of different dimensions.
