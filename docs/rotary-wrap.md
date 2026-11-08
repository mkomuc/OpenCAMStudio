# Fourth-axis wrap milling

Profile and Pocket operations can wrap a planar toolpath around a cylinder. Pocket's
engagement-limited **2D adaptive clearing** uses the same wrapping workflow. This
implements the cylinder selection → unwrap boundary → compute planar passes →
rewrap toolpath workflow shown in the supplied *Fusion Rotary Wrap Toolpath – 4Th
Axis Milling* tutorial. It is cylindrical milling, not general four-axis surfacing.

## Workflow

1. Align the cylinder with setup **X for A** or **Y for B**, with the cutter along
   setup Z. Set the work datum to the physical rotary centre.
2. Create or edit a Profile or Pocket operation. On **Multi-axis**, enable
   **Machining Type → Wrap**. Cutting settings and height definitions stay in their existing tabs when switching modes.
3. Choose **Wrap Cylinder → Select face…**, then click an exterior cylindrical CAD face.
   This derives the radius, centre and rotary axis. Radius is read-only.
   **Offset** changes the wrapping radius radially;
   the effective radius is radius plus offset.
4. On **Geometry**, Pocket uses the same contour selection list and geometry
   settings as a 3-axis pocket. Add a face or chain, choose face, loop or edge
   selection in its settings, then click **OK** to stage the choice. Multiple
   closed regions and nested islands are retained. Profile provides separate
   Loops, Edges and Faces selection controls. For drawings, select the ordinary
   operation geometry; its second coordinate is a circumference distance in
   millimetres, not degrees.
5. Set cutting depths, passes, leads and cutting data normally. Heights are in
   the **unwrapped plane**: by default the cylinder surface is Z=0, clearance is
   positive and cutting depths are negative. The Heights tab previews these references
   as concentric cylindrical surfaces and displays their radii. Clearance must be above the surface. A depth reaching the rotary centre is
   rejected. Use computed cutter compensation.
   Model top means the developed surface; Stock top uses the blank's outer
   radius (exact for a coaxial round blank). Origin means coordinate zero.
   Selected contours uses a cylindrical CAD boundary's radial height. Split
   faces at different radii into separate operations; nonconstant CAD floors
   require an explicit radial height reference. Drawing-only operations retain
   their entered depth. Enter radial
   offsets relative to these references; physical stock/model bottoms and picked
   Cartesian height geometry are unavailable.
6. Generate the toolpath. The backplot keeps the workpiece stationary and shows
   the cutter path around it; playback rotates the displayed cutter direction in
   that material frame. Choose **LinuxCNC**, **Fanuc**, or **Haas** for NC export.

For A, drawing X remains axial and drawing Y wraps around X. For B, drawing Y
remains axial and drawing X wraps around Y. The sign follows right-hand workpiece
rotation. Rotary zero is derived from the cylinder; angles remain continuous across ±180°
and across multiple turns. All wrapped and indexed operations in one setup share a physical axis
and centre, while their cylinder radii/offsets can differ. Ordinary machining
after a wrapped operation indexes back to rotary zero at tool-change clearance.

**Unwrap Geometry** develops selected cylindrical CAD boundaries before calculating
passes. Disable it for CAD boundaries already drawn in the flat XY plane. Drawings
already provide developed geometry. Geometry selection stays in the Geometry tab;
feeds, passes and linking stay in their usual tabs.

## Geometry and motion

CAD development uses exact boundary curves, sampled independently of display
tessellation. Missing entities and stale asset revisions require reselection.
The selected cylinder reference is resolved again when generating. Periodic
cylindrical floor faces with an explicit CAD seam develop into full-width bands.
The seam is periodic, so pocket passes cover the complete turn without treating
it as a physical wall. Axial ends and islands remain machining boundaries.
A single circular loop that winds around the cylinder cannot alone bound a finite
planar pocket; select its seamed floor face or split the boundary into patches.
Select connected edges to form
open profile chains, or closed loops/faces for pockets. Cones, oblique rotary
axes and general surface finishing are outside this mode.

The wrap radius sets the developed circumference. At a cut depth the cutter
moves at that depth's actual radial distance. Planar arcs become simultaneous
linear/rotary blocks, subdivided by wrap tolerance and angular bounds. Patterns
are expanded in the developed plane before wrapping. Tags, operation identity
and pattern ownership remain attached to the resulting moves.

