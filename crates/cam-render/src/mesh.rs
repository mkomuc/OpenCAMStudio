//! Solid-surface mesh data for the viewport: interleaved position+normal
//! vertices and the pure mapping that builds them. Like [`Scene`](crate::Scene),
//! nothing here touches a GPU, so it is unit-tested with the rest of the
//! pipeline; the `gpu` feature's [`MeshRenderer`](crate::MeshRenderer) uploads
//! and draws these.
//!
//! The mesh itself (a triangulated stock heightfield) is produced by `cam-sim`
//! as parallel position/normal/index arrays. We deliberately take those as raw
//! slices rather than depend on `cam-sim`: the renderer draws surfaces, it need
//! not know they came from a simulation.

/// A vertex for solid surface rendering: position (mm) and unit normal. Plain
/// data; the `gpu` feature makes it `bytemuck`-castable for upload.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "gpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear RGBA surface colour (also used for semantic CAD highlights).
    pub color: [f32; 4],
}

/// Interleave parallel `positions` and `normals` (as produced by `cam-sim`'s
/// `SurfaceMesh`) into [`MeshVertex`]es. The two slices are expected to be the
/// same length — one normal per position; any surplus tail on the longer slice
/// is ignored.
pub fn mesh_vertices(positions: &[[f32; 3]], normals: &[[f32; 3]]) -> Vec<MeshVertex> {
    positions
        .iter()
        .zip(normals)
        .map(|(&position, &normal)| MeshVertex {
            position,
            normal,
            color: [0.72, 0.66, 0.22, 1.0],
        })
        .collect()
}

/// Prepare stock vertices in world coordinates in one pass. Keep this bulk
/// conversion in the optimized renderer, including during development playback.
/// Setup frames rotate positions and normals; the setup datum is not a translation.
pub fn stock_mesh_vertices_controlled(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    frame: &cam_model::SetupFrame,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<MeshVertex>> {
    let rotation = frame.world_from_setup.rotation;
    let rotate = |v: [f32; 3]| {
        let [x, y, z] = v.map(f64::from);
        rotation.map(|row| (row[0] * x + row[1] * y + row[2] * z) as f32)
    };
    let count = positions.len().min(normals.len());
    let mut vertices = Vec::with_capacity(count);
    for start in (0..count).step_by(1024) {
        if cancelled() {
            return None;
        }
        let end = (start + 1024).min(count);
        vertices.extend(positions[start..end].iter().zip(&normals[start..end]).map(
            |(&position, &normal)| MeshVertex {
                position: rotate(position),
                normal: rotate(normal),
                color: [0.72, 0.66, 0.22, 1.0],
            },
        ));
    }
    (!cancelled()).then_some(vertices)
}

/// Back-to-front triangle order in clip depth for the viewport's orthographic camera.
#[cfg(any(feature = "gpu", test))]
pub(crate) fn transparent_indices(
    vertices: &[MeshVertex],
    indices: &[u32],
    view_proj: [[f32; 4]; 4],
) -> Vec<u32> {
    let mut triangles: Vec<_> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let depth: f32 = triangle
                .iter()
                .map(|&index| {
                    let p = vertices[index as usize].position;
                    view_proj[0][2] * p[0]
                        + view_proj[1][2] * p[1]
                        + view_proj[2][2] * p[2]
                        + view_proj[3][2]
                })
                .sum();
            (depth, [triangle[0], triangle[1], triangle[2]])
        })
        .collect();
    triangles.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    triangles
        .into_iter()
        .flat_map(|(_, triangle)| triangle)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_triangles_follow_camera_depth_when_orbiting() {
        let positions = [
            [0., 0., 1.],
            [1., 0., 1.],
            [0., 1., 1.],
            [0., 0., 3.],
            [1., 0., 3.],
            [0., 1., 3.],
        ];
        let vertices = mesh_vertices(&positions, &[[0., 0., 1.]; 6]);
        let indices = [0, 1, 2, 3, 4, 5];
        assert_eq!(
            transparent_indices(&vertices, &indices, crate::IDENTITY),
            [3, 4, 5, 0, 1, 2]
        );
        let mut reversed = crate::IDENTITY;
        reversed[2][2] = -1.;
        assert_eq!(transparent_indices(&vertices, &indices, reversed), indices);
    }

    #[test]
    fn mesh_vertices_interleaves_position_and_normal() {
        let pos = [[0.0, 0.0, -1.0], [1.0, 0.0, -1.0]];
        let nrm = [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let v = mesh_vertices(&pos, &nrm);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].position, [0.0, 0.0, -1.0]);
        assert_eq!(v[1].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn mismatched_lengths_stop_at_the_shorter() {
        let pos = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let nrm = [[0.0, 0.0, 1.0]];
        assert_eq!(mesh_vertices(&pos, &nrm).len(), 1);
    }

    #[test]
    fn stock_vertices_follow_setup_rotation_for_positions_and_normals() {
        let positions = [[1.25, -2.5, 3.75], [-10., 0., 0.5]];
        let normals = [[0., 0., 1.], [0.6, 0.8, 0.]];
        for frame in [
            cam_model::SetupFrame::default(),
            cam_model::SetupFrame::from_xy([0., 1., 0.], [-1., 0., 0.]).unwrap(),
            cam_model::SetupFrame::from_xy([1., 2., 3.], [-2., 1., 0.]).unwrap(),
        ] {
            let expected: Vec<_> = mesh_vertices(&positions, &normals)
                .into_iter()
                .map(|mut vertex| {
                    vertex.position = frame
                        .to_world_vector(vertex.position.map(f64::from))
                        .map(|v| v as f32);
                    vertex.normal = frame
                        .to_world_vector(vertex.normal.map(f64::from))
                        .map(|v| v as f32);
                    vertex
                })
                .collect();
            assert_eq!(
                stock_mesh_vertices_controlled(&positions, &normals, &frame, &|| false).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn stock_vertex_conversion_cancels_without_publishing_partial_geometry() {
        let points = vec![[1., 2., 3.]; 4096];
        let checks = std::cell::Cell::new(0);
        let result = stock_mesh_vertices_controlled(
            &points,
            &points,
            &cam_model::SetupFrame::default(),
            &|| {
                checks.set(checks.get() + 1);
                checks.get() == 2
            },
        );
        assert!(result.is_none());
        assert_eq!(checks.get(), 2);
    }
}
