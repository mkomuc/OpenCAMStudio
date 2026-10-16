//! Compact stock meshing. Flat interiors become rectangles; only the changing
//! surface is tessellated. Dual contour vertices recover walls from the actual
//! cutter sweeps instead of following the edges of square heightfield cells.
use super::{Heightfield, SurfaceMesh};

#[derive(Clone, Copy)]
struct Corner {
    xy: [f32; 2],
    normal: Option<[f32; 3]>,
}
impl SurfaceMesh {
    fn top_quad(&mut self, p: [[f32; 3]; 4], n: [[f32; 3]; 4], cut: bool, top: f32) {
        let flags = [[0, 1, 2], [0, 2, 3]].map(|ids| cut && ids.iter().any(|&i| p[i][2] < top));
        if flags[0] == flags[1] {
            self.quad(p, n, flags[0]);
        } else {
            for (ids, cut) in [[0, 1, 2], [0, 2, 3]].into_iter().zip(flags) {
                let base = self.positions.len() as u32;
                self.positions.extend(ids.map(|i| p[i]));
                self.normals.extend(ids.map(|i| n[i]));
                self.machined.extend([cut; 3]);
                self.indices.extend([base, base + 1, base + 2]);
            }
        }
    }
    fn quad(&mut self, p: [[f32; 3]; 4], n: [[f32; 3]; 4], cut: bool) {
        let base = self.positions.len() as u32;
        self.positions.extend(p);
        self.normals.extend(n);
        self.machined.extend([cut; 4]);
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}
impl Heightfield {
    fn height(&self, x: isize, y: isize) -> Option<f32> {
        if x < 0 || y < 0 || x >= self.nx as isize || y >= self.ny as isize {
            None
        } else {
            Some(self.z[y as usize * self.nx + x as usize])
        }
    }
    fn grid_xy(&self, x: usize, y: usize) -> [f32; 2] {
        [
            (self.origin[0] + x as f64 * self.res).min(self.max[0]) as f32,
            (self.origin[1] + y as f64 * self.res).min(self.max[1]) as f32,
        ]
    }
    fn contour_corner(&self, x: usize, y: usize, wall_step: f32) -> Corner {
        let mut corner = Corner {
            xy: self.grid_xy(x, y),
            normal: None,
        };
        if x == 0 || y == 0 || x == self.nx || y == self.ny {
            return corner;
        }
        let ids = [
            (y - 1) * self.nx + x - 1,
            (y - 1) * self.nx + x,
            y * self.nx + x,
            y * self.nx + x - 1,
        ];
        let heights = ids.map(|i| self.z[i]);
        if heights.iter().all(|&z| z == heights[0]) {
            return corner;
        }
        let mut sorted = heights;
        sorted.sort_unstable_by(f32::total_cmp);
        let (gap, split) = (0..3)
            .map(|i| (sorted[i + 1] - sorted[i], i))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        if gap <= wall_step {
            return corner;
        }
        let level = f64::from((sorted[split] + sorted[split + 1]) * 0.5);
        let low = heights.map(|h| f64::from(h) < level);
        // A saddle contains two separate contours; a single dual vertex must not
        // connect the two voids or erase the intervening material.
        if low[0] == low[2] && low[1] == low[3] {
            return corner;
        }
        let mut cutters = Vec::with_capacity(4);
        for i in 0..4 {
            let id = self.winners[ids[i]];
            if low[i] && id != u32::MAX && !cutters.contains(&id) {
                cutters.push(id);
            }
        }
        if cutters.is_empty() {
            return corner;
        }
        let implicit = |p: [f64; 2]| {
            cutters
                .iter()
                .map(|&id| self.cuts[id as usize].implicit(p[0], p[1], level))
                .fold(f64::INFINITY, f64::min)
        };
        let centers = [
            self.center(x - 1, y - 1),
            self.center(x, y - 1),
            self.center(x, y),
            self.center(x - 1, y),
        ];
        let origin = corner.xy.map(f64::from);
        // Solve a small Hermite QEF in local coordinates. The weak centre anchor
        // handles parallel normals without drifting along straight walls.
        let mut aa = 1e-5;
        let mut ab = 0.;
        let mut bb = 1e-5;
        let mut rhs = [0.; 2];
        let mut normal = [0.; 2];
        let mut count = 0;
        for i in 0..4 {
            let j = (i + 1) % 4;
            if low[i] == low[j] {
                continue;
            }
            let mut a = [centers[i].0, centers[i].1];
            let mut b = [centers[j].0, centers[j].1];
            let inside = implicit(a) <= 0.;
            if inside == (implicit(b) <= 0.) {
                continue;
            }
            for _ in 0..12 {
                let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                if (implicit(m) <= 0.) == inside {
                    a = m;
                } else {
                    b = m;
                }
            }
            let p = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let e = self.res * 0.001;
            let mut n = [
                implicit([p[0] + e, p[1]]) - implicit([p[0] - e, p[1]]),
                implicit([p[0], p[1] + e]) - implicit([p[0], p[1] - e]),
            ];
            let len = n[0].hypot(n[1]);
            if len < 1e-12 {
                continue;
            }
            n = n.map(|v| v / len);
            let d = n[0] * (p[0] - origin[0]) + n[1] * (p[1] - origin[1]);
            aa += n[0] * n[0];
            ab += n[0] * n[1];
            bb += n[1] * n[1];
            rhs[0] += n[0] * d;
            rhs[1] += n[1] * d;
            normal[0] -= n[0];
            normal[1] -= n[1];
            count += 1;
        }
        if count > 0 {
            let det = aa * bb - ab * ab;
            let delta = [
                (rhs[0] * bb - rhs[1] * ab) / det,
                (rhs[1] * aa - rhs[0] * ab) / det,
            ];
            corner.xy = std::array::from_fn(|i| {
                (origin[i] + delta[i]).clamp(
                    [centers[0].0, centers[0].1][i],
                    [centers[2].0, centers[2].1][i],
                ) as f32
            });
            let len = normal[0].hypot(normal[1]);
            if len > 1e-9 {
                corner.normal = Some([(normal[0] / len) as f32, (normal[1] / len) as f32, 0.]);
            }
        }
        corner
    }
    /// Surface at a shared corner, excluding neighbours across a vertical wall.
    /// Compatible slopes share heights and normals, so curved floors shade smoothly.
    fn corner_surface(&self, x: usize, y: usize, z: f32, wall_step: f32) -> (f32, [f32; 3], bool) {
        let (x, y) = (x as isize, y as isize);
        let mut h =
            [(x - 1, y - 1), (x, y - 1), (x, y), (x - 1, y)].map(|(i, j)| self.height(i, j));
        let mut sorted = h.map(|v| v.unwrap_or(z));
        sorted.sort_unstable_by(f32::total_cmp);
        let mut low = f32::NEG_INFINITY;
        let mut high = f32::INFINITY;
        for pair in sorted.windows(2) {
            if pair[1] - pair[0] > wall_step {
                if z <= pair[0] {
                    high = high.min(pair[0]);
                }
                if z >= pair[1] {
                    low = low.max(pair[1]);
                }
            }
        }
        h = h.map(|v| v.filter(|v| *v >= low && *v <= high));
        let mut sum = 0.;
        let mut count = 0.;
        let mut cut = false;
        for v in h.into_iter().flatten() {
            sum += v;
            count += 1.;
            cut |= v < self.top;
        }
        let derivative = |edges: [(usize, usize); 2]| {
            let mut sum = 0.;
            let mut count = 0.;
            for (a, b) in edges {
                if let (Some(a), Some(b)) = (h[a], h[b]) {
                    sum += (b - a) / self.res as f32;
                    count += 1.;
                }
            }
            if count > 0. {
                sum / count
            } else {
                0.
            }
        };
        let dx = derivative([(0, 1), (3, 2)]);
        let dy = derivative([(0, 3), (1, 2)]);
        let len = (dx * dx + dy * dy + 1.).sqrt();
        (sum / count, [-dx / len, -dy / len, 1. / len], cut)
    }
    /// Triangulate a solid stock with analytic sub-cell wall contours, shared
    /// smooth slope normals, and merged flat interiors. Verification retains the
    /// original sampled heights; contour recovery changes only the display mesh.
    pub fn to_solid_mesh(&self, floor: f64) -> SurfaceMesh {
        self.to_solid_mesh_controlled(floor, &|| false).unwrap()
    }
    pub fn to_solid_mesh_controlled(
        &self,
        floor: f64,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<SurfaceMesh> {
        let f = floor as f32;
        let wall_step = self.res as f32 * 2.5;
        let mut corners = Vec::with_capacity((self.nx + 1) * (self.ny + 1));
        for y in 0..=self.ny {
            if cancelled() {
                return None;
            }
            for x in 0..=self.nx {
                corners.push(self.contour_corner(x, y, wall_step));
            }
        }
        let mut flat = vec![0u8; self.z.len()];
        for y in 0..self.ny {
            if cancelled() {
                return None;
            }
            for x in 0..self.nx {
                let z = self.z[y * self.nx + x];
                flat[y * self.nx + x] = u8::from(
                    z > f + 1e-4
                        && (-1..=1).all(|dy| {
                            (-1..=1).all(|dx| {
                                self.height(x as isize + dx, y as isize + dy)
                                    .is_none_or(|v| v == z)
                            })
                        }),
                );
            }
        }
        let mut mesh = SurfaceMesh::default();
        let up = [[0., 0., 1.]; 4];
        let down = [[0., 0., -1.]; 4];
        // Greedy rectangles leave a one-cell collar around changing heights.
        // That collar shares all contour vertices with walls and neighbouring tops.
        for y in 0..self.ny {
            if cancelled() {
                return None;
            }
            for x in 0..self.nx {
                let id = y * self.nx + x;
                if flat[id] != 1 {
                    continue;
                }
                let z = self.z[id];
                let mut end_x = x + 1;
                while end_x < self.nx
                    && flat[y * self.nx + end_x] == 1
                    && self.z[y * self.nx + end_x] == z
                {
                    end_x += 1;
                }
                let mut end_y = y + 1;
                while end_y < self.ny
                    && (x..end_x)
                        .all(|i| flat[end_y * self.nx + i] == 1 && self.z[end_y * self.nx + i] == z)
                {
                    if cancelled() {
                        return None;
                    }
                    end_y += 1;
                }
                for j in y..end_y {
                    for i in x..end_x {
                        flat[j * self.nx + i] = 2;
                    }
                }
                // Mark merged cells separately, without allocating another bitmap.
                let a = self.grid_xy(x, y);
                let b = self.grid_xy(end_x, end_y);
                mesh.quad(
                    [
                        [a[0], a[1], z],
                        [b[0], a[1], z],
                        [b[0], b[1], z],
                        [a[0], b[1], z],
                    ],
                    up,
                    z < self.top,
                );
                mesh.quad(
                    [
                        [a[0], a[1], f],
                        [a[0], b[1], f],
                        [b[0], b[1], f],
                        [b[0], a[1], f],
                    ],
                    down,
                    false,
                );
            }
        }
        // Boundary cells and slopes retain local detail; merged interiors need no more geometry.
        for y in 0..self.ny {
            if cancelled() {
                return None;
            }
            for x in 0..self.nx {
                let z = self.z[y * self.nx + x];
                if z <= f + 1e-4 {
                    continue;
                }
                let interior = flat[y * self.nx + x] == 2;
                if interior && x > 0 && y > 0 && x + 1 < self.nx && y + 1 < self.ny {
                    continue;
                }
                let coords = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
                let c = coords.map(|(i, j)| corners[j * (self.nx + 1) + i]);
                let h = coords.map(|(i, j)| self.corner_surface(i, j, z, wall_step));
                let p: [[f32; 3]; 4] =
                    std::array::from_fn(|i| [c[i].xy[0], c[i].xy[1], h[i].0.max(f)]);
                if !interior {
                    mesh.top_quad(p, h.map(|v| v.1), h.iter().any(|v| v.2), self.top);
                    mesh.quad(
                        [p[0], p[3], p[2], p[1]].map(|v| [v[0], v[1], f]),
                        down,
                        false,
                    );
                }
                // CCW boundary of this top; reverse it for outward-facing walls.
                for (a, b, dx, dy, axis) in [
                    (0, 3, -1, 0, [-1., 0., 0.]),
                    (2, 1, 1, 0, [1., 0., 0.]),
                    (1, 0, 0, -1, [0., -1., 0.]),
                    (3, 2, 0, 1, [0., 1., 0.]),
                ] {
                    let nb = self.height(x as isize + dx, y as isize + dy);
                    if nb.is_some_and(|v| v > f + 1e-4 && z - v <= wall_step) {
                        continue;
                    }
                    let lower = |i: usize| {
                        nb.map_or(f, |v| {
                            let (cx, cy) = coords[i];
                            self.corner_surface(cx, cy, v, wall_step).0.max(f)
                        })
                    };
                    let n = |i: usize| {
                        c[i].normal
                            .filter(|n| n[0] * axis[0] + n[1] * axis[1] > 0.01)
                            .unwrap_or(axis)
                    };
                    mesh.quad(
                        [
                            p[a],
                            p[b],
                            [p[b][0], p[b][1], lower(b)],
                            [p[a][0], p[a][1], lower(a)],
                        ],
                        [n(a), n(b), n(b), n(a)],
                        nb.is_some(),
                    );
                }
            }
        }
        Some(mesh)
    }
}
