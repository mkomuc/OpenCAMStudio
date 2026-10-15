# CAD import implementation and qualification

Implemented 2026-10-13 from [the feature plan](cad-import-feature-selection.md).
This is the first fixed-axis STEP machining slice. The broader release gates in
that plan remain qualification work; the application does not claim general
surface machining or B-rep collision verification.

## Available workflow

Build with `gui,occt` and OCCT **7.9.3**. Home → Import accepts STEP/STP and the
existing DXF/DWG inputs. Translation runs off the UI thread and successful imports
are used immediately; there is no Add to project step. Warnings and errors appear
in an in-app dialog over the project window. An import failure leaves the project
intact. Edits or active geometry drafts during translation reject the request
without discarding those changes. Drawing imports add geometry and preserve
existing operations/tools; their addition can be undone.

Models contains one visibility row per model instance. Feature lists and filters
are absent from the project tree. Select geometry in the shaded viewport; setup
filters remain in the right inspector. Profile accepts a face interior as its
outer boundary, and Drill accepts cylindrical faces and their visible circular
rims. STEP and DXF use the same creation controls and five-tab operation editor.
Hover and selection highlight semantic geometry. Ctrl-click selects the visible
body's silhouette projected onto the active setup XY reference plane (Z = 0),
regardless of the geometry filter. Shift-click toggles multiple features;
Command-click also toggles on macOS. Cylindrical and toroidal fillet faces retain
their radius for inspection. Profile selection skips nonplanar wall/seam loops;
a top boundary adjoining a rounded transition selects the whole-body silhouette. Overlapping candidates and explicit select-through are
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
import budgets, analytic topology, independent projection tessellation and semantic display mesh in schema-14
JSON `.ocam` files. Asset source/topology snapshots share storage across history
entries. Existing projects migrate to explicit embedded geometry; no CAD
associations are guessed. Saving validates a temporary file and atomically
replaces the destination. Reopening does not need the source file or CAD SDK.

Model context menus provide explicit remove/replace commands. Replacement runs
in the background and commits directly, then warns when linked operations need
review. The controller retains its staged preview API for headless callers. Replacements
require reselection, preserve overrides, and invalidate runs. Missing, changed or
unresolved included sources block generation/export. Excluded operations are not
resolved or machined. Every run resolves planar inputs from the bound topology/projection data;
there is no stale embedded-contour fallback. Direct planner calls also require a
matching resolved fingerprint.

## Qualified geometry subset

- Pocket: one upward planar floor; closed vertical walls and constant opening
  elevation; raised islands retained. Walls require full-height trims and planar
  opening rims with unambiguous void/island adjacency. Slopes, blends on the
  selected walls, overhangs, ambiguous trims and bores through the selected floor
  are rejected conservatively. Unrelated curved surfaces elsewhere are allowed.
- Drill: inward analytic cylinders aligned with machining Z, complete rims and
  planar entry/termination. Split seam faces are merged. Bore diameter, entry and
  depth must agree within a group; the tool diameter must match. Circular edges
  alone cannot establish a hole. Conical tips and breakthrough are not inferred.
- Profile: an exterior coplanar loop with vertical supporting walls and an outside
  computed-compensation cut. Alternatively, select the whole-body XY silhouette,
  including spline outlines and rounded top transitions. It uses a separate OCCT
  projection tessellation at 0.0025 mm deflection and adds a 0.0027 mm outward
  allowance (deflection plus planar-grid margin). Concavities are retained;
  projected internal holes are filled for the outside profile. This is a bounded
  approximation of the body envelope, not a 3D fillet-finishing operation.
  Display tessellation never supplies this boundary. Old embedded imports without
  projection data require reimporting; incomplete projection data is rejected. A circular
  blind-bore floor also supports an inside cut: its inward cylindrical wall,
  complete circular rims and planar disk floor certify the local feature. Its
  entry must be at the top of the body in setup Z, excluding material above it.
  Unrelated blends elsewhere do not block this local bore profile. Creation
  derives the inside/outside side from the selected boundary. Cylinder trim loops
  containing seams require selection of the planar floor loop instead. Other
  inside profiles, control compensation and radial roughing remain unsupported.
- Face: an upward planar target at the body's highest setup Z. Conservative exact
  body bounds drive a rectangular stock-facing region, including above holes and
  curved boundaries below the target. Bosses above the target remain blocked.
  An XY silhouette can also supply the facing boundary, with the highest body
  bound as the target and stock top as entry.

Pocket and Drill certify the local constant-section void and check vertical
access above its opening using transformed exact face bounds. Unknown curved
surfaces overlapping that approach remain blocked; disjoint surfaces and curves
below the opening do not invalidate a qualified feature. Outside Profile checks
that remaining faces are enclosed by the selected silhouette, using conservative
bounds or supported exact planar boundaries. No operation requires every face
of an otherwise unrelated part of the body to be an analytic extrusion.

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
