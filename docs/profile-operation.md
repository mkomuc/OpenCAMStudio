# Profile milling

Use Profile to cut along a drawing contour or supported planar CAD boundary.
Choose the cutter in **Tool**, the boundary and cutting side in **Geometry**,
and review **Heights**, **Passes** and **Linking** before applying the operation.
[Operation compatibility](operation-compatibility.md) describes supported CAD
selections. Hover over the corresponding parameter labels for explanations and
diagrams of cutting sides, allowances, depths, tabs, leads and height planes.
Enable tooltips in the view preferences to show this help.

## Passes and entry

Enable **Roughing passes** for a fixed number of additional contours before the
normal profile or chamfer pass. **Maximum stepover** sets their radial
spacing, and **Number of stepovers** sets the number of additional contours.
They start on the air side and work towards the selected wall. **Smoothing
deviation** controls arc fitting on those roughing contours; it must be positive
and no greater than the stepover. The normal finishing contour retains its own
accuracy setting. This option works with open and closed Inside/Outside chains,
including linked profile geometry. Profile stock-boundary clearing is a separate
option and cannot be combined with contour roughing. Contour roughing also
requires computed compensation and cannot be combined with profile tabs or taper.
For modeled chamfer faces, each additional pass offsets every straight face band toward its air side; the existing stepdown still sets band height. Smoothing deviation has no effect on these straight bands.


Passes controls axial stepdown, radial finishing passes, finishing feed, spring
repeats and execution order. Even stepdowns distribute depth evenly; smaller
bottom increments can apply to roughing too or only to the final wall passes.
Stock allowances retain material on the wall and floor. Drawing-based radial
roughing requires one closed outside contour and enclosing stock. With finishing
enabled, roughing leaves one finish spacing for each finishing pass; the final
pass reaches the profile, subject to the wall stock allowance. With radial
roughing stepover set to zero, a contour roughing pass still leaves this allowance
before the finishing cuts.

Open chains stay open. Linking offers tangent extensions, linear or arc leads,
contour ramps, helix/zigzag entry and alternating depth-pass direction. An entry
that cannot fit the requested angle is rejected. Arc leads support sweeps up to
170 degrees. Exit shape can follow entry shape while using an independent exit
feed. The optional ramp RPM returns to cutting RPM after entry.

For an automatic inside entry, Profile tries edge midpoints when the original
corner cannot fit the requested leads. Selected entry positions are preserved.
A lead that cannot fit inside the contour is omitted with a warning; reduce its
size or select another position.

Holding tabs are configured in Geometry with rectangular or triangular shapes
and count, distance or manual placement. They require compatible closed contours,
computed compensation and no roughing or ramps. Tab placement modes cannot be
combined. See [entry and exit positions](lead-positions.md) for preferred points.

## Compensation and limits

Computed compensation offsets the cutter path in CAM. Controller compensation
requires a supporting post, an Inside/Outside contour, straight descent and linear
engagement/disengagement at least one cutter radius long. Its backplot shows
programmed coordinates; it does not simulate the controller's compensated cutter
envelope. Wear and inverse-wear compensation are unavailable.

Linked CAD profiles do not support stock-boundary clearing, controller compensation or
taper. Unlinked computed profiles can use a stepped taper with zero or negative
angle: higher cuts remove more material. Positive angles would undercut and are
rejected. Taper cannot be combined with tabs.

Rest machining, continuous tapered-surface finishing, paired thin-wall scheduling
and per-operation indexed orientations are unavailable. Use separate setups for
other machining sides.

## Parameter expressions

Profile **Finish spacing** and Profile/Chamfer **Maximum stepover** accept a
number or an Excel-style formula starting with `=`. While the field has focus,
the formula is visible and editable. Press **Enter** or move focus to finish the
edit and display its calculated value. Focus the field again to reveal the
formula. Enter finishes the field without applying the whole operation.

Typing part of a variable name after `=` shows matching variables, filtered as
you type. Click a suggestion, or select it with **↑/↓** and insert it with **Tab**.
Completion works at the caret inside a larger formula and preserves surrounding
operators and arguments. Tab otherwise moves to the next field as usual.

You can always replace the formula with a plain number. This is a fixed override
and stays fixed when the cutter changes. Invalid formulas stay visible for
correction and block Apply. Formulas recalculate using the operation's cutter and
are retained with the saved operation. Apply, Cancel and Undo work as for other
operation settings. Existing numeric values remain unchanged.

Example formulas are:

- Finish spacing (`finishingStepover`):
  `=Math.min(tool_diameter * 0.1; maximumStepover)`
- Maximum stepover (`maximumStepover`):
  `=Math.max((tool_diameter - (2 * tool_cornerRadius)) * 0.95; tool_diameter * 0.20)`

For a flat 6 mm end mill, these yield 0.6 mm and 5.7 mm respectively when both
formulas are enabled. Finishing spacing can also reference a numeric maximum
stepover override, even when the roughing section is collapsed.

Available variables are `tool_diameter`, `tool_cornerRadius`, `maximumStepover`
and (for Profile) `finishingStepover`. Corner radius is zero for a flat end mill,
half the diameter for a ball mill, and the specified radius for bull-nose and
radius mills. Expressions operate on millimetre values and dimensionless numbers;
do not include unit suffixes.

Supported syntax: `+`, `-`, `*`, `/`, `%`, unary signs, parentheses, decimal and
scientific notation, and `min`, `max`, `abs`, `sqrt`, `pow`, `floor`, `ceil`, `round`.
Functions accept an optional `Math.` prefix and comma or semicolon separators.
Unknown names, circular dependencies, division by zero, non-finite results and
nonpositive stepovers show an error and block Apply and toolpath generation.
The evaluator executes only this arithmetic grammar, with bounded expression
length and nesting. Further parameters can reuse the evaluator and add bindings
through `OperationParameter` and the inspector adapter.

See [numeric parameter formulas](parameter-expressions.md) for the parameters supported
across all operation types and the display-only precision setting.
