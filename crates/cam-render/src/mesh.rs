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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// Closed convex blanks have one front and one rear surface along each view
/// ray. A long triangle's centroid can be behind the centroid of an overlapping
/// rear triangle even when that whole overlap is in front. Group the rear faces
/// first, then sort by depth within each group. Non-convex simulated stock must
/// use ordinary depth sorting instead.
#[cfg(any(feature = "gpu", test))]
pub(crate) fn transparent_convex_indices(
    vertices: &[MeshVertex],
    indices: &[u32],
    view_proj: [[f32; 4]; 4],
) -> Vec<u32> {
    let clip = |p: [f32; 3]| -> [f32; 4] {
        std::array::from_fn(|i| {
            view_proj[0][i] * p[0]
                + view_proj[1][i] * p[1]
                + view_proj[2][i] * p[2]
                + view_proj[3][i]
        })
    };
    let mut triangles: Vec<_> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let p = triangle.map(|i| clip(vertices[i as usize].position));
            let xy = p.map(|v| [v[0] / v[3], v[1] / v[3]]);
            let cross = (xy[1][0] - xy[0][0]) * (xy[2][1] - xy[0][1])
                - (xy[1][1] - xy[0][1]) * (xy[2][0] - xy[0][0]);
            let depth = p.iter().map(|v| v[2] / v[3]).sum::<f32>();
            (cross > 0., depth, *triangle)
        })
        .collect();
    triangles.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.total_cmp(&a.1)));
    triangles
        .into_iter()
        .flat_map(|(_, _, triangle)| triangle)
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
    fn transparent_convex_stock_draws_rear_before_front_when_centroids_mislead() {
        // At the overlapping tip, the small front triangle is closer; its
        // centroid is nevertheless behind that of the long rear triangle.
        let positions = [
            [-1., -1., 0.1],
            [0., 1., 0.9],
            [1., -1., 0.1],
            [-0.2, 0.6, 0.75],
            [0.2, 0.6, 0.75],
            [0., 0.95, 0.75],
        ];
        let vertices = mesh_vertices(&positions, &[[0., 0., 1.]; 6]);
        let indices = [0, 1, 2, 3, 4, 5];
        assert_eq!(
            transparent_indices(&vertices, &indices, crate::IDENTITY),
            [3, 4, 5, 0, 1, 2]
        );
        assert_eq!(
            transparent_convex_indices(&vertices, &indices, crate::IDENTITY),
            indices
        );
    }

    #[test]
    fn translucent_long_cylinder_keeps_the_nearest_surface_last_across_the_view() {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for i in 0..128 {
            let a = std::f32::consts::TAU * i as f32 / 128.;
            let b = std::f32::consts::TAU * (i + 1) as f32 / 128.;
            let p = |angle: f32, z| [16. * angle.cos(), 16. * angle.sin(), z];
            let [oa, ob, oc, od] = [p(a, -1.), p(b, -1.), p(a, 41.), p(b, 41.)];
            for point in [oa, ob, od, oa, od, oc] {
                indices.push(positions.len() as u32);
                positions.push(point);
            }
        }
        let vertices = mesh_vertices(&positions, &vec![[0., 0., 1.]; positions.len()]);
        let mut camera = crate::OrbitCamera::framed([-16., -16., -1.], [16., 16., 41.]);
        let mut worst = (0, 0);
        for pitch in [0.25, 0.5, 0.75, 1., 1.25, 1.45, 1.55] {
            for yaw in [0., 0.3, 0.7, 1.1] {
                camera.orient = crate::orientation(yaw, pitch);
                let matrix = camera.view_proj(1.);
                let project = |p: [f32; 3]| -> [f32; 3] {
                    std::array::from_fn(|i| {
                        matrix[0][i] * p[0]
                            + matrix[1][i] * p[1]
                            + matrix[2][i] * p[2]
                            + matrix[3][i]
                    })
                };
                let projected: Vec<_> = positions.iter().copied().map(project).collect();
                let reversed_pixels = |order: &[u32]| {
                    let mut reversed = 0;
                    let mut overlaps = 0;
                    for y in 0..96 {
                        for x in 0..96 {
                            let (px, py) = (
                                -0.9 + 1.8 * (x as f32 + 0.5) / 96.,
                                -0.9 + 1.8 * (y as f32 + 0.5) / 96.,
                            );
                            let mut nearest = f32::INFINITY;
                            let mut last = f32::INFINITY;
                            let mut hits = 0;
                            for triangle in order.as_chunks::<3>().0 {
                                let [a, b, c] = [
                                    projected[triangle[0] as usize],
                                    projected[triangle[1] as usize],
                                    projected[triangle[2] as usize],
                                ];
                                let den =
                                    (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                                if den.abs() < 1e-10 {
                                    continue;
                                }
                                let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1]))
                                    / den;
                                let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1]))
                                    / den;
                                let w = 1. - u - v;
                                if u > 1e-5 && v > 1e-5 && w > 1e-5 {
                                    let z = u * a[2] + v * b[2] + w * c[2];
                                    nearest = nearest.min(z);
                                    last = z;
                                    hits += 1;
                                }
                            }
                            if hits > 1 {
                                overlaps += 1;
                                if last > nearest + 1e-5 {
                                    reversed += 1;
                                }
                            }
                        }
                    }
                    (reversed, overlaps)
                };
                let ordinary = transparent_indices(&vertices, &indices, matrix);
                let corrected = transparent_convex_indices(&vertices, &indices, matrix);
                let before = reversed_pixels(&ordinary);
                let after = reversed_pixels(&corrected);
                assert_eq!(after.0, 0);
                if before.0 > worst.0 {
                    worst = before;
                }
            }
        }
        assert!(
            worst.0 > 0,
            "exercise the long-cylinder ordering defect: {worst:?}"
        );
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
