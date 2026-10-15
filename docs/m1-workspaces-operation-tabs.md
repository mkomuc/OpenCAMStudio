# M1a/M1b implementation and acceptance evidence

Implemented 2026-10-14 against the existing fixed-axis backend. M1a's workspace
shell and M1b's supported operation controls are available. Material cutting
presets remain M2a work. Automated checks and desktop smoke tests exercised DXF
and native STEP, including feature picking, the shared operation workflow and
warning/error dialogs.

## Workspace and browser

Machining is the default workspace. The main drawing/model viewport has an
adaptive, dimensioned XY grid in millimetres, the project browser overlays its
left side, and the right column remains the inspector. Stock outlines, work
origins and the orientation cube retain their visibility controls. The complete
browser rectangle captures viewport input, including its empty area; covered
clicks, drags, releases and wheel events cannot pick geometry or move the camera.

Browser rows expose disclosure, selection, visibility and status. Setup and
operation commands live in context menus. Stable-ID drag/drop commits through
controller commands, persists document order and supports undo. Cross-setup
transfers use the existing tool remapping and CAD re-resolution checks. Operations
are displayed in the planner's work-origin groups; dragging between different
origin groups is rejected with a reason, and the explicit Move to Origin command
changes that assignment. Stock-dependency constraints remain future M7 work.
Visibility is presentation state; suppression changes machining inclusion.

Tools replaces the main viewport with an ordered/grouped library table. The
existing generated cutter preview and tool editor occupy the right inspector.
Switching back preserves the setup, camera and semantic selection. Dirty or
invalid drafts require OK/Cancel before context changes, including library
renumbering. Tool Apply targets the library even while the machining operation
selection is retained.

Successful DXF/STEP imports are used immediately. The project browser contains
only tree items; Models has one visibility row per instance, with no face/loop
lists, geometry filters, preview summaries or Add to project buttons. Import and
toolpath/verification warnings and errors appear in queued in-app dialogs.
Background project actions are blocked until a dialog is acknowledged.

## Operation draft and geometry workflows

All eight operation families have Tool, Geometry, Heights, Passes and Linking
tabs driven by their real typed backend parameters. Controls include cutter
selection, cutting data, compensation/strategy choices, supported heights and
passes, leads, entry style and preferred start points. Numeric fields carry
units and validators; automatic zero settings remain valid where supported.
Availability follows strategy/post support, and hidden controls cannot block OK.
Multi-axis controls are omitted.

Every tab shares one draft. Tab switches and enum changes preserve numeric text
and linked-height overrides. Geometry reselection stages sources alongside the
cutter, feeds, heights and other settings. CAD preview validates the prospective
transaction, including staged tools, without editing the document. OK commits
the complete draft in one undo step; Cancel restores all tabs and associations.
Newly created operations receive their own draft before subsequent editing.

Setup clearance/retract remain read-only operation context. CAD-derived cutting
heights and manual absolute-Z overrides are shown separately, with replacement
source heights previewed before commit. Overrides still respect the geometry's
certified cutting interval. Invalid or unresolved inputs cannot produce an
exportable result, and pending inspector/picker edits block Run and export.

STEP and DXF use the same creation inspector, cutter-family picker, cutting-data
controls and Confirm/Cancel flow. CAD creation embeds the cutter and cutting data
with the geometry in one undo transaction. Profile face clicks resolve to the
outer boundary, and Drill rim clicks resolve to the cylindrical hole. Visibility
of narrow features is tested at the feature position, including sloped surfaces.
Feature-first and operation-first creation share controller selection rules.
Ctrl/Command adds compatible geometry. Drawing Drill/Thread accept multiple
qualified circular features; Pocket/Carve accept a boundary with contained,
non-overlapping islands. A single-chain strategy rejects unrelated chains.
CAD selection requires one entity kind, current revisions and one body instance;
the existing qualified Pocket/Drill/Profile/Face recipes remain the supported
CAD subset. Rejections explain the incompatible input instead of silently
dropping part of the selection.

## Code locations

- `gui_workspace.rs` / `gui_grid.rs`: workspace surfaces, tool preview, grid and
  input isolation.
- `gui_project_tree.rs` / `setups.rs`: browser commands, stable-ID ordering,
  validated transfers and prospective operation transactions.
- `gui_operation_tabs.rs` / `gui_operation_metadata.rs`: typed field inventory,
  tab routing, draft validation and geometry preview.
- `gui_creation.rs` / `cad_creation.rs`: shared source workflows and atomic CAD creation.
- `gui_notices.rs`: direct import completion and queued warning/error dialogs.
- `selection.rs` / `drawing_drafts.rs` / `cad_operations.rs`: shared eligibility,
  both creation orders and geometry replacement that retains cutting settings.

## Verification

The automated suites cover workspace round trips; overlay input isolation;
stable-ID reorder/save/reopen/undo and actual generated order; suppression versus
visibility; all eight numeric field inventories and emitted feed/RPM changes;
tab retention and Cancel; atomic enum/numeric/geometry/height commits; stale,
mixed and incompatible selections; and both creation orders. A separate drill
regression changes cutter diameter and selected bore together, previews without
mutation, rejects a missing staged tool atomically, and restores the full document
with one undo.

Commands used:

```sh
cargo test --workspace
cargo test -p cam-app --features gui,occt
cargo test -p cam-app --features occt --test native_step -- --ignored
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p cam-app --features gui,occt --all-targets -- -D warnings
cargo build -p cam-app --features gui,occt
git diff --check
```

Final results: **794 workspace tests passed, 23 ignored; 275 GUI/CAD-feature tests
passed** (254 unit, 18 CAD integration, 3 multi-setup integration). The eight native
STEP integration tests also passed when explicitly enabled. Both strict clippy
configurations, the native GUI build and the diff whitespace check passed.

Desktop smoke tests verified the labeled grid, overlay input isolation, tree
context menus and reorder/Undo, Tools table and cutter preview, preserved
machining context, tab draft retention, Cancel, OK/Undo, direct native STEP
import, face-based Profile picking and successful generation. A DXF containing
an unsupported annotation imported its geometry and displayed a warning dialog;
a malformed STEP displayed an error dialog and left that drawing intact. These
checks used the local OCCT 7.9.3 SDK. M0 release qualification, M2a presets and the
additional M3a setup controls remain separate.
