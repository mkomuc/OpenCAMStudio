# Original STEP acceptance fixtures

Generated for OpenCAMStudio by the checked-in `generate.cpp` using OCCT 7.9.3.
The generator and generated shapes are original project work, licensed
GPL-3.0-only with the repository; they contain no customer/vendor part data.

- `box-mm.step`: 60 × 40 × 10 mm, top Z0, bottom Z−10.
- `pocket-island-holes-mm.step`: same plate; pocket from (10,8) to (50,32),
  floor Z−6; raised 12 × 8 mm island at (24,16); four Ø5 through bores at
  (5,5), (55,5), (55,35), (5,35).
- `pocket-island-holes-inch.step`: identical physical part exported in inches.
- `pocket-island-holes-rotated.step`: plate rotated 0.6 radians about X,
  translated by (100,−50,40) mm.
- `repeated-boxes.step`: two instances of the same box; second at X+80 mm.
- `blended-box-blind-bore-mm.step`: box with 1 mm edge blends and an Ø8 blind
  bore at (30,20), top Z0 and floor Z−2. Qualifies an inside profile of the bore
  floor independently of unrelated blends on the rest of the body.
- `toleranced-spline-prism-mm.step`: 5 mm extrusion of a spline outline with
  endpoints meeting within its declared 0.01 mm uncertainty. It is valid at the
  source tolerance; forcing/clamping tolerances to 1e-6 mm falsely reports
  self-intersecting wires. This qualifies import and selection, not spline machining.

These are AP214 files from one writer. They do not qualify the whole STEP
standard, AP203/AP242, open shells or arbitrary curved surfaces.
