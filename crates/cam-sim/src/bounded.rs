//! Sparse Z intervals removed by side-cutting teeth. The ordinary heightfield
//! remains compact; only columns touched by these finite cuts need extra detail.
use super::{Heightfield, SurfaceMesh};
use crate::{sweep::Sweep, ProfileShape, ToolProfile};

#[derive(Clone, Copy, Debug)]
pub(super) struct RemovedSpan {
    pub lower: f32,
    pub upper: f32,
    pub lower_op: Option<u32>,
    pub upper_op: Option<u32>,
    pub lower_source: u32,
    pub upper_source: u32,
}

#[derive(Clone, Copy)]
struct MaterialSpan {
    lower: f32,
    upper: f32,
    lower_op: Option<u32>,
    upper_op: Option<u32>,
    lower_cut: bool,
    upper_cut: bool,
    upper_thread: bool,
    lower_source: u32,
    upper_source: u32,
    lower_cylinder: Option<(f64, bool)>,
    upper_cylinder: Option<(f64, bool)>,
    lower_radial: Option<(f64, bool)>,
    upper_radial: Option<(f64, bool)>,
}

#[derive(Clone, Copy)]
struct Boundary {
    z: f32,
    thread: bool,
    lower_void: bool,
    operation: Option<u32>,
    cut: u32,
    cylinder: Option<(f64, bool)>,
    radial: Option<(f64, bool)>,
}

impl MaterialSpan {
    fn boundaries(self) -> [Boundary; 2] {
        [
            Boundary {
                z: self.lower,
                thread: self.lower_cut,
                lower_void: false,
                operation: self.lower_op,
                cut: self.lower_source,
                cylinder: self.lower_cylinder,
                radial: self.lower_radial,
            },
            Boundary {
                z: self.upper,
                thread: self.upper_thread,
                lower_void: true,
                operation: self.upper_op,
                cut: self.upper_source,
                cylinder: self.upper_cylinder,
                radial: self.upper_radial,
            },
        ]
    }
}

impl Heightfield {
    pub(crate) fn cut_thread_segment(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        tool: &ToolProfile,
        operation: Option<u32>,
        cancelled: &dyn Fn() -> bool,
    ) {
        let ProfileShape::Thread {
            root_radius,
            tooth_height,
            pitch,
            tooth_count,
        } = tool.shape
        else {
            return;
        };
        let width = tool.radius - root_radius;
        if width <= 1e-9 || tooth_height <= 0. {
            return;
        }
        let mut sweep = Sweep::new(a, b, *tool);
        sweep.source = self.cut_source;
        let cut = if self.surface_tracking {
            let cut = self.cuts.len() as u32;
            self.cuts.push(sweep);
            cut
        } else {
            u32::MAX
        };
        let (y0, y1) = self.index_range(
            a[1].min(b[1]) - tool.radius,
            a[1].max(b[1]) + tool.radius,
            1,
        );
        for y in y0..=y1 {
            if cancelled() {
                return;
            }
            let cy = self.center(0, y).1;
            let Some((lo, hi)) = sweep.row_span(cy) else {
                continue;
            };
            let (x0, x1) = self.index_range(lo, hi, 0);
            let mut changed = false;
            for x in x0..=x1 {
                let (cx, cy) = self.center(x, y);
                let (t, d2) = sweep.projection(cx, cy);
                let d = d2.sqrt();
                if d < root_radius || d > tool.radius {
                    continue;
                }
                let flank = (d - root_radius) / width * tooth_height * 0.5;
                let z = a[2] + (b[2] - a[2]) * t;
                let index = y * self.nx + x;
                for tooth in 0..tooth_count {
                    let bottom = z + f64::from(tooth) * pitch;
                    let lower = (bottom + flank) as f32;
                    let upper = ((bottom + tooth_height - flank) as f32).min(self.z[index]);
                    if upper - lower <= 1e-6 {
                        continue;
                    }
                    changed = true;
                    self.track_removed_interval(index, f64::from(lower), f64::from(upper));
                    let spans = self.bounded.entry(index).or_default();
                    spans.push(RemovedSpan {
                        lower,
                        upper,
                        lower_op: operation,
                        upper_op: operation,
                        lower_source: cut,
                        upper_source: cut,
                    });
                    spans.sort_by(|a, b| a.lower.total_cmp(&b.lower));
                    let mut count = 0;
                    for i in 0..spans.len() {
                        let span = spans[i];
                        if count > 0 && span.lower <= spans[count - 1].upper + 1e-6 {
                            let previous = &mut spans[count - 1];
                            if span.upper > previous.upper {
                                previous.upper = span.upper;
                                previous.upper_op = span.upper_op;
                                previous.upper_source = span.upper_source;
                            }
                        } else {
                            spans[count] = span;
                            count += 1;
                        }
                    }
                    spans.truncate(count);
                }
            }
            if changed {
                self.invalidate_mesh_region([x0, y], [x1, y]);
            }
        }
    }

