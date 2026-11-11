//! Directional entry/exit markers, sized by the viewport in screen pixels.
use crate::{Color, MeshVertex};

/// Entry into a cut.
pub const PATH_IN: Color = [0.95, 0.12, 0.10, 1.0];
/// Withdrawal from a cut.
pub const PATH_OUT: Color = [0.10, 0.90, 0.20, 1.0];

/// A toolpath marker with its tip at `at`, pointing along its motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathCone {
    pub at: [f32; 3],
    pub direction: [f32; 3],
    pub color: Color,
    pub op: u32,
}

/// Filled cones pointing along travel, with a cap so they read from either end.
pub fn path_cone_mesh(cones: &[PathCone], height: f32) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    if !height.is_finite() || height <= 0.0 {
        return (vertices, indices);
    }
    const SECTORS: usize = 16;
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    for cone in cones {
        let d = cone.direction;
        let at = cone.at;
        let axis = if d[2].abs() < 0.9 {
            [0., 0., 1.]
        } else {
            [0., 1., 0.]
        };
        let u = cross(d, axis);
        let len = u.iter().map(|v| v * v).sum::<f32>().sqrt();
        let u = u.map(|v| v / len);
        let v = cross(d, u);
        let radius = height * 0.3;
        let tip = at;
        let base: [f32; 3] = std::array::from_fn(|i| at[i] - d[i] * height);
        for sector in 0..SECTORS {
            let radial = [sector, sector + 1].map(|s| {
                let (sin, cos) = (s as f32 * std::f32::consts::TAU / SECTORS as f32).sin_cos();
                std::array::from_fn::<_, 3, _>(|i| u[i] * cos + v[i] * sin)
            });
            let ring = radial.map(|r| std::array::from_fn(|i| base[i] + radius * r[i]));
            let normal: [f32; 3] = std::array::from_fn(|i| {
                (radial[0][i] + radial[1][i]) * 0.5 * height + d[i] * radius
            });
            let len = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
            let normal = normal.map(|v| v / len);
            let offset = vertices.len() as u32;
            for (position, normal) in [
                (ring[0], normal),
                (ring[1], normal),
                (tip, normal),
                (ring[1], d.map(|v| -v)),
                (ring[0], d.map(|v| -v)),
                (base, d.map(|v| -v)),
            ] {
                vertices.push(MeshVertex {
                    position,
                    normal,
                    color: cone.color,
                });
            }
            indices.extend((0..6).map(|i| offset + i));
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_entry_and_exit_tips_stay_on_the_travel_endpoint() {
        let at = [4., 5., 6.];
        let cones = [
            PathCone {
                at,
                direction: [0., 0., -1.],
                color: PATH_IN,
                op: 7,
            },
            PathCone {
                at,
                direction: [0., 0., 1.],
                color: PATH_OUT,
                op: 7,
            },
        ];
        let (vertices, _) = path_cone_mesh(&cones, 2.);
        for (i, cone) in cones.iter().enumerate() {
            let mesh = &vertices[i * 16 * 6..(i + 1) * 16 * 6];
            for sector in mesh.chunks_exact(6) {
                assert_eq!(
                    sector[2].position, at,
                    "the arrow tip must not be visually displaced"
                );
                let base = sector[5].position;
                assert_eq!(
                    base,
                    std::array::from_fn(|j| at[j] - 2. * cone.direction[j])
                );
            }
        }
    }

    #[test]
    fn cones_have_outward_faces_and_point_along_travel_in_any_orientation() {
        for direction in [
            [0., 0., 1.],
            [0., 0., -1.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0.6, 0., 0.8],
        ] {
            let cone = PathCone {
                at: [4., 5., 6.],
                direction,
                color: PATH_OUT,
                op: 7,
            };
            let (vertices, indices) = path_cone_mesh(&[cone], 2.);
            assert_eq!(indices.len(), 16 * 6);
            for triangle in indices.as_chunks::<3>().0 {
                let [a, b, c] = std::array::from_fn(|i| vertices[triangle[i] as usize]);
                let u: [f32; 3] = std::array::from_fn(|i| b.position[i] - a.position[i]);
                let v: [f32; 3] = std::array::from_fn(|i| c.position[i] - a.position[i]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                assert!((0..3).map(|i| cross[i] * a.normal[i]).sum::<f32>() > 0.);
            }
            for vertex in vertices {
                assert_eq!(vertex.color, PATH_OUT);
                let axial: f32 = (0..3)
                    .map(|i| (vertex.position[i] - cone.at[i]) * direction[i])
                    .sum();
                assert!(axial.abs() < 1e-5 || (axial + 2.).abs() < 1e-5);
                assert!((vertex.normal.iter().map(|n| n * n).sum::<f32>() - 1.).abs() < 1e-5);
            }
        }
    }
}
