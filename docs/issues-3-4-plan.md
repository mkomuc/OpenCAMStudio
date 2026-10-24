# Implementation plan: issues 3 and 4

## Issue 3: 3D Adaptive Clearing

- Add an explicit tool containment choice (Inside, Tool center on boundary, Outside) and signed additional boundary offset to adaptive parameters and geometry settings. Persist defaults for existing project files and thread both settings through XY footprint generation.
- Accept vertical cylindrical faces only when their CAD topology has complete circular boundary wires; derive their XY footprint from the certified analytic cylinder. Reject open or ambiguous trims.
- Keep full-solid obstacle shadows and existing tolerance/collision protections. Preserve geometry-certified bottom override checks and report unsafe overrides clearly.

## Issue 4: Output Log

- Add a Clear control that clears stored output and the visible snapshot until new output arrives.
- Add human-readable UTC timestamps to output entries and geometry-validation errors, deduplicate before stamping, and use one newline between entries.
- Keep diagnostic sections readable and output copy/select behavior intact.

## Validation

- Verify the `cnc_test` Face 3 topology read-only; test the matching full-circle plus canceled seam and reject incomplete boundary wires.
- Verify signed offsets against emitted cutter-center paths for all containment modes, legacy parameter defaults and serialization round trips, and Output Clear/timestamps/single-line spacing.
- Run workspace compilation and focused/full model, geometry, toolpath, and GUI test checks. The current project has no operation binding for the screenshot selection, so the face selection is covered by a synthetic fixture matching its stored topology.
