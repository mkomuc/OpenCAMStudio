# Operation patterns

Use a setup or operation context menu to create a linear or circular pattern
from one or more operations. Select sources, configure **Placement**, inspect
**Preview**, then choose **Create pattern** or **Apply changes**. Cancel discards
the draft. Sources must belong to the same setup and work datum and cannot already
belong to another pattern. Grouping selected sources places them together at the
earliest selected operation's position in the execution order.

**Pattern type** switches between linear and circular placement.

Linear patterns have one direction and an optional **Additional direction**.
For each direction, select a straight model or stock edge parallel to setup XY.
Without an edge pick, directions follow setup X and Y. Rectangular patterns have
no direction-angle field. Set **Quantity**, **Distance** and **Reverse direction**
independently. **Spacing** interprets the distance as
the adjacent pitch; **Extent** divides the total distance by quantity minus one.
**Symmetric** alternates placements on either side of the original: 0, +1, −1,
+2, −2 pitches. Quantity includes the original; even quantities place the extra
instance on the positive side. Two active directions must not be parallel.

Circular patterns use **Angle**, **Instance count** and **Reverse direction**
(clockwise viewed from setup +Z). With **Extent**, the angle spans the first and
last instances; 360° distributes instances around a complete circle without a
duplicate endpoint. With **Spacing**, the angle is the increment between adjacent
instances and the resulting span must be less than 360°. The center initially
follows the active datum. Pick a circular model edge or a straight model axis
parallel to setup Z to set the center, or enter **Center X/Y** directly. Patterns
remain in setup XY; tilted axes and directions outside that plane are rejected.
Edge picks retain their references and resolved placement values; changing CAD
geometry later requires picking the direction or axis again.
Click **Edge selected** to highlight the stored edge in the viewport. This keeps
the selection and placement intact. Use **Reselect** to pick a replacement, or
**Clear** to remove the reference (restoring setup X/Y for linear directions).

**Keep original** includes the source placement. Turning it off omits placement
zero without changing the remaining positions or quantities. **Reverse instance
order** reverses placement traversal while retaining stable instance IDs.

**By instance** runs all source operations at one placement before moving to the
next. **By operation** runs each source at every placement before starting the
next source. **By tool** sorts sources by tool number, retaining their relative
order within each tool, and runs each source at every placement. Expand a pattern
in the tree to edit its source operations. Groups can be suppressed, dissolved or
deleted.

A source operation's selected **Lead-in position** applies only to its first
executed instance. Later instances use automatic entry placement and the normal
linking moves. The first instance follows the configured traversal order, including
**Reverse instance order** and **Keep original**. Lead geometry and selected
lead-out positions retain their operation settings.

The viewport previews the pattern live on every editor tab. Source toolpaths are
calculated in the background when the editor opens, even before a full job has
been generated. Changes to counts, distances, directions, type and original/order
options immediately reposition those cached paths. Changing the source operations
refreshes the source calculation. Placement markers appear while paths load, and
camera framing includes repeated paths beyond the original stock. Preview does
not apply the draft, simulate stock removal or replace the export program.

Patterns repeat calculated paths; they do not independently check model features
at every destination. **3D Adaptive Clearing cannot be patterned**; select multiple
bodies/faces or create separate adaptive operations instead. Repeated Face
footprints, compound two-tool Carve operations and fixture/holder clearance need
separate qualification. Patterns are limited to 10,000 instances and 5,000,000
motion steps; these are resource limits, not performance guarantees.
