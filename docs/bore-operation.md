# Bore milling

Bore mills the wall of an accessible circular hole or boss with an end mill.
Choose **Bore** in the milling ribbon, select circular drawing loops or qualified
CAD cylindrical faces, and choose a cutter smaller than the hole. It uses the
standard Tool, Geometry, Heights, Passes and Linking tabs. CAD selections supply
the diameter, internal/external side, top and bottom; reselection refreshes those
dimensions without discarding cutting settings. Multiple selected features must
share diameter, side and cutting heights.

Cutter suitability is advisory. Pointed, rounded and other cutters can follow
the same nominal bore path; their actual profiles determine the simulated cut.
Flank, floor and reach mismatches warn without blocking generation or simulation.
See [advisory verification](geometry-verification.md).

CAD cylinders can have a qualified coaxial rounded entrance opening upward to a
planar rim. Bore cuts only the straight cylindrical wall beneath the entrance.
Capped, incomplete, obstructed entrances and rounded bottoms are rejected.
Drill and Thread accept the same entrance types; Thread requires a thread mill
and an explicit thread specification. See [operation compatibility](operation-compatibility.md).

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
stock. Review cutter reach, fit and safety heights before generating, then inspect
the toolpath and simulation before export.

Compensation is calculated in CAM. Controller/wear compensation, automatic
diameter-range or same-diameter searches, nested containment, independent
vertical/linear lead controls, rapid-to-high-feed conversion, and per-operation
tool-axis overrides are not implemented. Setup WCS orientation is used.
Conical CAD faces are not exposed by the current topology importer, so CAD Bore
selection supports certified cylinders only; it does not infer cones from the
display mesh. Use separate operations for features of different dimensions.