Feed values remain material-path mm/min. Supported posts emit `G93` inverse-time
feed, with an `F` word on every rotary cutting block, and restore `G94` before
ordinary motion or program end. XYZ coordinates are translated by the work datum;
rotary degrees are unchanged. Other posts explicitly reject rotary output.
The machine model currently has no rotary speed/acceleration/travel limits, so
processing-time estimates omit those constraints.

## Indexed 3+1 machining

On an operation's **Multi-axis** tab choose **3+1 Indexed**, select A (X) or B (Y),
and enter the rotary centre and absolute angle in degrees. This mode is available
for every ordinary machining operation. It locks the angle during cutting; Wrap
continues to vary the angle along developed contours. Use computed compensation.
CAD selections are resolved after rotating their placements to the indexed machine
frame. Drawing coordinates and manually entered heights describe that frame.
Patterns repeat in the indexed machining plane. An operation cannot be both
indexed and wrapped. Index settings persist through save/open, duplication and undo.

The planner retracts to tool-change clearance before indexing, then approaches the
operation. Returning to ordinary three-axis cutting indexes back to zero. All
indexed and wrapped motions in a setup must use one physical rotary axis and centre.
Set tool-change clearance above the entire rotating stock and fixture enclosure;
the stock and fixture checks report obstructed index moves.

## Stock and simulation

Choose **Fixed Round** or **Relative Round** stock and align its axis with the rotary
axis using the stock's origin-axis or edge selection. Set the blank diameter, length
and centre to the actual workpiece. Tube stock also retains its internal bore. Wrap
radius is a toolpath parameter; it does not define the input stock.

Axis-aligned cylindrical and tubular blanks are initialized analytically. Oriented
round blanks, selected solids and inherited stock use their closed solid surfaces.
The remaining solid retains disjoint material intervals, so cutting from multiple
indexed sides preserves the other sides, internal voids and previously cut cavities.
The solid stays in the material frame through indexing; it is not repeatedly
resampled into a new heightfield at each angle.

Rotary and mixed jobs now simulate material removal, rapid and non-cutting stock
contacts, non-cutting tips, necks and shanks. Cutter profiles include flat, ball,
bull nose, conical, rounded V tips and finite thread teeth. Fixtures rotate with the
workpiece and use conservative envelopes of the whole tool. Fixture-aware automatic
link rerouting is limited to ordinary three-axis jobs; rotary fixture contacts must
be resolved by changing geometry, placement or clearance.

Playback uses the same simulation, including partially completed rotary moves.
Forward playback reuses completed prefixes; seeking backward or editing a prefix
restores the input blank and replays the requested work. Machined surfaces keep
operation and motion ownership. Force analysis uses actual removed volume and the
programmed block duration, with the same material/tool data requirements as ordinary
machining. Subsequent setups can inherit a collision-free rotary result.

Accuracy is limited by the stock column spacing and sampled cutter poses. Relative
material travel between poses is at most one quarter of the simulation spacing;
raise simulation accuracy to resolve small features. Collision checks use the physical
cutter at these poses. Stock contacts stop removal for the contacting motion and block
NC export. Cancellation or an exceeded stock/pose budget produces an incomplete run,
never an unverified clearance result. These checks do not model a machine's chuck,
rotary table or holder unless included as fixture geometry. General five-axis motion
and machine rotary speed/acceleration/travel limits remain outside this mode.
Pocket Stock Contours remain unavailable for simultaneous Wrap operations.

## Example

```sh
cargo run -p cam-app --example rotary_wrap -- /tmp/rotary-wrap.ocam
cargo run -p cam-app --features gui -- --setup /tmp/rotary-wrap.ocam
```

The example saves an unwrapped drawing pocket and its wrapped LinuxCNC program.
The example uses a cylindrical blank and requires a collision-free stock simulation
before exporting NC output.

The corresponding reference behavior is described in Autodesk's
[4th Axis Wrap Toolpath reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID-7FF25192-AED7-4230-92DC-E22ADF15D498.htm).
Feed-mode semantics follow the LinuxCNC
[G93/G94 reference](https://www.linuxcnc.org/docs/html/gcode/g-code.html#gcode:g93-g94-g95).
