# Geometry selection fixes: issues 1–2

Scope: [issues 1–2](issues/issues.md), using the existing fixed-axis geometry qualification.

## 1. Profile loop selection

Loop 72 in `cnc_test.ocam` is an inner rectangular boundary on an upward planar
face. The single-loop entry path currently requests an outside profile before
the existing contour extractor can infer the material side and certify its walls.

Route ordinary profile loop selections through the contour extractor, including
new-operation creation and geometry-entry editing. Retain explicit extraction
recipes and their safety checks. Derive the inside/outside side and cutting
heights from certified topology rather than accepting every loop indiscriminately.

Acceptance:

- Loop 72 resolves as an inside profile with certified wall heights.
- Single-loop creation and geometry-entry editing use the same qualification.
- Exterior loops, cylindrical bores and silhouettes retain their behavior.
- Unsupported walls or inaccessible geometry still fail qualification.

## 2. Context-aware viewport selection

Derive automatic picking from the current editing context instead of a filter
left behind by a previously selected operation. In the general viewport, offer
all supported CAD feature kinds. On an operation's Geometry tab, offer the kinds
supported by that operation and update its geometry selection mode from the
chosen feature. Preserve deliberate restricted picking in geometry settings,
height selection and setup-axis workflows.

Opening the tab only prepares picking; the first feature pick starts a geometry
edit. Named Profile selections use their existing entry transaction. This keeps
ordinary tab navigation compatible with Apply and preserves edits on other tabs.

Acceptance:

- Leaving an operation restores general feature selection.
- Entering Geometry permits supported picks without clicking a feature icon.
- Choosing an alternate supported kind updates the extraction/preview mode.
- Candidate lists retain occlusion, select-through and cycling behavior.

## Verification

Add focused topology/extraction and GUI/picking regression tests. Run the affected
geometry, renderer and GUI tests, then the workspace checks supported by the local
build environment. Record any remaining native OCCT or interactive validation
limits in the implementation handoff.

Validated during implementation:

- The supplied `cnc_test.ocam` Loop 72 resolves as an inside contour through both
  geometry-entry and direct contour extraction; this was a local fixture probe.
- Portable tests cover inner-loop operation creation and toolpath generation,
  circular inner rims, blocked bore diagnostics, mixed feature candidates, and
  automatic Geometry-tab selection for new and existing operations.
- `cargo test -p cam-geometry3d`, `cargo test -p cam-app --test cad_features`,
  `cargo test --workspace --exclude cam-app`, and workspace Clippy passed.
- `cargo test -p cam-app --features gui` passed, including 370 GUI library tests
  (4 ignored) and the app integration tests. GUI Clippy passed for all targets
  with warnings treated as errors.

The existing ignored native/long-running tests remain ignored. Interactive
desktop clicking and a new native CAD import are separate manual checks.
