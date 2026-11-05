# Pocket / 2D adaptive stock contours

In a **Pocket** operation, open **Geometry → Stock contours** and enable the
checkbox. It applies to both concentric pocket clearing and engagement-capped
2D adaptive clearing.

With no selection, the material footprint comes from the setup stock box.
Choose **Select…** to use a closed drawing contour, a CAD boundary loop,
connected CAD edges forming a closed chain, or a **Silhouette** projected from
a body. Clicking an edge of the stock box selects setup stock. Use **Clear** or
**Use setup stock** in the selector or Geometry to restore the setup footprint. CAD sources
follow their instance placements; missing or stale sources require reselection.

The stock footprint may be smaller, larger, or a different shape than the setup
stock. At open pocket edges it limits or extends the machining area. Pocket
walls and selected islands remain protected. CAD jobs use the independent
machining projection above the selected floor to preserve rising walls and
open descending exterior walls, including curved model edges. Legacy models
without that projection use conservative bounding footprints. Drawing jobs use the setup stock box for
this distinction. Keep the setup stock box enclosing the model, and select the
pocket floor and its islands as usual. Selecting stock does not replace the
pocket geometry or change heights.

This is a material boundary, not tool containment. Clearing permits cutter
runout beyond the footprint for edge coverage and lead/entry clearance. Wall
stock to leave and finishing allowances still protect pocket walls and islands.
Wall clearance reserves the machining projection error, path simplification,
and grid rounding. Dense obstacle contours are simplified only when the resulting
envelope still contains the original safety envelope.
Every emitted arc and link is checked against the protected walls. If an adaptive
path fails this check, a checked concentric fallback is used with a warning;
generation stops if neither path is safe.
Leave the checkbox **unchecked for closed pockets**.

The selector's OK stages its selection. The operation's OK commits all tabs
together; Cancel discards the edits, and Undo restores the previous operation.
Existing projects open with Stock contours disabled.
