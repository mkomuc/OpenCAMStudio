//! Tessellated-solid comparison, using the CAD scene's existing BVH.
use super::*;

impl CadScene {
    /// Distance to the nearest target face, negative inside any target solid:
    /// positive is excess stock, negative is a gouge. Requires closed solids.
    /// The result has the accuracy of the imported display tessellation.
    pub fn surface_deviation(&self, point: [f64; 3]) -> Option<f64> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut nearest = f64::INFINITY;
        self.nearest_surface(point, 0, &mut nearest);
        if nearest < 1e-18 {
            return Some(0.);
        }
        // An oblique ray avoids the grid-aligned edges of most milling models.
        let ray = Ray {
            origin: point,
            direction: [0.812381, 0.331197, 0.479113],
        };
        let mut hits = Vec::new();
        self.ray_hits(ray, 0, &mut hits);
        let mut bodies: BTreeMap<(InstanceId, u64), Vec<f64>> = BTreeMap::new();
        for (index, depth) in hits {
            let face = &self.triangles[index].face;
            bodies
                .entry((face.instance_id, face.body_id))
                .or_default()
                .push(depth);
        }
        let inside = bodies.values_mut().any(|depths| {
            depths.sort_by(f64::total_cmp);
            depths.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
            depths.len() % 2 == 1
        });
        Some(nearest.sqrt() * if inside { -1. } else { 1. })
    }

    fn nearest_surface(&self, point: Point, id: usize, nearest: &mut f64) {
        let node = &self.nodes[id];
        if box_distance_squared(point, node.bounds) > *nearest {
            return;
        }
        if let Some((a, b)) = node.children {
            let (a, b) = if box_distance_squared(point, self.nodes[a].bounds)
                <= box_distance_squared(point, self.nodes[b].bounds)
            {
                (a, b)
            } else {
                (b, a)
            };
            self.nearest_surface(point, a, nearest);
            self.nearest_surface(point, b, nearest);
        } else {
            for &index in &node.triangles {
                *nearest = nearest.min(triangle_distance_squared(
                    point,
                    self.triangles[index].points,
                ));
            }
        }
    }
}

fn box_distance_squared(point: Point, bounds: [Point; 2]) -> f64 {
    (0..3)
        .map(|axis| {
            let d = point[axis] - point[axis].clamp(bounds[0][axis], bounds[1][axis]);
            d * d
        })
        .sum()
}

fn triangle_distance_squared(point: Point, triangle: [Point; 3]) -> f64 {
    let [a, b, c] = triangle;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let normal = cross(ab, ac);
    let n2 = dot(normal, normal);
    let ap = sub(point, a);
    let d = dot(ap, normal);
    if n2 > 1e-24 {
        let projected = sub(point, scale(normal, d / n2));
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(u, v)| dot(cross(sub(v, u), sub(projected, u)), normal) >= -1e-12 * n2);
        if inside {
            return d * d / n2;
        }
    }
    [(a, b), (b, c), (c, a)]
        .iter()
        .map(|&(u, v)| {
            let edge = sub(v, u);
            let t = (dot(sub(point, u), edge) / dot(edge, edge).max(1e-30)).clamp(0., 1.);
            let delta = sub(point, std::array::from_fn(|i| u[i] + edge[i] * t));
            dot(delta, delta)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_distinguishes_excess_contact_and_gouges_in_a_solid_union() {
        let mut scene = CadScene::default();
        for instance in [1, 2] {
            let offset = (instance - 1) as f64 * 1.5;
            let p: [Point; 8] = std::array::from_fn(|i| {
                [
                    (i & 1) as f64 * 2. + offset,
                    ((i >> 1) & 1) as f64 * 2.,
                    ((i >> 2) & 1) as f64 * 2.,
                ]
            });
            for [a, b, c, d] in [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ] {
                for ids in [[a, b, c], [a, c, d]] {
                    scene.triangles.push(Triangle {
                        points: ids.map(|i| p[i]),
                        face: EntityRef {
                            asset_id: 1,
                            revision: 1,
                            instance_id: instance,
                            body_id: 1,
                            kind: EntityKind::Face,
                            id: 1,
                        },
                    });
                }
            }
        }
        scene.build_node((0..scene.triangles.len()).collect());
        assert_eq!(scene.surface_deviation([1., 1., 2.]), Some(0.));
        assert!((scene.surface_deviation([1., 1., 2.2]).unwrap() - 0.2).abs() < 1e-9);
        assert!(scene.surface_deviation([1., 1., 1.8]).unwrap() < 0.);
        assert!(scene.surface_deviation([1.75, 1., 1.]).unwrap() < 0.);
        assert_eq!(CadScene::default().surface_deviation([0.; 3]), None);
    }
}
