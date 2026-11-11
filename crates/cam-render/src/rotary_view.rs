//! A single display transform for a rotating workpiece. Material removal stays in
//! its stationary frame; this only maps the published geometry for viewing.
use crate::MeshVertex;
use cam_cldata::{Point3, RotaryAxis};
use cam_model::SetupFrame;

#[derive(Clone, Copy, Debug)]
pub struct RotaryStockTransform {
    rotation: [[f64; 3]; 3],
    translation: [f64; 3],
}

impl RotaryStockTransform {
    /// Invert the material rotation in setup coordinates, then map back to world
    /// coordinates. Setup datums do not translate the world-oriented geometry.
    pub fn new(frame: &SetupFrame, axis: RotaryAxis, center: Point3, angle: f64) -> Self {
        let (s, c) = (-angle).to_radians().sin_cos();
        let local = match axis {
            RotaryAxis::A => [[1., 0., 0.], [0., c, s], [0., -s, c]],
            RotaryAxis::B => [[c, 0., -s], [0., 1., 0.], [s, 0., c]],
        };
        let world = frame.world_from_setup.rotation;
        let intermediate: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|k| world[i][k] * local[k][j]).sum())
        });
        // F * R * inverse(F), where a validated setup's inverse is its transpose.
        let rotation = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|k| intermediate[i][k] * world[j][k]).sum())
        });
        let center = [center.x, center.y, center.z];
        let offset = std::array::from_fn::<_, 3, _>(|i| {
            center[i] - (0..3).map(|j| local[i][j] * center[j]).sum::<f64>()
        });
        let translation = world.map(|row| row.into_iter().zip(offset).map(|(a, b)| a * b).sum());
        Self {
            rotation,
            translation,
        }
    }

    /// Rotate a world-space point about the workpiece's axis center.
    pub fn point(&self, point: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = point.map(f64::from);
        std::array::from_fn(|i| {
            let r = self.rotation[i];
            (r[0] * x + r[1] * y + r[2] * z + self.translation[i]) as f32
        })
    }

    /// Rotate a world-space direction/normal without the center offset.
    pub fn vector(&self, vector: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = vector.map(f64::from);
        self.rotation
            .map(|r| (r[0] * x + r[1] * y + r[2] * z) as f32)
    }

    /// Keep the complete bulk loop in the optimized renderer even in dev builds.
    /// No per-vertex trigonometry or setup-frame traversal is necessary.
    pub fn apply_mesh(&self, vertices: &mut [MeshVertex]) {
        for vertex in vertices {
            vertex.position = self.point(vertex.position);
            vertex.normal = self.vector(vertex.normal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(
        frame: &SetupFrame,
        axis: RotaryAxis,
        center: Point3,
        angle: f64,
        point: [f32; 3],
        vector: bool,
    ) -> [f32; 3] {
        let p = frame.to_setup_vector(point.map(f64::from));
        let origin = if vector {
            Point3::new(0., 0., 0.)
        } else {
            center
        };
        let p = axis.material_point(Point3::new(p[0], p[1], p[2]), origin, -angle);
        frame.to_world_vector([p.x, p.y, p.z]).map(|v| v as f32)
    }

    #[test]
    fn composed_rotation_matches_scalar_setup_conversion_and_preserves_normals() {
        for frame in [
            SetupFrame::default(),
            SetupFrame::from_xy([0., 1., 0.], [-1., 0., 0.]).unwrap(),
            SetupFrame::from_xy([1., 2., 3.], [-2., 1., 0.]).unwrap(),
        ] {
            for axis in [RotaryAxis::A, RotaryAxis::B] {
                for angle in [-720., -179.3, -90., 0., 37., 90., 173., 720.] {
                    let center = Point3::new(3., 4., 5.);
                    let transform = RotaryStockTransform::new(&frame, axis, center, angle);
                    let mut vertices: Vec<_> = (0..100)
                        .map(|i| MeshVertex {
                            position: [i as f32 * 0.3, (i as f32).sin() * 20., i as f32 * -0.2],
                            normal: [0.6, 0.8, 0.],
                            color: [0.1, 0.2, 0.3, 0.4],
                        })
                        .collect();
                    let original = vertices.clone();
                    transform.apply_mesh(&mut vertices);
                    for (before, after) in original.iter().zip(&vertices) {
                        let expected =
                            reference(&frame, axis, center, angle, before.position, false);
                        let normal = reference(&frame, axis, center, angle, before.normal, true);
                        for i in 0..3 {
                            assert!((after.position[i] - expected[i]).abs() < 1e-5);
                            assert!((after.normal[i] - normal[i]).abs() < 1e-6);
                        }
                        assert_eq!(before.color, after.color);
                        assert!(
                            (after.normal.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-6
                        );
                    }
                }
            }
        }
    }
}
