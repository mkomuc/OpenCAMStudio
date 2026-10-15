//! Revolved cutter surfaces, in tool coordinates: tip at zero, shank along +Z.
//! Assembly placement is separate so a holder can later share the same pose.
use crate::MeshVertex;
use cam_geo::Profile2D;

pub fn tool_mesh(profile: &Profile2D) -> (Vec<MeshVertex>, Vec<u32>) {
    const SECTORS: usize = 64;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (points, cutting) in profile.segment_polylines(0.01) {
        let color = if cutting {
            [0.22, 0.68, 0.78, 1.]
        } else {
            [0.72, 0.75, 0.80, 1.]
        };
        for pair in points.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let dr = b.x - a.x;
            let dz = b.y - a.y;
            let length = dr.hypot(dz);
            if length < 1e-9 {
                continue;
            }
            for sector in 0..SECTORS {
                let angles =
                    [sector, sector + 1].map(|i| i as f64 * std::f64::consts::TAU / SECTORS as f64);
                let base = vertices.len() as u32;
                for (point, angle) in [
                    (a, angles[0]),
                    (a, angles[1]),
                    (b, angles[1]),
                    (b, angles[0]),
                ] {
                    let (sin, cos) = angle.sin_cos();
                    vertices.push(MeshVertex {
                        position: [
                            (point.x * cos) as f32,
                            (point.x * sin) as f32,
                            point.y as f32,
                        ],
                        normal: [
                            (dz / length * cos) as f32,
                            (dz / length * sin) as f32,
                            (-dr / length) as f32,
                        ],
                        color,
                    });
                }
                // Avoid zero-area triangles at the tip and the closing cap.
                if a.x > 1e-9 {
                    indices.extend([base, base + 1, base + 2]);
                }
                if b.x > 1e-9 {
                    indices.extend([base, base + 2, base + 3]);
                }
            }
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_model::{Tool, ToolKind};

    #[test]
    fn cutter_mesh_preserves_dimensions_and_has_outward_faces() {
        for kind in [
            ToolKind::EndMill,
            ToolKind::BallMill,
            ToolKind::Drill {
                point_angle_deg: 118.,
            },
        ] {
            let tool = Tool {
                kind,
                diameter: 6.,
                length: 40.,
                flute_length: 15.,
                shank_diameter: 8.,
                ..Tool::default()
            };
            let (vertices, indices) = tool_mesh(&tool.profile());
            assert!(!indices.is_empty());
            let z_min = vertices
                .iter()
                .map(|v| v.position[2])
                .fold(f32::INFINITY, f32::min);
            let z_max = vertices
                .iter()
                .map(|v| v.position[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!((z_min, z_max), (0., 40.));
            assert!(vertices.iter().any(|v| (v.position[0] - 4.).abs() < 1e-5));
            assert!(vertices.iter().any(|v| v.color != vertices[0].color));
            for triangle in indices.chunks_exact(3) {
                let [a, b, c] = std::array::from_fn(|i| vertices[triangle[i] as usize]);
                let u: [f32; 3] = std::array::from_fn(|i| b.position[i] - a.position[i]);
                let v: [f32; 3] = std::array::from_fn(|i| c.position[i] - a.position[i]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                assert!((0..3).map(|i| cross[i] * a.normal[i]).sum::<f32>() > 0.);
                assert!((a.normal.iter().map(|n| n * n).sum::<f32>() - 1.).abs() < 1e-5);
            }
        }
    }
}
