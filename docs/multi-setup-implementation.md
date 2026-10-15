# Multi-setup workflow implementation

Implemented from [the workflow/UI plan](multi-setup-workflow-ui.md), 2026-10-13.

## Working with setups

Import a STEP model once. The left browser lists physical setups, with operations
directly beneath them in execution order. Add a setup for another machining side,
or duplicate an existing setup to copy its operations and geometry links. Each
setup has independent model targets, work axes, datum, stock, material, safety
heights and NC program name. Selecting an operation activates its parent setup.

New setups start with no model bodies selected. In the Setup tab, click Add,
then click each desired body in the shaded view. Done or Escape finishes picking;
Remove can clear the last body. The model list and viewport highlights preview
this selection until OK. Duplicating a setup retains its selected bodies.
Existing projects migrate their former implicit all-body selection explicitly.
The Models tree groups body instances beneath their imported source model.

The right column holds Setup, Stock and Postprocess tabs. Edits across the tabs
share one draft. OK validates and commits the entire draft as one undoable change;
Cancel restores the committed values in every tab. Tab changes never commit.
Resolve a draft before navigating to another setup or operation.

For WCS orientation, use a selected model instance's coordinate system, or choose
straight model edges for X, Y and Z. Select an axis reference in the right column,
then pick its edge on the model. Each reference can be flipped or replaced.
Two perpendicular directions can determine the third axis. Parallel, skewed,
curved, degenerate, missing and left-handed reference combinations are rejected.
The datum remains independent of axis direction. A model-coordinate-system datum
follows its model instance; an explicit datum remains as entered.

Stock supports a fixed-size box, positioned against the model along each axis,
or a box with independent allowances on all six sides. Material is separate from
stock shape. Model dimensions and containment feedback are in the right column.
The center viewport contains the model and operation previews, including semantic
selection highlights; setup controls and stock/WCS overlays stay out of it.

Select Body, Face, Edge, Loop or Vertex geometry and create a supported operation,
or start an operation and select its geometry. STEP Pocket, Drill, Profile and
Face retain [their existing qualification limits](cad-import-implementation.md).
Eligibility and cutting heights are evaluated in the active setup's axes.

Generate and export the active setup as its own NC program. Switching setups
clears the previous run/export cache; generate the newly active setup before
exporting. Copy/move commands re-resolve linked operations in their destination
setup and remap conflicting tool numbers before committing. Explicit planar
operations require the same axes when transferred between setups.

## Coordinates, persistence and compatibility

Schema 15 stores an ordered setup list with stable setup identities, globally
unique operation IDs and independent CAD-linked-operation markers. Markers make
a missing geometry binding an error, rather than a fallback to cached contours.
Source assets and placements stay shared. Feature resolution rotates placements
into setup axes without changing source entity IDs. Planning and simulation use
setup-local coordinates; backplots and simulation meshes return to source/world
coordinates. Posting subtracts each datum once.

Schema 14 and earlier supported projects migrate to a single setup. Existing
extra work origins remain translation-only compatibility data; migration does
not invent rotational setups or change legacy operation ordering/output.
All setup definitions and geometry links save together in the self-contained
project. Active selection and temporary drafts are view state.

## Verification

Regression coverage includes migration/project validation, independent setup
settings and operations, save/reopen, undo/redo, active-setup cache isolation,
global operation/binding identity, invalid-frame and stock rejection, and tool
conflict handling during transfers. Native OCCT coverage machines the top and
flipped bottom of a STEP box, checking physical cut coordinates, separate NC
programs, one-time datum subtraction and unchanged shared placement.

Native qualification requires the configured OCCT SDK:

```sh
cargo test -p cam-app --features occt --test native_step -- --ignored
```

Cylinders, tubes, stock from another solid and remaining stock from a previous
setup are deferred. Bounds are conservatively derived from exact source body
bounds, independent of display tessellation. The existing heightfield simulation
remains per-setup 2.5D material removal; this change does not add general fixture,
holder or freeform collision verification.

### Browser visibility and expansion

Imported model rows expand/collapse their body instances and provide a whole-model
visibility checkbox and context action. Parent visibility preserves individual
body choices. Selecting Models shows the collection; selecting a model or body
shows that item. Selecting a setup or one of its operations scopes CAD display,
picking, snapping and framing to the setup's model instances (including draft
membership). An empty setup shows no CAD bodies. Add-model picking temporarily
exposes other visible models so bodies can be added to the setup.

Tools (in use) starts collapsed. Operations expand to show their own tool details,
including both tools for a clearing/carving operation in cutting order. Expansion
and visibility are view state and do not change machining inclusion or the project.
