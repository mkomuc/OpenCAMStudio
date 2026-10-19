//! A heightfield stock model for 2.5-D material-removal simulation.
//!
//! The stock is a regular XY grid; each cell stores the Z of the top of the
//! remaining material. A flat end mill of radius `r` moving along a segment
//! lowers every cell within `r` of the swept path to the tool's bottom. It is
//! the simplest model that captures 2.5-D removal faithfully — enough to verify
//! that a program clears what it should and never plows a rapid through stock.

use crate::{sweep::Sweep, ToolProfile};

#[path = "surface.rs"]
mod surface;

/// How the simulated stock compares to a desired target surface — the raw
/// material of gouge / residual verification.
///
/// Signs follow milling intuition: a **gouge** is stock cut *below* the target
/// (material destroyed that should have remained — the dangerous error); a
/// **residual** is stock left *above* the target (uncut material that should have
/// been removed — a quality miss, not a hazard).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceDiff {
    /// Deepest over-cut below the target, mm (0 if none exceeds tolerance).
    pub max_gouge: f32,
    /// XY of the deepest gouge, if any.
    pub gouge_at: Option<[f64; 2]>,
    /// Simulated Z at the deepest gouge.
    pub gouge_z: f64,
    /// Total volume cut below the target, mm³.
    pub gouge_volume: f64,
    /// Total volume of stock left above the target, mm³.
    pub residual_volume: f64,
    /// Cells whose over-cut exceeds tolerance.
    pub cells_gouged: usize,
    /// Cells whose leftover stock exceeds tolerance.
    pub cells_residual: usize,
}

/// An indexed triangle mesh of the stock surface, for rendering. Positions are
/// millimetres `(x, y, z)`; normals are unit and point out of the remaining stock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex material removal flag. Original stock faces remain false;
    /// newly exposed tops, slopes and internal cut walls are true.
    pub machined: Vec<bool>,
    /// Operation that exposed each surface vertex; original faces have no owner.
    pub operations: Vec<Option<u32>>,
    pub indices: Vec<u32>,
}

/// Unit normal of triangle `a→b→c` from its winding (CCW ⇒ toward the viewer).
#[cfg(test)]
fn tri_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
    [n[0] / len, n[1] / len, n[2] / len]
}

/// A former surface, retained only when a different operation cuts it deeper.
/// The linked layers let vertical walls keep the ownership of their upper bands.
#[derive(Clone, Copy, Debug)]
struct CutLayer {
    height: f32,
    operation: Option<u32>,
    previous: u32,
}

/// A grid of remaining-stock heights.
#[derive(Clone, Debug)]
pub struct Heightfield {
    origin: [f64; 2],
    max: [f64; 2],
    res: f64,
    nx: usize,
    ny: usize,
    top: f32,
    z: Vec<f32>,
    // Winning cutter at each sample supplies contour geometry and operation ownership.
    winners: Vec<u32>,
    cuts: Vec<Sweep>,
    previous_layers: Vec<u32>,
    layers: Vec<CutLayer>,
}

impl Heightfield {
    /// A fresh block of stock over `[min, max]` (XY), with `res`-sized cells, all
    /// at height `top`.
    pub fn new(min: [f64; 2], max: [f64; 2], res: f64, top: f64) -> Self {
        let nx = (((max[0] - min[0]) / res).ceil() as usize).max(1);
        let ny = (((max[1] - min[1]) / res).ceil() as usize).max(1);
        Self {
            origin: min,
            max,
            res,
            nx,
            ny,
            top: top as f32,
            z: vec![top as f32; nx * ny],
            winners: vec![u32::MAX; nx * ny],
            cuts: Vec::new(),
            previous_layers: vec![u32::MAX; nx * ny],
            layers: Vec::new(),
        }
    }

    pub(crate) fn top(&self) -> f32 {
        self.top
    }

    pub(crate) fn storage_bytes(&self) -> usize {
        self.z.len() * 12
            + self.cuts.len() * std::mem::size_of::<Sweep>()
            + self.layers.len() * std::mem::size_of::<CutLayer>()
    }

    /// Grid dimensions (columns, rows).
    pub fn dims(&self) -> (usize, usize) {
        (self.nx, self.ny)
    }

