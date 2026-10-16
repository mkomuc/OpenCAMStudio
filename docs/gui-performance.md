# GUI performance

The viewport keeps a shared CPU snapshot of its display buffers and picking
geometry. Camera changes, menu activity and repeated hover over the same entity
reuse that snapshot. The cache key includes the document revision, worker result
stamp, selection, visibility, stock preferences, setup draft and operation draft.
Playback stays live, and unstamped headless results bypass the snapshot cache.

CAD picking acceleration is cached separately, so changing a face highlight does
not rebuild the triangle hierarchy. CAD bounds are computed once when the scene
is built. The hierarchy uses median partitions instead of sorting every level.

The GPU pipeline retains shared buffer identities and uploads geometry only when
it changes. Small dynamic annotations have their own line buffer, avoiding copies
of the entire backplot. Transparent stock is re-sorted when the camera changes;
that updates its index buffer while preserving its vertex buffer.

## Reproducing the measurements

From the repository root:

```sh
cargo test -p cam-app --features gui viewport_interaction_benchmark -- --ignored --nocapture
```

This loads `testfiles/test_project.ocam` (12,194 CAD triangles), warms the viewport,
then measures 60 pointer updates and 60 camera updates, including CPU preparation
of the shader primitive. It also measures full widget construction while browsing
and while editing the first operation. It does not include window-system latency,
widget layout, text rasterization, GPU execution or frame presentation. These are
CPU costs, not frame-rate measurements.

Local macOS debug-build measurements on 2026-10-16:

| CPU work per interaction | Before | After |
| --- | ---: | ---: |
| Pointer update + viewport draw preparation | 137.257 ms | 0.012 ms |
| Orbit update + viewport draw preparation | 135.172 ms | 0.014 ms |
| Orbit update + full UI construction, browsing | — | 0.039 ms |
| Orbit update + full UI construction, editor open | — | 0.195 ms |

The viewport benchmark intentionally measures repeated interactions after loading,
not file parsing, initial scene preparation or simulation throughput.

## Validation

The regular GUI tests exercise camera/menu reuse, hover highlights, instance
visibility, setup revisions, transparent stock and uncommitted operation changes.
The existing picking, geometry editing and simulation tests also run with `gui`.
An optional offscreen GPU test exercises the real pipeline, repeated preparation,
orbiting and both opaque and transparent stock:

```sh
cargo test -p cam-app --features gui,occt viewport_gpu_smoke -- --ignored --nocapture
```

It requires access to a graphics adapter (Metal on macOS), and changes stock
preferences only in memory. Remove `occt` if the local OCCT SDK is unavailable.

The broader test run also exposed an existing failure in
`positive_world_floor_uses_physical_depth_and_matches_an_independent_planar_cut`
(`crates/cam-app/tests/cad_features.rs`). The same depth assertion fails on an
unmodified checkout. Strict Clippy also reports existing warnings in `mesh.rs`,
`profile.rs`, `gui_profile_tabs.rs` and an older workspace test; these are outside
this performance change.
