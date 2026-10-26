//! # cam-render — viewport scene + wgpu renderer
//!
//! Two layers, split so the logic stays testable without a GPU:
//!
//! - The **[`Scene`]** model and its mapping to GPU [`Vertex`]es — colored
//!   polylines for the part outline and the toolpath backplot — plus
//!   [`MeshVertex`]es for a solid stock surface. Pure data, unit-tested; builds
//!   with **no default features**.
//! - The **`gpu` feature**'s renderers ([`LineRenderer`], [`MeshRenderer`]),
//!   which upload those vertices and draw them with `wgpu`. They need a graphics
//!   stack, so they are compiled by CI and on the desktop, not exercised in
//!   headless unit tests.

mod cad;
mod camera;
mod gizmo;
mod mesh;
mod path_cone;
mod scene;
mod tool;

pub use cad::{CadScene, PickCandidate, SurfaceClass};
pub use camera::{orientation, top_view, OrbitCamera, Ray, IDENTITY};
#[cfg(feature = "gpu")]
pub use gizmo::label_atlas;
pub use gizmo::{pick_face, unit_cube, GizmoVertex};
pub use mesh::{mesh_vertices, MeshVertex};
pub use path_cone::{path_cone_mesh, PathCone, PATH_IN, PATH_OUT};
pub use scene::{
    Color, LineStrip, Scene, Vertex, CUT, ENVELOPE, ENVELOPE_OVER, PART, PLUNGE, RAPID,
};
pub use tool::tool_mesh;

#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "gpu")]
pub use gpu::{GizmoRenderer, LineRenderer, MeshRenderer, DEPTH_FORMAT};