    /// Cell size, mm.
    pub fn resolution(&self) -> f64 {
        self.res
    }

    /// The remaining-stock height at `(x, y)` (nearest cell), or the original top
    /// if outside the grid.
    pub fn sample(&self, x: f64, y: f64) -> f32 {
        match self.cell(x, y) {
            Some((ix, iy)) => self.z[iy * self.nx + ix],
            None => self.top,
        }
    }

    /// Lower every cell within `radius` of the segment `a→b` to the (flat) tool
    /// bottom, interpolating the bottom Z along the segment. A convenience for a
    /// flat end mill; [`cut_segment_profile`](Self::cut_segment_profile) handles
    /// shaped tools.
    pub fn cut_segment(&mut self, a: [f64; 3], b: [f64; 3], radius: f64) {
        self.cut_segment_profile(a, b, &ToolProfile::flat(radius));
    }

    /// Lower every cell within the tool's radius of the segment `a→b` to the tool
    /// bottom, accounting for the tool's [`ToolProfile`]: the axis bottom is
    /// interpolated along the segment, and each cell is raised by the profile's
    /// `offset` at its radial distance from the axis (so a ball mill leaves a
    /// rounded floor, a V mill a groove, and a flat mill a flat floor).
    pub fn cut_segment_profile(&mut self, a: [f64; 3], b: [f64; 3], tool: &ToolProfile) {
        self.cut_segment_profile_controlled(a, b, tool, None, &|| false);
    }

    pub(crate) fn cut_segment_profile_controlled(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        tool: &ToolProfile,
        operation: Option<u32>,
        cancelled: &dyn Fn() -> bool,
    ) {
        let radius = tool.radius;
        if a[2].min(b[2]) >= f64::from(self.top)
            || a[0].max(b[0]) + radius < self.origin[0]
            || a[0].min(b[0]) - radius > self.max[0]
            || a[1].max(b[1]) + radius < self.origin[1]
            || a[1].min(b[1]) - radius > self.max[1]
        {
            return;
        }
        let mut sweep = Sweep::new(a, b, *tool);
        sweep.operation = operation;
        let id = self.cuts.len() as u32;
        self.cuts.push(sweep);
        let (iy0, iy1) = self.index_range(a[1].min(b[1]) - radius, a[1].max(b[1]) + radius, 1);
        let r2 = radius * radius;
        for iy in iy0..=iy1 {
            if cancelled() {
                return;
            }
            let cy = self.center(0, iy).1;
            let Some((lo, hi)) = sweep.row_span(cy) else {
                continue;
            };
            let (ix0, ix1) = self.index_range(lo, hi, 0);
            for ix in ix0..=ix1 {
                let cx = self.center(ix, iy).0;
                let (t, dist2) = sweep.projection(cx, cy);
                if dist2 <= r2 {
                    let bottom = sweep.bottom(t, dist2) as f32;
                    let index = iy * self.nx + ix;
                    if bottom < self.z[index] {
                        let previous_op = self.operation_at(ix, iy);
                        if self.z[index] < self.top && previous_op != operation {
                            let layer = self.layers.len() as u32;
                            self.layers.push(CutLayer {
                                height: self.z[index],
                                operation: previous_op,
                                previous: self.previous_layers[index],
                            });
                            self.previous_layers[index] = layer;
                        }
                        self.z[index] = bottom;
                        self.winners[index] = id;
                    }
                }
            }
        }
    }

    fn operation_at(&self, x: usize, y: usize) -> Option<u32> {
        let winner = self.winners[y * self.nx + x];
        self.cuts.get(winner as usize).and_then(|cut| cut.operation)
    }

    /// The greatest remaining-stock height within `radius` of the swept XY path
    /// `a→b` — used to detect a rapid plowing through stock. Returns `f32::MIN`
    /// if the path covers no cells.
    pub fn max_height_along(&self, a: [f64; 2], b: [f64; 2], radius: f64) -> f32 {
        let (ix0, ix1) = self.index_range(a[0].min(b[0]) - radius, a[0].max(b[0]) + radius, 0);
        let (iy0, iy1) = self.index_range(a[1].min(b[1]) - radius, a[1].max(b[1]) + radius, 1);
        let r2 = radius * radius;
        let mut max = f32::MIN;
        for iy in iy0..=iy1 {
            for ix in ix0..=ix1 {
                let (cx, cy) = self.center(ix, iy);
                if project(cx, cy, a, b).1 <= r2 {
                    max = max.max(self.z[iy * self.nx + ix]);
                }
            }
        }
        max
    }

