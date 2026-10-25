# Operation patterns

Use a setup or operation context menu to create a rectangular or circular pattern
from one or more operations. Select sources, configure **Placement**, inspect
**Preview**, then choose **Create pattern** or **Apply changes**. Cancel discards
the draft. Sources must belong to the same setup and work datum and cannot already
belong to another pattern. Grouping selected sources places them together at the
earliest selected operation's position in the execution order.

Rectangular patterns use X/Y counts and pitches. Circular patterns use a count,
numeric center, sweep and clockwise/counterclockwise direction in setup XY.
The circular center initially follows the active datum and cannot be picked in
the viewport.

**By instance** runs all source operations at one placement before moving to the
next. **By operation** runs each source at every placement before starting the
next source. Expand a pattern in the tree to edit its source operations. Groups
can be suppressed, dissolved or deleted.

Preview shows transformed paths when a current generated result is available;
otherwise it shows placement markers. Editing the draft does not regenerate paths
on every change.

Patterns repeat calculated paths; they do not independently check model features
at every destination. **3D Adaptive Clearing cannot be patterned**; select multiple
bodies/faces or create separate adaptive operations instead. Repeated Face
footprints, compound two-tool Carve operations and fixture/holder clearance need
separate qualification. Patterns are limited to 10,000 instances and 5,000,000
motion steps; these are resource limits, not performance guarantees.
