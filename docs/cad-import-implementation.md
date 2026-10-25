# CAD import implementation and qualification

Implemented 2026-10-13 from [the feature plan](cad-import-feature-selection.md).
This is the first fixed-axis STEP machining slice. The broader release gates in
that plan remain qualification work; the application does not claim general
surface machining or B-rep collision verification.

## Available workflow

Build with `gui,occt` and OCCT **7.9.3**. Home → Import accepts STEP/STP, IGES/IGS and the
existing DXF/DWG inputs. Translation runs off the UI thread and successful imports
are used immediately; there is no Add to project step. Warnings and errors appear
in an in-app dialog over the project window. An import failure leaves the project
intact. Edits or active geometry drafts during translation reject the request
without discarding those changes. Drawing imports add geometry and preserve
existing operations/tools; their addition can be undone.

Models groups each imported source model above its body instances, each with its
own visibility control. Feature lists and filters are absent from the project
tree. Setup body selection uses Add followed by clicks in the shaded viewport;
new setups contain no models by default. The setup selection filter and its
select-through/hide/isolate controls have been removed. Body visibility and
isolation remain available in the project tree. Profile accepts a face interior as its
outer boundary, and Drill accepts cylindrical faces and their visible circular
rims. STEP and DXF use the same creation controls and five-tab operation editor.
Hover and selection highlight semantic geometry. Ctrl-click selects the visible
body's silhouette projected onto the active setup XY reference plane (Z = 0),
for Profile and Face. Surface and thread operations keep their operation-specific
picking filters. Shift-click toggles multiple features;
Command-click also toggles on macOS. Cylindrical and toroidal fillet faces retain
their radius for inspection. Profile selection skips nonplanar wall/seam loops;
a top boundary adjoining a rounded transition selects the whole-body silhouette.
Overlapping edge candidates remain available for WCS picking. Hide/isolate/show-all are view state. Select a planar face and choose
**Use face as machining top at Z0** to establish the fixed frame; its longest
straight boundary edge sets X. Placement also supports XYZ translation and Z
rotation. Stock remains independently editable.

Create Pocket, Drill, Profile, Face, Chamfer, Engrave, Carve or Thread from the selection. Review extracted top,
bottom, preserved islands, holes and approximation diagnostics, then select the
tool from the library. Operation links survive save/reopen, duplicate, undo and
redo. Absolute height overrides are explicit; blank height inputs follow geometry.
**Edit / repair geometry** changes the source selection while preserving machining
parameters and overrides. Unsupported selections give an incompatibility reason.

On the Geometry tab, Chamfer, Engrave, Carve and Thread share Profile's
icon-based collection list. The chain, face and thread icons add supported
geometry; delete removes the selected item and the gear opens its settings.
Hover over an icon for its selection description. Settings use OK/Cancel to
stage or discard geometry without changing other operation tabs. Chamfer offers
**Modeled faces**, **Boundary loop**, and **Individual edges**. Modeled upward
inclined planar faces supply their actual upper and lower boundaries; select
multiple adjacent bevel faces with the face icon. Straight trimmed strips that
widen toward the lower edge are supported. The cutter's included angle must
match the modeled bevel. On **Passes**, **Stepdown** limits each finishing band;
zero automatically uses the cutter's straight cutting flank, so a cutter smaller
than the whole bevel finishes it in several descending bands. Each band's XY
position follows the face slope, with a feed approach from the air side. Both
boundaries appear at their actual Z heights in the geometry preview. Face width
and total depth follow the model. For an edge or loop, set **Chamfer width** under **Fase** on
**Passes** and optional width increments on **Passes**. A single planar edge
or connected open chain retains its endpoints, with a retract before each return;
no closing cut is added. Side is inferred from adjoining planar surfaces and can
be changed explicitly. Schema v24 preserves open chains, while legacy chamfers
remain closed; modeled-face inputs were introduced in v23.

