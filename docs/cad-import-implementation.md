# CAD import implementation and qualification

Implemented 2026-10-13 from [the feature plan](cad-import-feature-selection.md).
This is the first fixed-axis STEP machining slice. The broader release gates in
that plan remain qualification work; the application does not claim general
surface machining or B-rep collision verification.

## Available workflow

Build with `gui,occt` and OCCT **7.9.3**. Home → Import accepts STEP/STP and the
existing DXF/DWG inputs. Translation runs off the UI thread. Review source units,
normalized dimensions, instance count and diagnostics before **Add to project**.
Cancel leaves the project intact. Edits during translation or after preview
invalidate the import request. Drawing imports now add geometry and preserve
existing operations/tools; their addition can be undone.

Select Body, Face, Edge, Loop or Vertex in the geometry tree or shaded viewport.
Hover and selection highlight semantic geometry. Ctrl/Command or Shift-click
changes multi-selection. Overlapping candidates and explicit select-through are
available. Hide/isolate/show-all are view state. Select a planar face and choose
**Use face as machining top at Z0** to establish the fixed frame; its longest
straight boundary edge sets X. Placement also supports XYZ translation and Z
rotation. Stock remains independently editable.

Create Pocket, Drill, Profile or Face from the selection. Review extracted top,
bottom, preserved islands, holes and approximation diagnostics, then select the
tool from the library. Operation links survive save/reopen, duplicate, undo and
redo. Absolute height overrides are explicit; blank height inputs follow geometry.
**Edit / repair geometry** changes the source selection while preserving machining
parameters and overrides. Unsupported selections give an incompatibility reason.

Assets embed their original bytes, source fingerprint, units, backend version,
import budgets, exact analytic topology and semantic display mesh in schema-14
JSON `.ocam` files. Asset source/topology snapshots share storage across history
entries. Existing projects migrate to explicit embedded geometry; no CAD
associations are guessed. Saving validates a temporary file and atomically
replaces the destination. Reopening does not need the source file or CAD SDK.

The selection inspector provides explicit remove/replace model commands, with
a background replacement preview. The controller also exposes these commands. Replacements
require reselection, preserve overrides, and invalidate runs. Missing, changed or
unresolved included sources block generation/export. Excluded operations are not
resolved or machined. Every run derives new planar inputs from exact curves;
there is no stale embedded-contour fallback. Direct planner calls also require a
matching resolved fingerprint.

## Qualified geometry subset

- Pocket: one upward planar floor; closed vertical walls and constant opening
  elevation; raised islands retained. Slopes, blends, overhangs, ambiguous walls
  and bores through the selected floor are rejected conservatively.
- Drill: inward analytic cylinders aligned with machining Z, complete rims and
  planar entry/termination. Split seam faces are merged. Bore diameter, entry and
  depth must agree within a group; the tool diameter must match. Circular edges
  alone cannot establish a hole. Conical tips and breakthrough are not inferred.
- Profile: an exterior coplanar loop with vertical supporting walls, a certified
  whole-body silhouette and an outside computed-compensation cut. Other sides,
  control compensation and radial roughing require further qualification.
- Face: a rectangular whole-body planar top without holes, bosses or protected
  material. The selected extent drives the existing rectangular facing strategy.

Pocket/profile strategies run in a local Z frame using the explicit physical
cutting interval and then return world coordinates. Drill/Face share the same
absolute top/bottom contract. Legacy world-space planes stay unchanged. Reach
checks use physical depth, setup clearance/retract must be above the cutting top,
and posting applies the existing datum translation once.

Machining curve approximation is 0.01 mm plus the reported planar-grid budget.
Display tessellation and import precision have separate options. Changing display
quality does not change machining contours or NC.

## Native build

The default headless build and ordinary GUI build have no required CAD SDK.
Requesting `occt` without its SDK now fails the build with setup instructions;
it cannot silently produce a desktop app without STEP support. Builds without
`occt` still report backend unavailable when importing STEP.
With a tested OCCT 7.9.3 installation:

```sh
OCCT_ROOT=/path/to/occt cargo build -p cam-app --features gui,occt
```

`OCCT_ROOT` expects `include/opencascade` and `lib`. Alternatively set
`OCCT_INCLUDE_DIR` and `OCCT_LIB_DIR`. The runtime loader must also find OCCT and
its runtime dependencies (`DYLD_LIBRARY_PATH` on macOS, `LD_LIBRARY_PATH` on Linux,
or `PATH` on Windows). Ship the native libraries and their notices with packaged
CAD-enabled builds. OCCT LGPL 2.1 and its exception are included in `cam-kernel`;
the C++ bridge is project-owned GPL-3.0 code.

Local qualification used the conda-forge OCCT 7.9.3 `novtk` SDK on Apple silicon,
initially installed under `/tmp/opencam-occt-env`, without changing a system CAD
installation. This checkout now keeps a persistent copy in the git-ignored
`.local/occt` directory. A git-ignored `.cargo/config.toml` supplies `OCCT_ROOT`
and macOS `DYLD_LIBRARY_PATH`, so the ordinary launch command works here:

```sh
cargo run -p cam-app --features gui,occt
```

Other checkouts must install/configure their own SDK; these local paths are not
committed to the repository. The generator and original STEP fixtures are checked in at
`crates/cam-kernel/fixtures`. Reproduce native acceptance explicitly:

```sh
OCCT_ROOT=/path/to/occt DYLD_LIBRARY_PATH=/path/to/occt/lib \
  cargo test -p cam-app --features occt --test native_step -- --ignored
```

The native tests intentionally require a real backend. They are ignored in the
ordinary headless suite and never count an unavailable backend as import success.

## Evidence and remaining release work

Native tests exercise the plate pocket/island/four holes/exterior profile through
simulation and NC export, portable save/reopen, inch/mm normalization, rotated
model alignment, repeated component placements, and identical NC across display
qualities. Independent analytic tests compare positive-world-Z pocket cuts with
manually constructed planar inputs and cover binding edits, overrides,
replacement/reselection, undo, cancellation, stale requests and export blocking.
Render tests exercise depth/filter picking and repeated-instance identity.

Remaining release work from the original plan: OCCT/truck fixture comparison,
AP203/AP242 and difficult spline/shell qualification, assembly names/colours and
external dependency handling, editable X-direction/datum choices during frame
setup, replacement mapping suggestions, unknown-unit choice, fine-grained stage
progress, IGES, package-size/performance budgets and native release bundles on all
three platforms. Current STEP assemblies are flattened to solid instances with
shared body identity and placements; assembly hierarchy/metadata are not claimed.
Surface-only files and unknown/mixed units are rejected with diagnostics. The
optional backend is not yet enabled in release packaging.
