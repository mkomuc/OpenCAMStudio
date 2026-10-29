# Profile milling

Use Profile to cut along a drawing contour or supported planar CAD boundary.
Choose the cutter in **Tool**, the boundary and cutting side in **Geometry**,
and review **Heights**, **Passes** and **Linking** before applying the operation.
[Operation compatibility](operation-compatibility.md) describes supported CAD
selections. Hover over the corresponding parameter labels for explanations and
diagrams of cutting sides, allowances, depths, tabs, leads and height planes.
Enable tooltips in the view preferences to show this help.

## Passes and entry

Passes controls axial stepdown, radial finishing passes, finishing feed, spring
repeats and execution order. Even stepdowns distribute depth evenly; smaller
bottom increments can apply to roughing too or only to the final wall passes.
Stock allowances retain material on the wall and floor. Drawing-based radial
roughing requires one closed outside contour and enclosing stock.

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

Linked CAD profiles do not support radial roughing, controller compensation or
taper. Unlinked computed profiles can use a stepped taper with zero or negative
angle: higher cuts remove more material. Positive angles would undercut and are
rejected. Taper cannot be combined with tabs.

Rest machining, continuous tapered-surface finishing, paired thin-wall scheduling
and per-operation indexed orientations are unavailable. Use separate setups for
other machining sides.
