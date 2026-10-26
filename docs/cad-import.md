# CAD import and selection

Use **Home → Import** for DXF/DWG drawings or STEP/STP and IGES/IGS models.
Release packages include STEP/IGES support. For source builds, see
[build instructions](../RUNNING.md). Import runs in the background; warnings and
errors appear in the application. A failed import leaves the project intact.
Resolve active editing drafts before importing.

The Models tree groups each imported model and its bodies. Add the required bodies
to a [setup](multi-setup.md) before creating model-linked operations. Visibility,
hide and isolate affect the view; they do not change which bodies belong to a setup
or which bodies protect material during adaptive clearing.

Select geometry and create an operation, or create an operation and choose its
geometry in the **Geometry** tab. Review the extracted heights, boundaries and
islands before choosing a cutter. Shift-click toggles multiple features;
Command-click also toggles on macOS. Ctrl-click selects a body's XY silhouette
for Profile or Face. Unsupported selections show an incompatibility reason;
see [operation compatibility](operation-compatibility.md).

Geometry lists provide add, remove and settings controls. Settings use OK/Cancel
to stage or discard selections. **Edit / repair geometry** replaces a selection
while preserving machining parameters and height overrides. Blank linked height
inputs follow the selected geometry; explicit values override it within the
supported cutting interval.

Chamfer can use modeled planar bevel faces, boundary loops or individual edges.
For modeled faces, the cutter angle must match the bevel; Stepdown limits each
finishing band. For edges and loops, set Chamfer width explicitly. Engrave follows
open or closed planar paths; Carve uses a closed region and retains its islands.
Thread selects cylindrical bores or bosses; review its nominal size and pitch
using the [thread presets guide](thread-presets.md).

Projects embed imported geometry, so reopening does not require the original
source file. Model replacement is available from the model context menu.
Replacement requires reviewing and reselecting linked geometry before generating
again. Missing or unresolved geometry in an included operation blocks generation
and export. Excluded operations do not block the included job. Older imported
models may need reimporting to enable newer spline or adaptive selections.

The **Output** pane identifies selection and generation errors by setup, operation
and geometry. Drag to select text and use Cmd/Ctrl+C, or choose **Copy output**.
Updates pause while text is selected; clear the selection to resume updates.