    /// How far remaining stock rises **into the tool** along the swept XY path `a→b`
    /// with the tool tip at `tip_z` — the profile-aware companion to
    /// [`max_height_along`](Self::max_height_along), and the mirror of
    /// [`cut_segment_profile`](Self::cut_segment_profile)'s removal rule.
    ///
    /// A cell at radial distance `d` from the path is cleared up to
    /// `tip_z + offset(d)`; anything above that is material the tool body would hit.
    /// Returns `(intrusion, stock_height)` for the worst cell — a positive
    /// `intrusion` means a collision — or `None` if the path covers no cells.
    ///
    /// Why this matters over a plain radius test: a **pointed** tool is only as wide
    /// as its cone has opened at the depth it sits. A ⌀6 V-bit 0.15 mm below the
    /// surface is a 0.3 mm sliver, not a 6 mm cylinder, so testing its full radius
    /// against the *uncut surface alongside* the groove reports a collision for a
    /// move that physically fits in the groove the tool just cut.
    ///
    /// **Resolution-limited.** The test samples cell centres, so a feature narrower
    /// than a cell is invisible to it: at 0.5 mm cells the nearest sample to the path
    /// is 0.25 mm off-axis, where a 60° cone has already climbed 0.33 mm — enough to
    /// miss a tip sitting 0.3 mm into solid stock. The caller is responsible for
    /// sizing the grid to the narrowest cut (`cam-app`'s `narrowest_cut_width` does
    /// this), and with a grid so sized the test is accurate in both directions.
    ///
    /// `tip_z` is the **lowest** tip height over the move, not an interpolation:
    /// `G0` is not guaranteed to travel a straight line (axes may run at
    /// independent rates, so Z can arrive before XY), and assuming the worst is the
    /// safe reading for a rapid.
    pub fn max_intrusion_along(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        tip_z: f64,
        tool: &ToolProfile,
    ) -> Option<(f32, f32)> {
        self.max_intrusion_along_controlled(a, b, tip_z, tool, &|| false)
    }