Engrave offers boundary-loop and individual-edge modes and also accepts a
connected open chain, preserving its endpoints. Carve offers a face with islands
(all inner boundaries remain uncut) or a single closed boundary loop.
Thread accepts cylindrical bore or boss faces, including picks on their circular
rims; multiple centres must share diameter and cutting heights. The selected
cylinder seeds internal/external type and diameter. Geometry also provides
metric coarse/fine and UNC/UNF standard-size presets, optional prepared-bore or
nominal-diameter matching, and shared pitch/hand controls with Passes. Nominal
thread diameter and pitch remain editable; see [thread presets](thread-presets.md).
These operations follow the selected feature's setup Z
plane. Surface-operation depth is specified by the operator; thread geometry
provides the cylindrical cutting interval. Clear, remove and reselect work during
creation and editing, with all changes waiting for the operation's OK/Cancel.

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

The Output pane retains selection and operation errors with setup, operation
type/ID, named geometry entry, CAD model/instance and exact feature IDs. It logs
invalid chain previews as soon as selection changes. Drag to select text and
copy with Cmd/Ctrl+C, or use Copy output. Text updates pause while a selection is
active so incoming progress cannot erase text being copied; click in the text
to clear the selection and resume updates.

## Qualified geometry subset

- Pocket: one upward planar floor; closed vertical walls and constant opening
  elevation; raised islands retained. Walls require full-height trims and planar
  opening rims with unambiguous void/island adjacency. Slopes, blends on the
  selected walls, overhangs, ambiguous trims and bores through the selected floor
  are rejected conservatively. Unrelated curved surfaces elsewhere are allowed.
- Drill: inward analytic cylinders aligned with machining Z, complete rims and
  planar termination. Entry may be planar or an analytically certified coaxial
  outward-opening quarter-torus, using the same qualification as Bore and Thread.
  Split seam faces are merged. Bore diameter, entry and
  depth must agree within a group; the tool diameter must match. Circular edges
  alone cannot establish a hole. Conical tips and breakthrough are not inferred.
- Open Profile chains: connected line, circle, B-spline or Bezier edges on an
  accessible setup-XY rim or floor plane, with vertical supporting walls.
  Explicit **Open** mode on a named Chain entry also accepts planar edges
  adjoining rounded or unsupported surfaces. It uses the upward face boundary
  orientation for the initial cutter side and the operation's Top/Bottom height
  settings for depth. This cuts the vertical extension of the selected curve;
  it does not infer a safe wall depth or finish the adjoining 3D surface.
  Auto/Closed modes continue to require supporting vertical walls.
  Exact rational spline spans are approximated within 0.01 mm using control-hull
  bounds. Plane/cylinder normals or imported spline-wall edge orientations choose
  the free side locally; unrelated face trim gaps or unsupported curves do not
  block a supported open chain. Spline walls must be exact linear extrusions.
  Two end trims may meet rounded transitions: open profiles follow the selected
  XY curve at each depth, cutting its vertical extension through those ends.
  Closed features retain the full-height wall certificate. Selecting floor
  edges uses the wall's upper/lower cutting interval, rather than requiring
  the selected edge itself to be at the top of the body. Reimport old saved
  CAD assets to obtain spline descriptors and extrusion orientation data.
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
`occt` still report backend unavailable when importing STEP or IGES.
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
progress, broader IGES qualification, package-size/performance budgets and native release bundles on all
three platforms. Current STEP assemblies are flattened to solid instances with
shared body identity and placements; assembly hierarchy/metadata are not claimed.
Open surface-only files and unknown/mixed units are rejected with diagnostics.
IGES closed surface shells are sewn into solids; see the kernel README for limits. The
optional backend is not yet enabled in release packaging.

### Multi-body Tigerente import check

`testfiles/Tigerente_multiple.step` imports five valid solid bodies and five
placements, containing 54 faces. Each body has display mesh triangles. Projection
meshing retains the 0.0025 mm absolute linear deflection and interior surface
checks, with a 0.35 rad angular limit instead of 0.05 rad. The tighter angular
limit unnecessarily overmeshed small rounds and exhausted the combined 1,000,000
triangle budget at body 4. The optimized import produces 386,326 projection
triangles covering every face of all five bodies, without a projection warning.
Local debug import time fell from approximately 21.0 s to 3.6 s. The triangle
budget, source tolerances and separate display mesh remain unchanged. Regression
coverage asserts complete face coverage and the original linear tolerance.
Native projection warnings still identify the body and specific failure or limit.
