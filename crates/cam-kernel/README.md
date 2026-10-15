# Optional STEP backend

Default builds do not load native CAD libraries. `--features cam-import/occt`
(or `cam-kernel/occt`) enables the owned OCCT bridge when an SDK is present.
Set `OCCT_ROOT` to a prefix containing `include/opencascade` and `lib`, or set
`OCCT_INCLUDE_DIR` and `OCCT_LIB_DIR`. OCCT 7.8/7.9 API is the bounded target;
no native distribution or whole-STEP-standard qualification is implied.
Missing SDKs return `BackendUnavailable`, including all-features headless tests.

The bridge uses the real `STEPControl_Reader`, millimetre system units, solid
validation, analytic plane/cylinder/torus and line/circle descriptors, oriented topology,
and independent display tessellation. Other analytic/spline entities do not
supply analytic feature adapters. Whole-body XY silhouettes use a separate,
fixed-tolerance projection tessellation rather than display triangles. OCCT calls are serialized,
all handles live inside a call, and all C++ exceptions are translated.

Source units must be known and homogeneous. Assemblies are flattened to placed
solid occurrences with distinct body identity; product names, colours, component
paths, unresolved external dependencies and XDE sharing are not yet qualified.
Surface-only STEP is rejected. No additional healing is performed beyond OCCT's
translator using the STEP file's declared uncertainty. The import precision is a
fallback for files without uncertainty; it does not clamp source tolerances.
Forcing a finer precision can falsely invalidate otherwise valid spline joins.
Solid validation and the independent machining eligibility checks still apply.
These are normal importer capabilities, documented here rather than reported as
warnings on every successful import. Import warnings describe file-specific
problems, such as unavailable XY silhouettes after projection meshing fails.
Saved source bytes plus the versioned exact subset snapshot support reopening
without the original file.
The snapshot preserves the supported machining subset, not arbitrary native B-rep;
unsupported curved surfaces remain inspectable, and can contribute to the
whole-body projected outline through the independent projection tessellation.

The feature resolver can profile a circular blind-bore floor even when the body
has unrelated blends. This uses the analytic bore wall and complete planar floor
boundary, with an entry at the body's top in setup Z; it does not derive a cut
from unsupported surfaces or display triangles.

Native tests require a locally installed SDK. Before release, build and run the
fixture corpus on each desktop target, audit the SDK packaging/licence notices,
and qualify AP203/AP214/AP242 units, instances and topology. This repository does
not pretend a text scanner is a STEP importer and never substitutes display facets
for exact machining geometry.

Ctrl-click silhouette selection stores a body/instance reference. Imports also
embed a separate 3D projection mesh at 0.0025 mm absolute edge/interior deflection,
with surface-deflection checks enabled for analytic surfaces as well as splines.
The resolver projects it into the active setup XY plane, unions the triangles,
fills internal holes and adds a 0.0027 mm outward allowance. This permits an
outside envelope Profile across rounded transitions without claiming exact
freeform-surface machining. The projection does not depend on display quality;
its snapshot and placement participate in the operation fingerprint. Reopening
needs neither the STEP source nor OCCT. Projects predating this data need a STEP
reimport before silhouette selection can drive machining. Failed/incomplete
projection meshing or more than one million triangles disables silhouettes while
retaining ordinary inspection/import behavior.
