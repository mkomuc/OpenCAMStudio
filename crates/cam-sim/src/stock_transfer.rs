//! Rasterize a closed remaining-stock mesh in the next setup's axes. Columns
//! retain disjoint material intervals, so flipping a pocket preserves its floor
//! and cavity instead of filling the blank back into a bounding box.
use super::{Heightfield, RemovedSpan, SurfaceMesh};

#[derive(Clone, Copy)]
struct Hit {
    z: f32,
    entering: bool,
    operation: Option<u32>,
}

impl Heightfield {
    /// Sample a closed, outward-wound solid already transformed into the new
    /// machining axes. Like cutting simulation, accuracy is limited by `res`.
    pub fn from_stock_mesh_controlled(
        mesh: &SurfaceMesh,
        min: [f64; 3],
        max: [f64; 3],
        res: f64,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Self> {
        if cancelled() {
            return None;
        }
        let mut field = Self::new([min[0], min[1]], [max[0], max[1]], res, max[2]);
        let mut columns: Vec<Vec<Hit>> = vec![Vec::new(); field.z.len()];
        for triangle in mesh.indices.as_chunks::<3>().0 {
            if cancelled() {
                return None;
            }
            let [a, b, c] = std::array::from_fn::<_, 3, _>(|i| {
                mesh.positions[triangle[i] as usize].map(f64::from)
            });
            let cross = |u: [f64; 2], v: [f64; 2]| u[0] * v[1] - u[1] * v[0];
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ac = [c[0] - a[0], c[1] - a[1]];
            let area = cross(ab, ac);
            if area.abs() < 1e-12 {
                continue;
            }
            let (x0, x1) = field.index_range(a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]), 0);
            let (y0, y1) = field.index_range(a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]), 1);
            let operation = triangle
                .iter()
                .find_map(|&i| mesh.operations.get(i as usize).copied().flatten());
            for y in y0..=y1 {
                if cancelled() {
                    return None;
                }
                for x in x0..=x1 {
                    let (cx, cy) = field.center(x, y);
                    let ap = [cx - a[0], cy - a[1]];
                    let u = cross(ap, ac) / area;
                    let v = cross(ab, ap) / area;
                    if u >= -1e-9 && v >= -1e-9 && u + v <= 1. + 1e-9 {
                        columns[y * field.nx + x].push(Hit {
                            z: (a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2])) as f32,
                            entering: area < 0.,
                            operation,
                        });
                    }
                }
            }
        }
        field.inherited_operations = vec![None; field.z.len()];
        for (index, hits) in columns.iter_mut().enumerate() {
            if index % field.nx == 0 && cancelled() {
                return None;
            }
            hits.sort_by(|a, b| a.z.total_cmp(&b.z));
            // A ray on a shared triangle edge can hit a face twice. Collapse
            // coincident boundaries before extracting material intervals.
            hits.dedup_by(|a, b| (a.z - b.z).abs() < 1e-5 && a.entering == b.entering);
            let mut lower = min[2] as f32;
            let mut lower_op = None;
            let mut voids = Vec::new();
            let mut top = min[2] as f32;
            for hit in hits.iter() {
                let z = hit.z.clamp(min[2] as f32, max[2] as f32);
                if hit.entering {
                    if z > lower + 1e-5 {
                        voids.push(RemovedSpan {
                            lower,
                            upper: z,
                            lower_op,
                            upper_op: hit.operation,
                        });
                    }
                } else {
                    top = z;
                    lower = z;
                    lower_op = hit.operation;
                    field.inherited_operations[index] = hit.operation;
                }
            }
            field.z[index] = top;
            if !voids.is_empty() {
                field.bounded.insert(index, voids);
            }
        }
        Some(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flipped_pocket_keeps_material_above_its_cavity() {
        let mut original = Heightfield::new([0., 0.], [10., 10.], 0.5, 0.);
        original.lower_rect([2., 2.], [8., 8.], -3.);
        let mut mesh = original.to_solid_mesh(-5.);
        for p in &mut mesh.positions {
            *p = [p[0], -p[1], -p[2]];
        }
        let mut flipped = Heightfield::from_stock_mesh_controlled(
            &mesh,
            [0., -10., 0.],
            [10., 0., 5.],
            0.5,
            &|| false,
        )
        .unwrap();
        assert_eq!(flipped.sample(5., -5.), 5.);
        // Remove the flipped floor, revealing the original pocket underneath.
        flipped.lower_rect([4., -6.], [6., -4.], 2.);
        assert_eq!(flipped.sample(5., -5.), 0.);
        assert_eq!(flipped.sample(1., -1.), 5.);
        assert!(!flipped.to_solid_mesh(0.).indices.is_empty());
    }

    #[test]
    fn mesh_transfer_can_be_cancelled() {
        assert!(Heightfield::from_stock_mesh_controlled(
            &SurfaceMesh::default(),
            [0.; 3],
            [10.; 3],
            0.5,
            &|| true,
        )
        .is_none());
    }

    #[test]
    fn rotated_stock_keeps_the_pocket_and_empty_corners_of_its_enclosure() {
        let mut original = Heightfield::new([0., 0.], [10., 10.], 0.2, 0.);
        original.lower_rect([3., 3.], [7., 7.], -3.);
        let mut mesh = original.to_solid_mesh(-5.);
        let c = std::f32::consts::FRAC_1_SQRT_2;
        for p in &mut mesh.positions {
            *p = [c * (p[0] - p[1]), c * (p[0] + p[1]), p[2]];
        }
        let field = Heightfield::from_stock_mesh_controlled(
            &mesh,
            [-7.071068, 0., -5.],
            [7.071068, 14.142136, 0.],
            0.2,
            &|| false,
        )
        .unwrap();
        assert_eq!(field.sample(0., 7.), -3.);
        assert_eq!(field.sample(0., 2.), 0.);
        assert_eq!(
            field.sample(-6., 1.),
            -5.,
            "enclosing-box corners contain no material"
        );
    }
}