    pub(crate) fn max_intrusion_along_controlled(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        tip_z: f64,
        tool: &ToolProfile,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<(f32, f32)> {
        let radius = tool.radius;
        let sweep = Sweep::new([a[0], a[1], tip_z], [b[0], b[1], tip_z], *tool);
        let (iy0, iy1) = self.index_range(a[1].min(b[1]) - radius, a[1].max(b[1]) + radius, 1);
        let r2 = radius * radius;
        let mut worst: Option<(f32, f32)> = None;
        for iy in iy0..=iy1 {
            if cancelled() {
                return None;
            }
            let cy = self.center(0, iy).1;
            let Some((lo, hi)) = sweep.row_span(cy) else {
                continue;
            };
            let (ix0, ix1) = self.index_range(lo, hi, 0);
            for ix in ix0..=ix1 {
                let cx = self.center(ix, iy).0;
                let dist2 = sweep.projection(cx, cy).1;
                if dist2 > r2 {
                    continue;
                }
                let height = self.z[iy * self.nx + ix];
                let tool_bottom = (tip_z + tool.offset(dist2.sqrt())) as f32;
                let intrusion = height - tool_bottom;
                if worst.is_none_or(|(w, _)| intrusion > w) {
                    worst = Some((intrusion, height));
                }
            }
        }
        worst
    }

    /// Volume of material removed so far, mm³.
    pub fn removed_volume(&self) -> f64 {
        let cell = self.res * self.res;
        self.z
            .iter()
            .map(|&z| (self.top - z).max(0.0) as f64 * cell)
            .sum()
    }

    /// Lower every cell whose centre lies in the XY rectangle `[min, max]` to
    /// `z`, never raising — a primitive for building a target surface (e.g. a
    /// pocket floor) or pre-shaping stock.
    pub fn lower_rect(&mut self, min: [f64; 2], max: [f64; 2], z: f64) {
        let (ix0, ix1) = self.index_range(min[0], max[0], 0);
        let (iy0, iy1) = self.index_range(min[1], max[1], 1);
        let z = z as f32;
        for iy in iy0..=iy1 {
            for ix in ix0..=ix1 {
                let (cx, cy) = self.center(ix, iy);
                if cx >= min[0] && cx <= max[0] && cy >= min[1] && cy <= max[1] {
                    let cell = &mut self.z[iy * self.nx + ix];
                    if z < *cell {
                        *cell = z;
                        self.winners[iy * self.nx + ix] = u32::MAX;
                    }
                }
            }
        }
    }

    /// Compare this (simulated) field against a `target` surface, cell by cell.
    /// `target` is sampled at each of this field's cell centres, so the two grids
    /// need not align. `tol` (mm) is the deviation ignored as grazing.
    pub fn compare(&self, target: &Heightfield, tol: f64) -> SurfaceDiff {
        let tol = tol as f32;
        let cell_area = self.res * self.res;
        let mut diff = SurfaceDiff::default();
        let mut worst = 0.0f32;
        for iy in 0..self.ny {
            for ix in 0..self.nx {
                let actual = self.z[iy * self.nx + ix];
                let (cx, cy) = self.center(ix, iy);
                let over = target.sample(cx, cy) - actual; // >0 ⇒ cut below target
                if over > tol {
                    diff.gouge_volume += over as f64 * cell_area;
                    diff.cells_gouged += 1;
                    if over > worst {
                        worst = over;
                        diff.gouge_at = Some([cx, cy]);
                        diff.gouge_z = actual as f64;
                    }
                } else if -over > tol {
                    diff.residual_volume += (-over) as f64 * cell_area;
                    diff.cells_residual += 1;
                }
            }
        }
        diff.max_gouge = worst;
        diff
    }

    /// Triangulate the current surface into a [`SurfaceMesh`] for rendering.
    /// Per-vertex normals come from central differences of the height grid.
    pub fn to_mesh(&self) -> SurfaceMesh {
        let mut positions = Vec::with_capacity(self.nx * self.ny);
        for iy in 0..self.ny {
            for ix in 0..self.nx {
                let (cx, cy) = self.center(ix, iy);
                positions.push([cx as f32, cy as f32, self.z[iy * self.nx + ix]]);
            }
        }

        let res = self.res as f32;
        let mut normals = Vec::with_capacity(self.nx * self.ny);
        for iy in 0..self.ny {
            for ix in 0..self.nx {
                let xm = ix.saturating_sub(1);
                let xp = (ix + 1).min(self.nx - 1);
                let ym = iy.saturating_sub(1);
                let yp = (iy + 1).min(self.ny - 1);
                let dzdx = (self.z[iy * self.nx + xp] - self.z[iy * self.nx + xm])
                    / ((xp - xm).max(1) as f32 * res);
                let dzdy = (self.z[yp * self.nx + ix] - self.z[ym * self.nx + ix])
                    / ((yp - ym).max(1) as f32 * res);
                let n = [-dzdx, -dzdy, 1.0];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                normals.push([n[0] / len, n[1] / len, n[2] / len]);
            }
        }

        let mut indices = Vec::new();
        for iy in 0..self.ny.saturating_sub(1) {
            for ix in 0..self.nx.saturating_sub(1) {
                let i = (iy * self.nx + ix) as u32;
                let right = i + 1;
                let down = i + self.nx as u32;
                let down_right = down + 1;
                // Two CCW triangles as seen from +Z (front-facing looking down).
                indices.extend_from_slice(&[i, right, down_right]);
                indices.extend_from_slice(&[i, down_right, down]);
            }
        }

        SurfaceMesh {
            positions,
            normals,
            machined: self.z.iter().map(|&z| z < self.top).collect(),
            operations: (0..self.ny)
                .flat_map(|y| (0..self.nx).map(move |x| self.operation_at(x, y)))
                .collect(),
            indices,
        }
    }

    /// Centre of cell `(ix, iy)`.
    fn center(&self, ix: usize, iy: usize) -> (f64, f64) {
        (
            (self.origin[0]
                + ix as f64 * self.res
                + (self.origin[0] + (ix + 1) as f64 * self.res).min(self.max[0]))
                * 0.5,
            (self.origin[1]
                + iy as f64 * self.res
                + (self.origin[1] + (iy + 1) as f64 * self.res).min(self.max[1]))
                * 0.5,
        )
    }

    /// Cell index containing `(x, y)`, if inside the grid.
    fn cell(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let ix = ((x - self.origin[0]) / self.res).floor();
        let iy = ((y - self.origin[1]) / self.res).floor();
        if x >= self.max[0]
            || y >= self.max[1]
            || ix < 0.0
            || iy < 0.0
            || ix >= self.nx as f64
            || iy >= self.ny as f64
        {
            None
        } else {
            Some((ix as usize, iy as usize))
        }
    }

    /// Clamped inclusive cell-index range covering `[lo, hi]` on axis `axis`.
    fn index_range(&self, lo: f64, hi: f64, axis: usize) -> (usize, usize) {
        let n = if axis == 0 { self.nx } else { self.ny };
        let i0 =
            (((lo - self.origin[axis]) / self.res).floor()).clamp(0.0, (n - 1) as f64) as usize;
        let i1 =
            (((hi - self.origin[axis]) / self.res).floor()).clamp(0.0, (n - 1) as f64) as usize;
        (i0, i1)
    }
}

/// Project `(px, py)` onto segment `a→b`, returning the clamped parameter `t`
/// and the squared distance to the segment.
fn project(px: f64, py: f64, a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 1e-12 {
        0.0
    } else {
        (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (a[0] + dx * t, a[1] + dy * t);
    let d2 = (px - qx) * (px - qx) + (py - qy) * (py - qy);
    (t, d2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_plunges_remove_material_to_the_lowest_endpoint() {
        let mut field = Heightfield::new([0., 0.], [10., 10.], 0.25, 0.);
        field.cut_segment([5., 5., 2.], [5., 5., -3.], 1.);
        assert_eq!(field.sample(5., 5.), -3.);
        field.cut_segment([5., 5., -3.], [5., 5., 2.], 1.);
        assert_eq!(field.sample(5., 5.), -3.);
        assert_eq!(field.sample(1., 1.), 0.);
    }
    #[test]
    fn a_straight_cut_lowers_a_band_to_depth() {
        let mut hf = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        // Cut along y=10 from x=2 to x=18 at Z=-3 with a 2 mm-radius tool.
        hf.cut_segment([2.0, 10.0, -3.0], [18.0, 10.0, -3.0], 2.0);
        assert!(
            (hf.sample(10.0, 10.0) + 3.0).abs() < 1e-6,
            "on the cut ⇒ -3"
        );
        assert!(
            (hf.sample(10.0, 10.0) - hf.sample(10.0, 11.5)).abs() < 1e-6,
            "within radius ⇒ cut"
        );
        assert!(
            (hf.sample(10.0, 15.0) - 0.0).abs() < 1e-6,
            "outside radius ⇒ untouched"
        );
    }

    #[test]
    fn max_height_sees_uncut_stock() {
        let mut hf = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        hf.cut_segment([0.0, 5.0, -2.0], [20.0, 5.0, -2.0], 2.0);
        // A path over the cut band is clear; a path over uncut stock is not.
        assert!(hf.max_height_along([0.0, 5.0], [20.0, 5.0], 1.0) < -1.9);
        assert!(hf.max_height_along([0.0, 15.0], [20.0, 15.0], 1.0) > -0.001);
    }

    #[test]
    fn lower_rect_carves_only_inside_the_rectangle() {
        let mut hf = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        hf.lower_rect([5.0, 5.0], [15.0, 15.0], -3.0);
        assert!((hf.sample(10.0, 10.0) + 3.0).abs() < 1e-6, "inside ⇒ -3");
        assert!(
            (hf.sample(2.0, 2.0) - 0.0).abs() < 1e-6,
            "outside ⇒ untouched"
        );
        // Never raises: a deeper existing cut survives a shallower lower_rect.
        hf.cut_segment([10.0, 10.0, -6.0], [10.0, 10.0, -6.0], 1.0);
        hf.lower_rect([5.0, 5.0], [15.0, 15.0], -3.0);
        assert!(hf.sample(10.0, 10.0) < -5.9, "deeper cut preserved");
    }

    #[test]
    fn compare_reports_gouge_and_residual() {
        // Simulated: a flat -4 floor. Target: floor at -2 inside a rectangle, top
        // (0) outside. Inside ⇒ gouge (cut 2 mm too deep); outside ⇒ residual
        // (2 mm of stock left standing above the target's -2... no — outside the
        // rect the target is the original top 0, and actual is -4, so it's still
        // a gouge). Use a target lowered everywhere except a raised pad.
        let mut actual = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        actual.lower_rect([0.0, 0.0], [20.0, 20.0], -4.0); // cut flat to -4

        let mut target = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        target.lower_rect([0.0, 0.0], [20.0, 20.0], -2.0); // wanted a -2 floor…
        target.lower_rect([5.0, 5.0], [15.0, 15.0], -6.0); // …with a deep pocket

        let diff = actual.compare(&target, 0.05);
        // Outside the pocket: actual -4 vs target -2 ⇒ 2 mm gouge.
        assert!(
            (diff.max_gouge - 2.0).abs() < 1e-3,
            "max gouge {}",
            diff.max_gouge
        );
        assert!(diff.gouge_volume > 0.0 && diff.cells_gouged > 0);
        // Inside the pocket: actual -4 vs target -6 ⇒ 2 mm of stock left = residual.
        assert!(diff.residual_volume > 0.0 && diff.cells_residual > 0);
    }

    #[test]
    fn to_mesh_has_a_vertex_per_cell_and_two_triangles_per_quad() {
        let hf = Heightfield::new([0.0, 0.0], [10.0, 10.0], 1.0, -1.0);
        let (nx, ny) = hf.dims();
        let mesh = hf.to_mesh();
        assert_eq!(mesh.positions.len(), nx * ny);
        assert_eq!(mesh.normals.len(), nx * ny);
        assert_eq!(mesh.indices.len(), (nx - 1) * (ny - 1) * 6);
        // A flat field ⇒ every normal points straight up.
        for n in &mesh.normals {
            assert!((n[2] - 1.0).abs() < 1e-6, "flat ⇒ +Z normal, got {n:?}");
        }
        // Indices stay in bounds.
        assert!(mesh
            .indices
            .iter()
            .all(|&i| (i as usize) < mesh.positions.len()));
    }

    #[test]
    fn solid_mesh_is_a_closed_block_with_walls_and_a_floor() {
        // Uncut 10×10 block, top 0, floor -5.
        let hf = Heightfield::new([0.0, 0.0], [10.0, 10.0], 1.0, 0.0);
        let mesh = hf.to_solid_mesh(-5.0);
        assert!(mesh
            .indices
            .iter()
            .all(|&i| (i as usize) < mesh.positions.len()));
        // Must carry top (+Z), bottom (−Z) and side-wall (horizontal) faces — a
        // sheet would have only +Z.
        let has = |pred: fn(&[f32; 3]) -> bool| mesh.normals.iter().any(pred);
        assert!(has(|n| n[2] > 0.9), "has a top");
        assert!(has(|n| n[2] < -0.9), "has a bottom");
        assert!(
            has(|n| n[0].abs() > 0.9 || n[1].abs() > 0.9),
            "has perimeter walls"
        );
        // No geometry ever dips below the floor.
        assert!(mesh.positions.iter().all(|p| p[2] >= -5.0 - 1e-4));
        assert_eq!(mesh.machined.len(), mesh.positions.len());
        assert!(mesh.machined.iter().all(|&cut| !cut));
    }

    #[test]
    fn removal_marks_cut_floors_slopes_and_walls_but_not_original_faces() {
        for depth in [-0.5, -3.0, -6.0] {
            let mut hf = Heightfield::new([0., 0.], [10., 10.], 0.5, 0.);
            hf.lower_rect([3., 3.], [7., 7.], depth);
            let mesh = hf.to_solid_mesh(-5.);
            assert_eq!(mesh.machined.len(), mesh.positions.len());
            let mut cut_tops = 0;
            let mut cut_walls = 0;
            for indices in mesh.indices.as_chunks::<3>().0 {
                let triangle = [0, 1, 2].map(|i| mesh.positions[indices[i] as usize]);
                let flags = [0, 1, 2].map(|i| mesh.machined[indices[i] as usize]);
                assert!(flags.iter().all(|&cut| cut == flags[0]));
                let n = tri_normal(triangle[0], triangle[1], triangle[2]);
                let original = n[2] < -0.9
                    || (0..2).any(|axis| {
                        triangle.iter().all(|p| p[axis] == 0.)
                            || triangle.iter().all(|p| p[axis] == 10.)
                    })
                    || triangle.iter().all(|p| p[2] == 0.);
                assert_eq!(flags[0], !original, "depth {depth}: {triangle:?}");
                if flags[0] {
                    if n[2] > 0.1 {
                        cut_tops += 1;
                    } else {
                        cut_walls += 1;
                    }
                }
            }
            assert_eq!(cut_tops > 0, depth > -5., "through cuts have no floor");
            assert_eq!(
                cut_walls > 0,
                depth < -1.25,
                "steep and through cuts expose walls"
            );
        }
    }

    #[test]
    fn uncut_faces_stay_unmachined_at_nonzero_stock_heights() {
        // Averaging equal corner heights can round below the original top.
        // Classification must follow material removal, not the rounded mesh Z.
        for top in [0.1, 100.1, -100.1] {
            let hf = Heightfield::new([0., 0.], [3., 3.], 1., top);
            let mesh = hf.to_solid_mesh(top - 5.);
            assert!(mesh.machined.iter().all(|&cut| !cut));
        }
    }

    #[test]
    fn gentle_slopes_stay_smooth_while_deep_steps_get_walls() {
        // The chamfer-staircase fix: a shallow step (gentle vs the cell size) ramps
        // smoothly, adding no vertical wall; a deep step keeps a crisp wall.
        let floor = -5.0;
        let wall_faces = |hf: &Heightfield| {
            hf.to_solid_mesh(floor)
                .normals
                .iter()
                .filter(|n| n[2].abs() < 0.1) // near-horizontal normal ⇒ vertical face
                .count()
        };
        let mut shallow = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        shallow.lower_rect([5.0, 5.0], [15.0, 15.0], -0.5);
        let mut deep = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        deep.lower_rect([5.0, 5.0], [15.0, 15.0], -3.0);
        assert!(
            wall_faces(&deep) > wall_faces(&shallow),
            "deep {} vs shallow {}: a deep pocket walls its edge; a shallow one ramps",
            wall_faces(&deep),
            wall_faces(&shallow)
        );
    }

    #[test]
    fn curved_walls_shade_radially_not_just_axis_aligned() {
        // A round hole's walls take gradient (radial) normals, so some wall face
        // points diagonally — impossible for pure ±X/±Y voxel walls.
        let mut hf = Heightfield::new([0.0, 0.0], [20.0, 20.0], 0.5, 0.0);
        // A cylindrical hole (radius 4) cut through the floor at the centre.
        hf.cut_segment([9.99, 10.0, -6.0], [10.01, 10.0, -6.0], 4.0);
        let mesh = hf.to_solid_mesh(-5.0);
        let diagonal = mesh
            .normals
            .iter()
            .any(|n| n[2].abs() < 0.2 && n[0].abs() > 0.3 && n[1].abs() > 0.3);
        assert!(
            diagonal,
            "round-hole walls should shade radially (a diagonal normal), not only ±X/±Y"
        );
    }

    #[test]
    fn solid_mesh_leaves_through_cuts_open() {
        // A cell cut clean through (below the floor) contributes no top face, so a
        // through feature reads as see-through rather than a filled recess.
        let floor = -5.0;
        let tops = |hf: &Heightfield| {
            let mesh = hf.to_solid_mesh(floor);
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let [a, b, c] = [0, 1, 2].map(|i| mesh.positions[t[i] as usize]);
                    // Projected top area, independent of mesh compaction.
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).max(0.) * 0.5
                })
                .sum::<f32>()
        };
        let mut hf = Heightfield::new([0.0, 0.0], [3.0, 3.0], 1.0, 0.0);
        let before = tops(&hf);
        // Cut the centre cell past the floor.
        hf.cut_segment([1.3, 1.5, -6.0], [1.7, 1.5, -6.0], 0.3);
        assert!(hf.sample(1.5, 1.5) <= -5.9, "centre cell cut through");
        assert!(
            tops(&hf) < before,
            "the through-cut cell has no top ⇒ open hole"
        );
    }
}
