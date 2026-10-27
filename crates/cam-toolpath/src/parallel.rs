//! Three-axis ball-end parallel finishing over the independent CAD projection.
use crate::{CancelToken, Diagnostic, JobEnv, Strategy, StrategyResult};
use cam_cldata::{MoveKind, Point3, Step, Tag};
use cam_geometry3d::ParallelGeometry;
use cam_model::{AdaptiveContainment, ParallelOp, StockToLeave, ToolKind};
use std::collections::HashMap;
type V = [f64; 3];
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn inside(p: V, t: &[V; 3]) -> bool {
    let mut signs = [0.; 3];
    for i in 0..3 {
        let a = t[i];
        let b = t[(i + 1) % 3];
        signs[i] = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    }
    signs.iter().all(|v| *v >= -1e-10) || signs.iter().all(|v| *v <= 1e-10)
}
/// Maximum triangle height minus the spherical cutter profile. Includes face,
/// edge and vertex contact (vertical triangles included).
fn drop_ball(p: V, t: &[V; 3], r: f64) -> Option<f64> {
    let mut best = f64::NEG_INFINITY;
    let mut candidate = |q: V| {
        let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
        if d <= r * r + 1e-10 {
            best = best.max(q[2] + (r * r - d).max(0.).sqrt() - r);
        }
    };
    for i in 0..3 {
        let a = t[i];
        let d = sub(t[(i + 1) % 3], a);
        let l = d[0].hypot(d[1]);
        if l < 1e-12 {
            candidate(a);
            candidate(t[(i + 1) % 3]);
            continue;
        }
        let along = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l;
        let perp = ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]) / l;
        if perp.abs() > r {
            continue;
        }
        let reach = (r * r - perp * perp).max(0.).sqrt();
        let slope = d[2] / l;
        let q = (along + reach * slope / (1. + slope * slope).sqrt()).clamp(0., l);
        candidate(add(a, mul(d, q / l)));
    }
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    if n[2].abs() > 1e-12 {
        let gx = -n[0] / n[2];
        let gy = -n[1] / n[2];
        let den = (1. + gx * gx + gy * gy).sqrt();
        let mut q = [p[0] + r * gx / den, p[1] + r * gy / den, 0.];
        q[2] = t[0][2] + gx * (q[0] - t[0][0]) + gy * (q[1] - t[0][1]);
        if inside(q, t) {
            candidate(q);
        }
    }
    best.is_finite().then_some(best)
}
struct Grid<'a> {
    mesh: &'a [[V; 3]],
    cells: HashMap<(i32, i32), Vec<usize>>,
    bounds: Vec<([f64; 2], [f64; 2])>,
    size: f64,
}
impl<'a> Grid<'a> {
    fn new(mesh: &'a [[V; 3]], size: f64, cancel: &CancelToken) -> Result<Self, &'static str> {
        let mut out = Self {
            mesh,
            cells: HashMap::new(),
            bounds: Vec::with_capacity(mesh.len()),
            size,
        };
        let mut entries = 0u64;
        for (i, t) in mesh.iter().enumerate() {
            if i % 1024 == 0 && cancel.is_cancelled() {
                return Err("Parallel cancelled");
            }
            let lo = [0, 1].map(|a| t.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min));
            let hi = [0, 1].map(|a| t.iter().map(|p| p[a]).fold(f64::NEG_INFINITY, f64::max));
            out.bounds.push((lo, hi));
            let width = (i64::from(out.key(hi[0])) - i64::from(out.key(lo[0])) + 1) as u64;
            let height = (i64::from(out.key(hi[1])) - i64::from(out.key(lo[1])) + 1) as u64;
            entries = entries.saturating_add(width.saturating_mul(height));
            if entries > 4_000_000 {
                return Err("Parallel CAD spatial index exceeds its memory budget; use a larger cutter or smaller model");
            }
            for x in out.key(lo[0])..=out.key(hi[0]) {
                for y in out.key(lo[1])..=out.key(hi[1]) {
                    out.cells.entry((x, y)).or_default().push(i);
                }
            }
        }
        Ok(out)
    }
    fn key(&self, x: f64) -> i32 {
        (x / self.size).floor() as i32
    }
    fn indices(&self, p: V, r: f64) -> Vec<usize> {
        let mut ids = Vec::new();
        for x in self.key(p[0] - r)..=self.key(p[0] + r) {
            for y in self.key(p[1] - r)..=self.key(p[1] + r) {
                if let Some(v) = self.cells.get(&(x, y)) {
                    ids.extend_from_slice(v);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|i| {
            let (lo, hi) = self.bounds[*i];
            let dx = (lo[0] - p[0]).max(p[0] - hi[0]).max(0.);
            let dy = (lo[1] - p[1]).max(p[1] - hi[1]).max(0.);
            dx * dx + dy * dy <= r * r + 1e-10
        });
        ids
    }
    fn height(&self, p: V, r: f64) -> Option<f64> {
        self.indices(p, r)
            .into_iter()
            .filter_map(|i| drop_ball(p, &self.mesh[i], r))
            .max_by(f64::total_cmp)
    }
    fn contact(&self, p: V) -> bool {
        self.indices(p, 0.).into_iter().any(|i| {
            let t = &self.mesh[i];
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            n[2].abs() > 1e-12 && inside(p, t)
        })
    }
}
#[derive(Clone, Debug)]
pub struct ParallelStrategy {
    op: ParallelOp,
    stock: StockToLeave,
    geometry: ParallelGeometry,
}
impl ParallelStrategy {
    pub const DOMAIN_REVISION: u32 = 1;
    pub fn new(op: ParallelOp, stock: StockToLeave, geometry: ParallelGeometry) -> Self {
        Self {
            op,
            stock,
            geometry,
        }
    }
}
impl Strategy for ParallelStrategy {
    fn name(&self) -> &str {
        "parallel"
    }
    fn compute(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        let mut out = StrategyResult::default();
        let op = &self.op;
        let p = op.params;
        macro_rules! fail {
            ($s:expr) => {{
                out.program = Default::default();
                out.diagnostics.push(Diagnostic::error($s));
                return out;
            }};
        }
        if cancel.is_cancelled() {
            out.cancelled = true;
            return out;
        }
        if [p.tolerance, p.stepover, p.feed, p.plunge_feed]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || !p.angle.is_finite()
            || !p.additional_offset.is_finite()
        {
            fail!("Parallel tolerance, stepover and feeds must be finite and positive; angle and offset must be finite");
        }
        let Some(tool) = env.tool(op.tool) else {
            fail!("Parallel tool is missing");
        };
        if tool.kind != ToolKind::BallMill {
            fail!("3D Parallel currently requires a ball nose end mill");
        }
        if !tool.diameter.is_finite() || tool.diameter <= 0. {
            fail!("Parallel tool diameter must be finite and positive");
        }
        if let Err(e) = op.entry_feeds.validate() {
            fail!(e);
        }
        let (radial, axial) = self.stock.effective();
        if [radial, axial].iter().any(|v| !v.is_finite() || *v < 0.) {
            fail!("Parallel stock allowances must be finite and nonnegative");
        }
        if self.geometry.selected.is_empty() || self.geometry.triangles.is_empty() {
            fail!("Parallel has no resolved CAD surface");
        }
        if self
            .geometry
            .triangles
            .iter()
            .flatten()
            .flatten()
            .any(|v| !v.is_finite())
        {
            fail!("Parallel CAD projection contains nonfinite coordinates");
        }
        if let Err(e) = self.geometry.heights.validate() {
            fail!(e);
        }
        if !self.geometry.error_bound.is_finite() || self.geometry.error_bound < 0.0 {
            fail!("Parallel CAD error bound must be finite and nonnegative");
        }
        if [
            tool.length,
            tool.flute_len(),
            tool.shank_dia(),
            tool.neck_dia(),
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
            || tool.flute_len() < tool.radius()
        {
            fail!("Parallel requires finite tool dimensions and flutes covering the ball nose");
        }
        let r = tool.radius() + radial;
        let error = self.geometry.error_bound;
        let contact_radius = r + error;
        let lift = error + p.tolerance + axial;
        if !r.is_finite() || !lift.is_finite() {
            fail!("Parallel cutter envelope is not finite");
        }
        let top = self
            .geometry
            .triangles
            .iter()
            .flatten()
            .map(|v| v[2])
            .fold(f64::NEG_INFINITY, f64::max)
            + lift;
        let clearance = env.heights.clearance;
        if !clearance.is_finite() || clearance <= top || clearance <= env.heights.top_of_stock {
            fail!("Parallel clearance must be above the complete model, allowance and stock");
        }
        // The spherical tip is checked against every setup body. A larger shaft
        // would require a separate swept-holder model and is rejected explicitly.
        if tool.shank_dia() > tool.diameter + 1e-9 || tool.neck_dia() > tool.diameter + 1e-9 {
            fail!("Parallel does not yet support shafts wider than the cutter");
        }
        let angle = p.angle.to_radians();
        let (s, c) = angle.sin_cos();
        let local = |v: V| [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]];
        let world = |u: f64, v: f64| [c * u - s * v, s * u + c * v, 0.];
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for v in self.geometry.selected.iter().flatten().copied().map(local) {
            for a in 0..2 {
                lo[a] = lo[a].min(v[a]);
                hi[a] = hi[a].max(v[a]);
            }
        }
        let inset = match p.containment {
            AdaptiveContainment::Inside => r + p.additional_offset,
            AdaptiveContainment::Center => -p.additional_offset,
            AdaptiveContainment::Outside => -r - p.additional_offset,
        };
        for a in 0..2 {
            lo[a] += inset;
            hi[a] -= inset;
        }
        if (0..2).any(|a| lo[a] >= hi[a]) {
            fail!("Parallel containment leaves no machining area");
        }
        let step = (r * p.tolerance).sqrt().clamp(0.01, 0.5);
        let rows = ((hi[1] - lo[1]) / p.stepover).ceil() as usize;
        let samples = ((hi[0] - lo[0]) / step).ceil() as usize;
        if rows > 100_000 || samples > 1_000_000 || rows.saturating_mul(samples) > 5_000_000 {
            fail!(
                "Parallel exceeds the five-million sample budget; increase stepover or tolerance"
            );
        }
        let grid = match Grid::new(&self.geometry.triangles, r.max(0.1) * 2., cancel) {
            Ok(grid) => grid,
            Err(reason) => {
                if cancel.is_cancelled() {
                    out.cancelled = true;
                    return out;
                }
                fail!(reason);
            }
        };
        let selected = match Grid::new(&self.geometry.selected, r.max(0.1) * 2., cancel) {
            Ok(grid) => grid,
            Err(reason) => {
                if cancel.is_cancelled() {
                    out.cancelled = true;
                    return out;
                }
                fail!(reason);
            }
        };
        for row in 0..=rows {
            if cancel.is_cancelled() {
                out.cancelled = true;
                return out;
            }
            let v = lo[1] + (row as f64 * p.stepover).min(hi[1] - lo[1]);
            let mut path: Vec<V> = Vec::new();
            for j in 0..=samples {
                if j % 256 == 0 && cancel.is_cancelled() {
                    out.cancelled = true;
                    return out;
                }
                let k = if p.both_ways && row % 2 == 1 {
                    samples - j
                } else {
                    j
                };
                let mut q = world(lo[0] + (hi[0] - lo[0]) * k as f64 / samples as f64, v);
                // Contact-only trimming prevents cutting across openings or
                // separate selected faces. Every disconnected run retracts.
                if !selected.contact(q) && selected.height(q, r).is_none() {
                    emit(&mut out, &mut path, op, clearance);
                    continue;
                }
                let Some(z) = grid.height(q, contact_radius) else {
                    emit(&mut out, &mut path, op, clearance);
                    continue;
                };
                q[2] = z + lift;
                // Protect the cylindrical flute above the ball: no triangle
                // within its XY disk may reach above the sphere centre. Tall
                // walls are handled by lifting the cutter conservatively.
                let ids = grid.indices(q, contact_radius);
                let wall = ids
                    .iter()
                    .filter_map(|i| flat_height(q, &grid.mesh[*i], contact_radius))
                    .fold(f64::NEG_INFINITY, f64::max);
                q[2] = q[2].max(wall - r + lift).max(self.geometry.heights.bottom);
                if q[2] > self.geometry.heights.top + lift {
                    emit(&mut out, &mut path, op, clearance);
                    continue;
                }
                if let Some(prev) = path.last().copied() {
                    let previous_length = path.len();
                    if !refine(
                        &grid,
                        prev,
                        q,
                        r,
                        error,
                        lift,
                        p.tolerance,
                        self.geometry.heights.bottom,
                        0,
                        &mut path,
                        cancel,
                    ) {
                        if cancel.is_cancelled() {
                            out.cancelled = true;
                            return out;
                        }
                        // A vertical contact discontinuity cannot be followed
                        // continuously. Discard the attempted connector and
                        // restart from clearance rather than cutting a shortcut.
                        path.truncate(previous_length);
                        emit(&mut out, &mut path, op, clearance);
                        path.push(q);
                    }
                } else {
                    path.push(q);
                }
            }
            emit(&mut out, &mut path, op, clearance);
            if out.program.steps().len() > 2_000_000 {
                fail!("Parallel exceeds the two-million motion budget; increase tolerance or stepover");
            }
        }
        let passes = out
            .program
            .steps()
            .iter()
            .filter(|step| matches!(step, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge))
            .count();
        out.diagnostics.push(Diagnostic::info(format!(
            "3D Parallel: {passes} passes at {:.3} mm stepover",
            p.stepover
        )));
        if passes == 0 {
            fail!("Parallel produced no surface contact passes");
        }
        out
    }
}
fn emit(out: &mut StrategyResult, path: &mut Vec<V>, op: &ParallelOp, clearance: f64) {
    if path.len() > 1 {
        let p = path[0];
        out.program.push(Step::Rapid {
            to: Point3::new(p[0], p[1], clearance),
            tag: Tag::new(op.id, MoveKind::Link),
        });
        out.program.push(Step::Linear {
            to: Point3::new(p[0], p[1], p[2]),
            feed: op.params.plunge_feed,
            tag: Tag::new(op.id, MoveKind::Plunge),
        });
        for p in path.iter().skip(1) {
            out.program.push(Step::Linear {
                to: Point3::new(p[0], p[1], p[2]),
                feed: op.params.feed,
                tag: Tag::new(op.id, MoveKind::Cutting),
            });
        }
        let p = path.last().unwrap();
        out.program.push(Step::Rapid {
            to: Point3::new(p[0], p[1], clearance),
            tag: Tag::new(op.id, MoveKind::Retract),
        });
    }
    path.clear();
}
// Maximum triangle Z inside a disk, for cylindrical flute protection.
fn flat_height(p: V, t: &[V; 3], r: f64) -> Option<f64> {
    let mut best = f64::NEG_INFINITY;
    for i in 0..3 {
        let a = t[i];
        let d = sub(t[(i + 1) % 3], a);
        let l = d[0] * d[0] + d[1] * d[1];
        if l < 1e-20 {
            if (a[0] - p[0]).hypot(a[1] - p[1]) <= r {
                best = best.max(a[2]);
            }
            continue;
        }
        let b = (a[0] - p[0]) * d[0] + (a[1] - p[1]) * d[1];
        let cc = (a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2) - r * r;
        let disc = b * b - l * cc;
        if disc < 0. {
            continue;
        }
        let lo = ((-b - disc.sqrt()) / l).max(0.);
        let hi = ((-b + disc.sqrt()) / l).min(1.);
        if lo <= hi {
            best = best.max(a[2] + d[2] * if d[2] >= 0. { hi } else { lo });
        }
    }
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    if n[2].abs() > 1e-12 {
        let gx = -n[0] / n[2];
        let gy = -n[1] / n[2];
        let len = gx.hypot(gy);
        let mut q = p;
        if len > 1e-12 {
            q[0] += r * gx / len;
            q[1] += r * gy / len;
        }
        if inside(q, t) {
            best = best.max(t[0][2] + gx * (q[0] - t[0][0]) + gy * (q[1] - t[0][1]));
        }
    }
    best.is_finite().then_some(best)
}
fn refine(
    grid: &Grid,
    a: V,
    b: V,
    r: f64,
    error: f64,
    lift: f64,
    tol: f64,
    bottom: f64,
    depth: u32,
    path: &mut Vec<V>,
    cancel: &CancelToken,
) -> bool {
    if cancel.is_cancelled() || path.len() > 100_000 {
        return false;
    }
    let mut mid = mul(add(a, b), 0.5);
    let z = grid.height(mid, r + error).unwrap_or(f64::NEG_INFINITY);
    let wall = grid
        .indices(mid, r + error)
        .into_iter()
        .filter_map(|i| flat_height(mid, &grid.mesh[i], r + error))
        .fold(f64::NEG_INFINITY, f64::max);
    let required = (z + lift).max(wall - r + lift).max(bottom);
    let ca = add(a, [0., 0., r]);
    let cb = add(b, [0., 0., r]);
    let half_xy = (a[0] - b[0]).hypot(a[1] - b[1]) * 0.5;
    let collision = grid.indices(mid, r + error + half_xy).into_iter().any(|i| {
        let t = &grid.mesh[i];
        segment_triangle(ca, cb, t) < (r + error).powi(2)
            || flat_height(mid, t, r + error + half_xy)
                .is_some_and(|z| z + error > ca[2].min(cb[2]))
    });
    let deviation = if z.is_finite() || wall.is_finite() {
        (required - mid[2]).abs()
    } else {
        0.
    };
    if deviation > tol * 0.25 || collision {
        if depth >= 20 {
            return false;
        }
        mid[2] = if z.is_finite() || wall.is_finite() {
            required
        } else {
            mid[2]
        };
        return refine(
            grid,
            a,
            mid,
            r,
            error,
            lift,
            tol,
            bottom,
            depth + 1,
            path,
            cancel,
        ) && refine(
            grid,
            mid,
            b,
            r,
            error,
            lift,
            tol,
            bottom,
            depth + 1,
            path,
            cancel,
        );
    }
    path.push(b);
    true
}

// Squared distance from a point to a solid triangle (Ericson's Voronoi regions).
fn point_triangle(p: V, t: &[V; 3]) -> f64 {
    let [a, b, c] = *t;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let n = cross(ab, ac);
    if dot(n, n) < 1e-24 {
        return (0..3)
            .map(|i| segment_distance(p, p, t[i], t[(i + 1) % 3]))
            .fold(f64::INFINITY, f64::min);
    }
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0. && d2 <= 0. {
        return dot(ap, ap);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0. && d4 <= d3 {
        return dot(bp, bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        let q = sub(p, add(a, mul(ab, d1 / (d1 - d3))));
        return dot(q, q);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0. && d5 <= d6 {
        return dot(cp, cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        let q = sub(p, add(a, mul(ac, d2 / (d2 - d6))));
        return dot(q, q);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0. {
        let q = sub(
            p,
            add(b, mul(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))),
        );
        return dot(q, q);
    }
    let den = va + vb + vc;
    if den.abs() < 1e-25 {
        return (0..3)
            .map(|i| segment_distance(p, p, t[i], t[(i + 1) % 3]))
            .fold(f64::INFINITY, f64::min);
    }
    let q = sub(p, add(a, add(mul(ab, vb / den), mul(ac, vc / den))));
    dot(q, q)
}
fn segment_distance(p: V, q: V, a: V, b: V) -> f64 {
    let d1 = sub(q, p);
    let d2 = sub(b, a);
    let r = sub(p, a);
    let aa = dot(d1, d1);
    let ee = dot(d2, d2);
    let f = dot(d2, r);
    let (mut s, mut t);
    if aa <= 1e-24 && ee <= 1e-24 {
        return dot(r, r);
    }
    if aa <= 1e-24 {
        s = 0.;
        t = (f / ee).clamp(0., 1.);
    } else {
        let c = dot(d1, r);
        if ee <= 1e-24 {
            t = 0.;
            s = (-c / aa).clamp(0., 1.);
        } else {
            let bb = dot(d1, d2);
            let den = aa * ee - bb * bb;
            s = if den.abs() > 1e-24 {
                ((bb * f - c * ee) / den).clamp(0., 1.)
            } else {
                0.
            };
            t = (bb * s + f) / ee;
            if t < 0. {
                t = 0.;
                s = (-c / aa).clamp(0., 1.);
            } else if t > 1. {
                t = 1.;
                s = ((bb - c) / aa).clamp(0., 1.);
            }
        }
    }
    let delta = sub(add(p, mul(d1, s)), add(a, mul(d2, t)));
    dot(delta, delta)
}
fn segment_triangle(a: V, b: V, t: &[V; 3]) -> f64 {
    let d = sub(b, a);
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() > 1e-16 {
        let s = sub(a, t[0]);
        let u = dot(s, h) / det;
        let q = cross(s, e1);
        let v = dot(d, q) / det;
        let along = dot(e2, q) / det;
        if u >= 0. && v >= 0. && u + v <= 1. && (0.0..=1.0).contains(&along) {
            return 0.;
        }
    }
    let mut best = point_triangle(a, t).min(point_triangle(b, t));
    for i in 0..3 {
        best = best.min(segment_distance(a, b, t[i], t[(i + 1) % 3]));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::SpindleDir;
    use cam_model::{CuttingHeights, Heights, ParallelParams, Tool};
    fn surface() -> ParallelGeometry {
        let triangles = vec![
            [[0., 0., 0.], [10., 0., 2.], [10., 10., 2.]],
            [[0., 0., 0.], [10., 10., 2.], [0., 10., 0.]],
        ];
        ParallelGeometry {
            selected: triangles.clone(),
            triangles,
            error_bound: 0.0025,
            heights: CuttingHeights {
                top: 3.,
                bottom: 0.,
            },
        }
    }
    fn op() -> ParallelOp {
        ParallelOp {
            id: 7,
            tool: 1,
            work_offset: 1,
            spindle_rpm: 1000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            params: ParallelParams {
                stepover: 1.,
                ..Default::default()
            },
        }
    }
    #[test]
    fn ball_contact_accounts_for_face_edge_vertex_and_vertical_wall() {
        let t = [[0., 0., 0.], [10., 0., 2.], [0., 10., 0.]];
        let expected = 0.2 * 3. + (1.04_f64.sqrt() - 1.);
        assert!((drop_ball([3., 3., 0.], &t, 1.).unwrap() - expected).abs() < 1e-10);
        assert_eq!(drop_ball([-1., 0., 0.], &t, 1.), Some(-1.));
        assert!(drop_ball([-2., 0., 0.], &t, 1.).is_none());
        let wall = [[0., 0., 0.], [0., 0., 5.], [0., 5., 5.]];
        assert!(
            (drop_ball([0.5, 2., 0.], &wall, 1.).unwrap() - (4. + 0.75_f64.sqrt())).abs() < 1e-10
        );
    }
    #[test]
    fn swept_distance_detects_ridge_between_clear_endpoints() {
        let t = [[0., -1., 0.], [0., 1., 0.], [0., 0., 2.]];
        assert_eq!(segment_triangle([-1., 0., 1.], [1., 0., 1.], &t), 0.);
        assert!((segment_triangle([-1., 0., 3.], [1., 0., 3.], &t) - 1.).abs() < 1e-10);
    }
    #[test]
    fn sloped_surface_has_variable_z_parallel_rows_and_full_clearance_links() {
        let tool = Tool {
            kind: ToolKind::BallMill,
            diameter: 2.,
            number: 1,
            length: 20.,
            ..Default::default()
        };
        let env = JobEnv {
            heights: Heights {
                clearance: 10.,
                retract: 5.,
                top_of_stock: 3.,
            },
            tools: &[tool],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let result = ParallelStrategy::new(op(), StockToLeave::default(), surface())
            .compute(&env, &CancelToken::new());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let mut last: Option<f64> = None;
        let mut zmin = f64::INFINITY;
        let mut zmax = f64::NEG_INFINITY;
        for step in result.program.steps() {
            match step {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => {
                    if let Some(prev) = last {
                        assert!((to.y - prev).abs() < 1e-8);
                    }
                    last = Some(to.y);
                    zmin = zmin.min(to.z);
                    zmax = zmax.max(to.z);
                }
                Step::Rapid { to, .. } => {
                    assert_eq!(to.z, 10.);
                    last = None;
                }
                _ => {}
            }
        }
        assert!(zmax - zmin > 1.);
    }
    #[test]
    fn invalid_inputs_and_cancellation_emit_no_motion() {
        let tool = Tool {
            number: 1,
            diameter: 2.,
            length: 20.,
            ..Default::default()
        };
        let env = JobEnv {
            heights: Heights {
                clearance: 10.,
                retract: 5.,
                top_of_stock: 3.,
            },
            tools: &[tool],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let strategy = ParallelStrategy::new(op(), StockToLeave::default(), surface());
        assert!(strategy.compute(&env, &CancelToken::new()).has_errors());
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = strategy.compute(&env, &cancel);
        assert!(r.cancelled);
        assert!(r.program.steps().is_empty());
    }
    #[test]
    fn angled_one_way_rows_remain_parallel_and_protect_unselected_boss() {
        let tool = Tool {
            number: 1,
            diameter: 2.,
            length: 20.,
            kind: ToolKind::BallMill,
            ..Default::default()
        };
        let env = JobEnv {
            heights: Heights {
                clearance: 10.,
                retract: 5.,
                top_of_stock: 5.,
            },
            tools: &[tool],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let mut geometry = surface();
        geometry.triangles.extend([
            [[4., 4., 4.], [6., 4., 4.], [6., 6., 4.]],
            [[4., 4., 4.], [6., 6., 4.], [4., 6., 4.]],
        ]);
        geometry.heights.top = 5.;
        let mut operation = op();
        operation.params.angle = 37.;
        operation.params.both_ways = false;
        let r = ParallelStrategy::new(operation, StockToLeave::default(), geometry)
            .compute(&env, &CancelToken::new());
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        let (s, c) = 37_f64.to_radians().sin_cos();
        let mut last: Option<Point3> = None;
        let mut highest = f64::NEG_INFINITY;
        for step in r.program.steps() {
            match step {
                Step::Linear { to, tag, .. } => {
                    if tag.kind == MoveKind::Cutting {
                        let p = last.unwrap();
                        let dx = to.x - p.x;
                        let dy = to.y - p.y;
                        assert!((-s * dx + c * dy).abs() < 1e-8);
                        assert!(c * dx + s * dy >= -1e-8);
                        highest = highest.max(to.z);
                    }
                    last = Some(*to);
                }
                Step::Rapid { .. } => last = None,
                _ => {}
            }
        }
        assert!(highest >= 4., "the unselected boss must lift the cutter");
    }

    #[test]
    fn clearance_below_model_and_spatial_index_overflow_fail_closed() {
        let tool = Tool {
            number: 1,
            diameter: 2.,
            length: 20.,
            kind: ToolKind::BallMill,
            ..Default::default()
        };
        let env = JobEnv {
            heights: Heights {
                clearance: 1.,
                retract: 0.5,
                top_of_stock: 0.,
            },
            tools: &[tool],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let r = ParallelStrategy::new(op(), StockToLeave::default(), surface())
            .compute(&env, &CancelToken::new());
        assert!(r.has_errors());
        assert!(r.program.steps().is_empty());
        assert!(Grid::new(
            &[[[0., 0., 0.], [1e6, 0., 0.], [0., 1e6, 0.]]],
            1.,
            &CancelToken::new()
        )
        .is_err());
    }
}
