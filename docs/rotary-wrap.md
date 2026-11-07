# Fourth-axis wrap milling

Profile and Pocket operations can wrap a planar toolpath around a cylinder. Pocket's
engagement-limited **2D adaptive clearing** uses the same wrapping workflow. This
implements the cylinder selection → unwrap boundary → compute planar passes →
rewrap toolpath workflow shown in the supplied *Fusion Rotary Wrap Toolpath – 4Th
Axis Milling* tutorial. It is cylindrical milling, not general four-axis surfacing.

## Workflow

1. Align the cylinder with setup **X for A** or **Y for B**, with the cutter along
   setup Z. Set the work datum to the physical rotary centre, or enter the axis
   centre relative to your chosen datum.
2. Create or edit a Profile or Pocket operation. On **Multi-axis**, enable
   **Wrap Toolpath (4th axis)**.
   Enabling wrapping initializes height references for the developed plane.
3. Choose **Select Wrap Cylinder**, then click an exterior cylindrical CAD face.
   This derives the radius, centre and rotary axis. Alternatively enter these
   values for an unwrapped drawing. Editing a derived radius/centre detaches the
   cylinder reference. **Cylinder offset** changes the wrapping radius radially;
   the effective radius is radius plus offset.
4. For CAD, select **Loops**, **Edges**, or **Faces**, click the machining
   boundaries, then **Done selecting**. Multiple closed regions and nested islands
   are retained. For drawings, select the ordinary operation geometry; its second
   coordinate is a circumference distance in millimetres, not degrees.
5. Set cutting depths, passes, leads and cutting data normally. Heights are in
   the **unwrapped plane**: by default the cylinder surface is Z=0, clearance is
   positive and cutting depths are negative. **Unwrapped surface Z** can move this
   reference. Clearance must be above it. A depth reaching the rotary centre is
   rejected. Use computed cutter compensation.
   In this mode Stock/Model top means the developed surface, Origin means its
   coordinate zero. Selected contours uses a cylindrical CAD boundary's radial
   height, or the operation depth when no constant-radius boundary is available. Enter radial
   offsets relative to these references; physical stock/model bottoms and picked
   Cartesian height geometry are unavailable.
6. Generate the toolpath. The backplot keeps the workpiece stationary and shows
   the cutter path around it; playback rotates the displayed cutter direction in
   that material frame. Choose **LinuxCNC**, **Fanuc**, or **Haas** for NC export.

For A, drawing X remains axial and drawing Y wraps around X. For B, drawing Y
remains axial and drawing X wraps around Y. The sign follows right-hand workpiece
rotation. **Seam angle** sets rotary zero; angles remain continuous across ±180°
and across multiple turns. All wrapped operations under one datum share an axis
and centre, while their cylinder radii/offsets can differ. Ordinary machining
after a wrapped operation indexes back to rotary zero at tool-change clearance.

## Geometry and motion

CAD development uses exact boundary curves, sampled independently of display
tessellation. Missing entities and stale asset revisions require reselection.
The selected cylinder reference is resolved again when generating. Periodic
cylindrical floor faces with an explicit CAD seam develop into full-width bands.
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

## Verification limits

**Cylindrical stock-removal, stock-collision and cutting-force simulation are not
implemented.** Rotary runs show an explicit warning and do not display a false
fixed-Z stock simulation. This also applies to mixed rotary/ordinary jobs. The
backplot and animated cutter are geometric previews, not a collision certificate.
Fixture envelopes and Pocket Stock Contours are currently rejected with wraps.
Subsequent setups cannot inherit a wrapped setup's stock.
Numerical motion, XYZ machine travel, cutting feed, spindle limits, depth and
below-surface rapid traverses are still checked. Wrapped walls follow radial tool
orientation; a straight-sided pocket in the CAD model need not be machinable to
the same wall shape with cylindrical wrapping.

## Example

```sh
cargo run -p cam-app --example rotary_wrap -- /tmp/rotary-wrap.ocam
cargo run -p cam-app --features gui -- --setup /tmp/rotary-wrap.ocam
```

The example saves an unwrapped drawing pocket and its wrapped LinuxCNC program.
The sample illustrates geometry and NC output; it does not include a stock
verification result.

The corresponding reference behavior is described in Autodesk's
[4th Axis Wrap Toolpath reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID-7FF25192-AED7-4230-92DC-E22ADF15D498.htm).
Feed-mode semantics follow the LinuxCNC
[G93/G94 reference](https://www.linuxcnc.org/docs/html/gcode/g-code.html#gcode:g93-g94-g95).
