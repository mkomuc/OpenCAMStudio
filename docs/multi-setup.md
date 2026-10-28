# Setups and work coordinates

New projects start with no setups. Import a STEP model once, select a model or
body in the project browser or 3D view, and create a setup. A selected model adds
all its bodies; selected bodies add just those instances. With no model or body
selected, the setup starts with no model targets.

The left browser lists physical setups, with operations
directly beneath them in execution order. Add a setup for another machining side,
or duplicate an existing setup to copy its operations and geometry links. Each
setup has independent model targets, work axes, datum, stock, material, safety
heights and NC program name. Selecting an operation activates its parent setup.

In the Setup tab, click Add to include more bodies,
then click each desired body in the shaded view. Done or Escape finishes picking;
Remove can clear the last body. The model list and viewport highlights preview
this selection until OK. Duplicating a setup retains its selected bodies.
The copy is inserted immediately after its source and automatically uses **From
previous setup** stock. Choose a box in its Stock tab to start with a fresh blank.
The Models tree groups body instances beneath their imported source model.

The right column holds Setup, Stock and Postprocess tabs. Edits across the tabs
share one draft. OK validates and commits the entire draft as one undoable change;
Cancel restores the committed values in every tab. Tab changes never commit.
Resolve a draft before navigating to another setup or operation.

Double-click a setup (or choose **Edit setup** from its context menu), then open
the **Setup** tab's **Tool change** section. **Use machine maximum Z** is enabled
by default; disable it to enter a **Tool-change height** in millimetres relative
to the base WCS origin. Click **Apply setup** to save. This height is used before
tool changes and manual re-fixturing and should be at or above every operation's
clearance plane. The planner raises lower values to clearance, with a warning
for differences greater than 0.000001 mm; smaller CAD padding is clamped silently.

The Setup tab combines orientation and datum in **Workpiece coordinate system
(WCS)**. The Orientation menu offers model orientation, Z/X, Z/Y and X/Y axis
pairs, plus individual selected axes. Choose straight model or stock edges for
axis references; each selected direction can be flipped, cleared or replaced.
Two perpendicular directions determine the third axis. Parallel, skewed, curved,
degenerate, missing and left-handed reference combinations are rejected.

The Origin menu offers model origin, a selected point, a stock-box point, a
model-box point, two points or numeric coordinates. For either box option, click
Point on box and choose a corner, edge midpoint, face center or box center in the
view. Box points remain selectable with drawing snapping disabled. Box and picked
points capture an explicit datum; they do not follow later stock/model resizing.
Changing orientation preserves an explicit datum's world position. Model origin
uses the selected model instance's origin. All WCS edits preview in the same setup
draft and commit with OK or revert with Cancel.

Stock supports a fixed-size box, positioned against the model along each axis,
or a box with independent allowances on all six sides. Material is separate from
stock shape. After the first setup, **From previous setup** uses the remaining
material after the immediately preceding setup's included operations. Generating
or simulating recalculates the required earlier setups automatically, including
chains of inherited stock. Reordering setups changes the source. Upstream edits,
suppression and tool changes invalidate the inherited result. Separate box stock
starts a new stock chain. Work axes can change between setups; transferred stock
retains cavities when the workpiece is turned over. Resolve any upstream generation
errors or simulation collisions before using its stock. The first setup needs box
stock and cannot inherit stock.

Model dimensions and containment feedback are in the right column.
The center viewport contains the model and operation previews, including semantic
selection highlights; setup controls and stock/WCS overlays stay out of it.

Select Body, Face, Edge, Loop or Vertex geometry and create a supported operation,
or start an operation and select its geometry. STEP Pocket, Drill, Profile and
Face retain [their existing qualification limits](operation-compatibility.md).
Eligibility and cutting heights are evaluated in the active setup's axes.

Generate and export the active setup as its own NC program. Switching setups
discards the previous generated result; generate the newly active setup before
exporting. Copy/move commands re-resolve linked operations in their destination
setup and remap conflicting tool numbers before committing. Explicit planar
operations require the same axes when transferred between setups.

## Limits

Initial stock is a box. Cylindrical stock and stock from another solid are not
available. Material-removal simulation uses a heightfield with bounded material
intervals for transferred cavities. Stock transfer is sampled at the simulation
accuracy and stock bounds/heights use the enclosing original blank. Simulation
does not verify fixtures, holders or an entire machine assembly.

## Browser visibility

Imported model rows expand/collapse their body instances and provide a whole-model
visibility checkbox and context action. Parent visibility preserves individual
body choices. Picking geometry in the 3D view highlights its body in the browser
and expands the parent model. The browser title is the project filename without
`.ocam`, or Untitled before the first save. Selecting Models shows the collection; selecting a model or body
shows that item. Selecting a setup or one of its operations scopes CAD display,
picking, snapping and framing to the setup's model instances (including draft
membership). An empty setup shows no CAD bodies. Add-model picking temporarily
exposes other visible models so bodies can be added to the setup.

Tools (in use) starts collapsed. Operations expand to show their own tool details,
including both tools for a clearing/carving operation in cutting order. Expansion
and visibility are view state and do not change machining inclusion or the project.