    pub(super) fn remaining_top(&self, index: usize) -> f32 {
        let mut top = self.z[index];
        if let Some(spans) = self.bounded.get(&index) {
            for span in spans.iter().rev() {
                if span.upper >= top && span.lower < top {
                    top = span.lower;
                }
            }
        }
        top
    }

    /// Include a collar so both sides of every newly opened wall are meshed
    /// with the same column representation.
    pub(super) fn detailed_column(&self, x: usize, y: usize) -> bool {
        if self.bounded.is_empty() {
            return false;
        }
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (x, y) = (x as isize + dx, y as isize + dy);
                x >= 0
                    && y >= 0
                    && x < self.nx as isize
                    && y < self.ny as isize
                    && self
                        .bounded
                        .contains_key(&(y as usize * self.nx + x as usize))
            })
        })
    }

    fn material_spans(&self, x: usize, y: usize, floor: f32) -> Vec<MaterialSpan> {
        let index = y * self.nx + x;
        let top = self.z[index];
        let mut result = Vec::new();
        let mut lower = floor;
        let mut lower_op = None;
        let mut lower_cut = false;
        let mut lower_source = u32::MAX;
        if let Some(spans) = self.bounded.get(&index) {
            for span in spans {
                if span.upper <= floor || span.lower >= top {
                    continue;
                }
                let upper = span.lower.max(floor);
                if upper > lower {
                    result.push(MaterialSpan {
                        lower,
                        upper,
                        lower_op,
                        upper_op: span.lower_op,
                        lower_cut,
                        upper_cut: span.lower_op.is_some() || span.lower_source != u32::MAX,
                        upper_thread: true,
                        lower_source,
                        upper_source: span.lower_source,
                        lower_cylinder: None,
                        upper_cylinder: None,
                        lower_radial: None,
                        upper_radial: None,
                    });
                }
                lower = lower.max(span.upper);
                lower_op = span.upper_op;
                lower_cut = span.upper_op.is_some() || span.upper_source != u32::MAX;
                lower_source = span.upper_source;
            }
        }
        if top > lower {
            result.push(MaterialSpan {
                lower,
                upper: top,
                lower_op,
                upper_op: self.operation_at(x, y),
                lower_cut,
                upper_cut: self.top_is_cut(index),
                upper_thread: false,
                lower_source,
                upper_source: self.winners[index],
                lower_cylinder: None,
                upper_cylinder: None,
                lower_radial: None,
                upper_radial: None,
            });
        }
        if let Some(cylinder) = self.horizontal_cylinder {
            let transverse = 1 - cylinder.axis;
            let (cx, cy) = self.center(x, y);
            let offset = [cx, cy][transverse] - cylinder.center[transverse];
            let classify = |height: f32, cut: bool| {
                if cut {
                    return None;
                }
                for radius in [cylinder.outer_radius, cylinder.inner_radius] {
                    if radius <= 0. || offset.abs() > radius {
                        continue;
                    }
                    let h = (radius * radius - offset * offset).max(0.).sqrt();
                    for positive in [false, true] {
                        let z = cylinder.center[2] + if positive { h } else { -h };
                        if z as f32 == height {
                            return Some((radius, positive));
                        }
                    }
                }
                None
            };
            for span in &mut result {
                span.lower_cylinder = classify(span.lower, span.lower_cut);
                span.upper_cylinder = classify(span.upper, span.upper_cut);
                let radial = |height: f32, cut: bool, operation: Option<u32>| {
                    if !cut || operation.is_none() {
                        return None;
                    }
                    let z = f64::from(height) - cylinder.center[2];
                    Some((offset.hypot(z), z >= 0.))
                };
                span.lower_radial = radial(span.lower, span.lower_cut, span.lower_op);
                span.upper_radial = radial(span.upper, span.upper_cut, span.upper_op);
            }
        }
        result
    }

    fn air_operation(&self, x: usize, y: usize, z: f32) -> Option<u32> {
        if let Some(spans) = self.bounded.get(&(y * self.nx + x)) {
            if let Some(span) = spans.iter().find(|s| s.lower <= z && s.upper >= z) {
                return span.lower_op.or(span.upper_op);
            }
        }
        let mut op = self.operation_at(x, y);
        let mut previous = self.previous_layers[y * self.nx + x];
        while let Some(layer) = self.layers.get(previous as usize) {
            if z <= layer.height {
                break;
            }
            op = layer.operation;
            previous = layer.previous;
        }
        op
    }

    fn air_cut(&self, x: usize, y: usize, z: f32) -> u32 {
        if let Some(spans) = self.bounded.get(&(y * self.nx + x)) {
            if let Some(span) = spans.iter().find(|s| s.lower <= z && s.upper >= z) {
                return if span.lower_source != u32::MAX {
                    span.lower_source
                } else {
                    span.upper_source
                };
            }
        }
        let mut cut = self.winners[y * self.nx + x];
        let mut previous = self.previous_layers[y * self.nx + x];
        while let Some(layer) = self.layers.get(previous as usize) {
            if z <= layer.height {
                break;
            }
            cut = layer.cut;
            previous = layer.previous;
        }
        cut
    }

    /// Average matching tooth boundaries at a shared grid corner. Separate Z
    /// intervals keep opposite flanks and adjacent turns from being joined.
    fn boundary_corner(&self, x: usize, y: usize, boundary: Boundary) -> f32 {
        if let (Some(cylinder), Some((radius, positive))) =
            (self.horizontal_cylinder, boundary.cylinder)
        {
            let transverse = 1 - cylinder.axis;
            let index = [x, y][transverse];
            let coordinate =
                (self.origin[transverse] + index as f64 * self.res).min(self.max[transverse]);
            let offset = coordinate - cylinder.center[transverse];
            let h = (radius * radius - offset * offset).max(0.).sqrt();
            return (cylinder.center[2] + if positive { h } else { -h }) as f32;
        }
        // A full rotary pass creates the same radius in neighbouring columns.
        // Interpolate that nearly constant radius at the shared corner instead
        // of averaging Z heights, which creates radial stair-steps near the
        // cylinder silhouette. Distinct operations and distinct radial layers
        // remain separate; the underlying material intervals are unchanged.
        if let (Some(cylinder), Some((radius, positive))) =
            (self.horizontal_cylinder, boundary.radial)
        {
            let transverse = 1 - cylinder.axis;
            let mut sum = 0.;
            let mut count = 0;
            let mut first_row = None;
            let mut multiple_rows = false;
            for dy in [-1isize, 0] {
                for dx in [-1isize, 0] {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if nx < 0 || ny < 0 || nx >= self.nx as isize || ny >= self.ny as isize {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    let (cx, cy) = self.center(nx, ny);
                    let offset = [cx, cy][transverse] - cylinder.center[transverse];
                    if let Some(spans) = self.bounded.get(&(ny * self.nx + nx)) {
                        let nearest = spans
                            .iter()
                            .filter_map(|span| {
                                let (z, op) = if boundary.lower_void {
                                    (span.lower, span.lower_op)
                                } else {
                                    (span.upper, span.upper_op)
                                };
                                let z = f64::from(z) - cylinder.center[2];
                                let candidate = offset.hypot(z);
                                (op == boundary.operation
                                    && (z >= 0.) == positive
                                    && (candidate - radius).abs()
                                        <= (self.res * 0.02).clamp(1e-5, 0.001))
                                .then_some(candidate)
                            })
                            .min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs()));
                        if let Some(candidate) = nearest {
                            let row = [nx, ny][transverse];
                            multiple_rows |= first_row.is_some_and(|first| first != row);
                            first_row.get_or_insert(row);
                            sum += candidate;
                            count += 1;
                        }
                    }
                }
            }
            if count >= 2 && multiple_rows {
                let radius = sum / count as f64;
                let index = [x, y][transverse];
                let coordinate =
                    (self.origin[transverse] + index as f64 * self.res).min(self.max[transverse]);
                let offset = coordinate - cylinder.center[transverse];
                let h = (radius * radius - offset * offset).max(0.).sqrt();
                return (cylinder.center[2] + if positive { h } else { -h }) as f32;
            }
        }
        if !boundary.thread {
            return boundary.z;
        }
        let mut sum = 0.;
        let mut count = 0.;
        for dy in [-1isize, 0] {
            for dx in [-1isize, 0] {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if nx < 0 || ny < 0 || nx >= self.nx as isize || ny >= self.ny as isize {
                    continue;
                }
                if let Some(spans) = self.bounded.get(&(ny as usize * self.nx + nx as usize)) {
                    let nearest = spans
                        .iter()
                        .filter_map(|s| {
                            let (z, op) = if boundary.lower_void {
                                (s.lower, s.lower_op)
                            } else {
                                (s.upper, s.upper_op)
                            };
                            (op == boundary.operation
                                && (z - boundary.z).abs() < self.res as f32 * 1.5)
                                .then_some(z)
                        })
                        .min_by(|a, b| (a - boundary.z).abs().total_cmp(&(b - boundary.z).abs()));
                    if let Some(z) = nearest {
                        sum += z;
                        count += 1.;
                    }
                }
            }
        }
        if count > 0. {
            sum / count
        } else {
            boundary.z
        }
    }

    fn edge_material(
        &self,
        x: usize,
        y: usize,
        floor: f32,
        corners: [(usize, usize); 2],
    ) -> Vec<[[f32; 2]; 2]> {
        self.material_spans(x, y, floor)
            .into_iter()
            .map(|span| {
                span.boundaries().map(|bound| {
                    corners.map(|(x, y)| self.boundary_corner(x, y, bound).clamp(floor, self.top))
                })
            })
            .collect()
    }

    pub(super) fn mesh_column(
        &self,
        x: usize,
        y: usize,
        floor: f32,
        xy: [[f32; 2]; 4],
        mesh: &mut SurfaceMesh,
    ) {
        let material = self.material_spans(x, y, floor);
        if material.is_empty() {
            return;
        }
        let coords = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
        for span in &material {
            for (side, bound) in span.boundaries().into_iter().enumerate() {
                let mut p = std::array::from_fn(|i| {
                    [
                        xy[i][0],
                        xy[i][1],
                        self.boundary_corner(coords[i].0, coords[i].1, bound)
                            .clamp(floor, self.top),
                    ]
                });
                let cut = if side == 0 {
                    span.lower_cut
                } else {
                    span.upper_cut
                };
                let u: [f32; 3] = std::array::from_fn(|i| p[1][i] - p[0][i] + p[2][i] - p[3][i]);
                let v: [f32; 3] = std::array::from_fn(|i| p[3][i] - p[0][i] + p[2][i] - p[1][i]);
                let sign = if side == 0 { -1. } else { 1. };
                let normal = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
                let normal = if length > 1e-12 {
                    normal.map(|v| v * sign / length)
                } else {
                    [0., 0., sign]
                };
                if side == 0 {
                    p = [p[0], p[3], p[2], p[1]];
                }
                let first = mesh.positions.len();
                mesh.quad(p, [normal; 4], cut, bound.operation);
                mesh.stamp_source(first, self, bound.cut);
            }
        }
        for (dx, dy, ids) in [
            (-1, 0, [0, 3]),
            (1, 0, [2, 1]),
            (0, -1, [1, 0]),
            (0, 1, [3, 2]),
        ] {
            let edge = ids.map(|i| xy[i]);
            let delta = [edge[1][0] - edge[0][0], edge[1][1] - edge[0][1]];
            let length = delta[0].hypot(delta[1]);
            if length < 1e-12 {
                continue;
            }
            let normal = [-delta[1] / length, delta[0] / length, 0.];
            let corners = ids.map(|i| coords[i]);
            let (nx, ny) = (x as isize + dx, y as isize + dy);
            let neighbour = (nx >= 0 && ny >= 0 && nx < self.nx as isize && ny < self.ny as isize)
                .then_some((nx as usize, ny as usize));
            let adjacent = neighbour.map_or_else(Vec::new, |(nx, ny)| {
                self.edge_material(nx, ny, floor, corners)
            });
            let own = self.edge_material(x, y, floor, corners);
            let mid = |z: [f32; 2]| (z[0] + z[1]) * 0.5;
            let contains =
                |spans: &[[[f32; 2]; 2]], z| spans.iter().any(|s| mid(s[0]) < z && mid(s[1]) > z);
            let mut levels: Vec<_> = own.iter().chain(&adjacent).flat_map(|s| *s).collect();
            if let Some((nx, ny)) = neighbour {
                let mut previous = self.previous_layers[ny * self.nx + nx];
                while let Some(layer) = self.layers.get(previous as usize) {
                    levels.push([layer.height; 2]);
                    previous = layer.previous;
                }
            }
            levels.sort_by(|a, b| mid(*a).total_cmp(&mid(*b)));
            for pair in levels.windows(2) {
                let z = (mid(pair[0]) + mid(pair[1])) * 0.5;
                if mid(pair[1]) - mid(pair[0]) <= 1e-6
                    || !contains(&own, z)
                    || contains(&adjacent, z)
                {
                    continue;
                }
                let lower = pair[0];
                let upper: [f32; 2] = std::array::from_fn(|i| pair[1][i].max(lower[i]));
                let op = neighbour.and_then(|(nx, ny)| self.air_operation(nx, ny, z));
                let cut = neighbour.map_or(u32::MAX, |(nx, ny)| self.air_cut(nx, ny, z));
                let first = mesh.positions.len();
                mesh.quad(
                    [
                        [edge[0][0], edge[0][1], upper[0]],
                        [edge[1][0], edge[1][1], upper[1]],
                        [edge[1][0], edge[1][1], lower[1]],
                        [edge[0][0], edge[0][1], lower[0]],
                    ],
                    [normal; 4],
                    op.is_some()
                        || cut != u32::MAX
                        || neighbour.is_some_and(|(nx, ny)| self.top_is_cut(ny * self.nx + nx)),
                    op,
                );
                mesh.stamp_source(first, self, cut);
            }
        }
    }
}
