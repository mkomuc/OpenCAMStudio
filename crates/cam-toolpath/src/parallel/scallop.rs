//! Sampled surface-distance contours. The distance field is computed on the
//! accessible cutter-centre surface, then each contour is reprojected and
//! certified by the same swept-cutter checks as Parallel.
use super::*;
use cam_model::ScallopRetraction;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, PartialEq)]
struct QueueItem {
    distance: f64,
    index: usize,
}
impl Eq for QueueItem {}
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .distance
            .total_cmp(&self.distance)
            .then_with(|| self.index.cmp(&other.index))
    }
}
impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

type Edge = (usize, usize);
fn edge(a: usize, b: usize) -> Edge {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn preferred_start(chain: &mut Vec<Edge>, positions: &HashMap<Edge, V>, start: Option<[f64; 2]>) {
    let Some(start) = start else {
        return;
    };
    let distance = |key: &Edge| {
        let p = positions[key];
        (p[0] - start[0]).powi(2) + (p[1] - start[1]).powi(2)
    };
    if chain.len() > 2 && chain.first() == chain.last() {
        chain.pop();
        if let Some((index, _)) = chain
            .iter()
            .enumerate()
            .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
        {
            chain.rotate_left(index);
        }
        chain.push(chain[0]);
    } else if chain.len() > 1 && distance(chain.last().unwrap()) < distance(&chain[0]) {
        chain.reverse();
    }
}

// Remove vertices only when the entire old polyline stays within the filter
// tolerance of the replacement chord, remains ordered, and the chord passes
// the same selected-surface and swept-cutter checks as the original cut.
fn simplify(path: &[V], linker: &PassLinker, op: &ParallelOp, cancel: &CancelToken) -> Vec<V> {
    if !op.params.scallop_smoothing || path.len() < 3 {
        return path.to_vec();
    }
    let tolerance = op.params.scallop_smoothing_tolerance;
    let mut result = vec![path[0]];
    let mut first = 0;
    while first + 1 < path.len() && !cancel.is_cancelled() {
        let last = (first + 64).min(path.len() - 1);
        let mut chosen = first + 1;
        'candidate: for end in (first + 2..=last).rev() {
            let a = path[first];
            let b = path[end];
            let ab = sub(b, a);
            let length2 = dot(ab, ab);
            if length2 < 1e-20 {
                continue;
            }
            let mut previous_t = 0.;
            for point in &path[first + 1..end] {
                let t = dot(sub(*point, a), ab) / length2;
                if t < previous_t - 1e-10
                    || !(0.0..=1.0).contains(&t)
                    || dot(
                        sub(*point, add(a, mul(ab, t))),
                        sub(*point, add(a, mul(ab, t))),
                    ) > tolerance * tolerance
                {
                    continue 'candidate;
                }
                previous_t = t;
            }
            if !linker.selected.covers_segment(a, b, cancel) {
                continue;
            }
            let mut checked = Vec::new();
            if refine(
                linker.grid,
                a,
                b,
                linker.radius,
                linker.error,
                linker.error + op.params.tolerance + linker.axial,
                op.params.tolerance,
                linker.bottom,
                0,
                &mut checked,
                cancel,
            ) && checked.len() == 1
            {
                chosen = end;
                break;
            }
        }
        result.push(path[chosen]);
        first = chosen;
    }
    result
}

// Each short arc is enclosed by its endpoints and the intersection of their
// tangents. Require that whole triangle to lie in one selected CAD triangle;
// this certifies the arc cannot cross a small unselected gap between samples.
fn arc_clear(
    a: V,
    b: V,
    center: cam_geo::Point,
    ccw: bool,
    linker: &PassLinker,
    tolerance: f64,
    cancel: &CancelToken,
) -> bool {
    let radius = (a[0] - center.x).hypot(a[1] - center.y);
    if !radius.is_finite() || radius < 1e-9 {
        return false;
    }
    let start = (a[1] - center.y).atan2(a[0] - center.x);
    let end = (b[1] - center.y).atan2(b[0] - center.x);
    let sweep = if ccw {
        (end - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - end).rem_euclid(std::f64::consts::TAU)
    };
    if sweep < 1e-9 || sweep > std::f64::consts::TAU - 1e-9 {
        return false;
    }
    let max_sag = (tolerance * 0.125).min(0.005).max(1e-7);
    let max_angle = (2. * (1. - (max_sag / radius).min(1.)).acos()).min(0.1);
    let count = (sweep / max_angle).ceil() as usize;
    if count == 0 || count > 10_000 {
        return false;
    }
    let delta = sweep / count as f64;
    let sag = radius * (1. - (delta * 0.5).cos());
    let point = |angle: f64, r: f64| [center.x + r * angle.cos(), center.y + r * angle.sin(), a[2]];
    let sign = if ccw { 1. } else { -1. };
    for i in 0..count {
        if cancel.is_cancelled() {
            return false;
        }
        let aa = start + sign * delta * i as f64;
        let bb = aa + sign * delta;
        let p = if i == 0 { a } else { point(aa, radius) };
        let q = if i + 1 == count { b } else { point(bb, radius) };
        let tangent_intersection = point((aa + bb) * 0.5, radius / (delta * 0.5).cos());
        let mid = mul(add(p, q), 0.5);
        let search = (p[0] - q[0]).hypot(p[1] - q[1]) + sag + 1e-8;
        let contained = linker.selected.indices(mid, search).into_iter().any(|id| {
            let triangle = &linker.selected.mesh[id];
            cross(sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]))[2].abs() > 1e-12
                && inside(p, triangle)
                && inside(q, triangle)
                && inside(tangent_intersection, triangle)
        });
        if !contained || !linker.segment_clear_extra(p, q, sag + 1e-8, cancel) {
            return false;
        }
    }
    true
}

fn fitted_arcs(
    path: &[V],
    linker: &PassLinker,
    op: &ParallelOp,
    cancel: &CancelToken,
) -> Option<Vec<cam_geo::PathSeg>> {
    if !op.params.scallop_smoothing
        || path.len() < 4
        || path.iter().any(|p| (p[2] - path[0][2]).abs() > 1e-9)
    {
        return None;
    }
    let points: Vec<_> = path
        .iter()
        .map(|p| cam_geo::Point::new(p[0], p[1]))
        .collect();
    let fitted = cam_geo::fit_arcs(&points, op.params.scallop_smoothing_tolerance * 0.25);
    let mut from = path[0];
    let mut arcs = 0;
    for segment in &fitted {
        match *segment {
            cam_geo::PathSeg::Line { end } => from = [end.x, end.y, from[2]],
            cam_geo::PathSeg::Arc { end, center, ccw } => {
                let to = [end.x, end.y, from[2]];
                if !arc_clear(
                    from,
                    to,
                    center,
                    ccw,
                    linker,
                    op.params.scallop_smoothing_tolerance,
                    cancel,
                ) {
                    return None;
                }
                from = to;
                arcs += 1;
            }
        }
    }
    (arcs > 0).then_some(fitted)
}

fn emit_path(
    out: &mut StrategyResult,
    path: &mut Vec<V>,
    linker: &mut PassLinker,
    op: &ParallelOp,
    cancel: &CancelToken,
) {
    if path.len() < 2 {
        path.clear();
        return;
    }
    let arcs = fitted_arcs(path, linker, op, cancel);
    let path = if arcs.is_some() {
        path.clone()
    } else {
        simplify(path, linker, op, cancel)
    };
    let to = path[0];
    let mut at_depth = false;
    if let Some(from) = linker.previous {
        let policy = op.params.scallop_retraction;
        // Bound the spatial-index query; long or uncertain links use full clearance.
        let local = (from[0] - to[0]).hypot(from[1] - to[1]) / linker.grid.size <= 100.;
        if policy == ScallopRetraction::Shortest && local && linker.segment_clear(from, to, cancel)
        {
            out.program.push(Step::Linear {
                to: Point3::new(to[0], to[1], to[2]),
                feed: op.params.feed,
                tag: Tag::new(op.id, MoveKind::Link),
            });
            linker.previous = None;
            at_depth = true;
        } else if policy != ScallopRetraction::Full && local {
            let mid = mul(add(from, to), 0.5);
            let half = (from[0] - to[0]).hypot(from[1] - to[1]) * 0.5;
            let reach = linker.radius + linker.error + half;
            let highest = linker
                .grid
                .indices(mid, reach)
                .iter()
                .filter_map(|i| flat_height(mid, &linker.grid.mesh[*i], reach))
                .fold(f64::NEG_INFINITY, f64::max);
            let z = (highest + op.params.scallop_safe_distance + linker.error + linker.axial)
                .max(from[2])
                .max(to[2]);
            let a = [from[0], from[1], z];
            let b = [to[0], to[1], z];
            if z.is_finite()
                && z < linker.clearance
                && linker.segment_clear(from, a, cancel)
                && linker.segment_clear(a, b, cancel)
                && linker.segment_clear(b, to, cancel)
            {
                for q in [a, b] {
                    out.program.push(Step::Linear {
                        to: Point3::new(q[0], q[1], q[2]),
                        feed: op.params.feed,
                        tag: Tag::new(op.id, MoveKind::Link),
                    });
                }
                linker.previous = None;
                at_depth = true;
                out.program.push(Step::Linear {
                    to: Point3::new(to[0], to[1], to[2]),
                    feed: op.params.plunge_feed,
                    tag: Tag::new(op.id, MoveKind::Plunge),
                });
            }
        }
    }
    if !at_depth {
        linker.retract(out, op);
        out.program.push(Step::Rapid {
            to: Point3::new(to[0], to[1], linker.clearance),
            tag: Tag::new(op.id, MoveKind::Link),
        });
        out.program.push(Step::Linear {
            to: Point3::new(to[0], to[1], to[2]),
            feed: op.params.plunge_feed,
            tag: Tag::new(op.id, MoveKind::Plunge),
        });
    }
    if let Some(segments) = arcs {
        for segment in segments {
            let tag = Tag::new(op.id, MoveKind::Cutting);
            match segment {
                cam_geo::PathSeg::Line { end } => out.program.push(Step::Linear {
                    to: Point3::new(end.x, end.y, to[2]),
                    feed: op.params.feed,
                    tag,
                }),
                cam_geo::PathSeg::Arc { end, center, ccw } => out.program.push(Step::Arc {
                    end: Point3::new(end.x, end.y, to[2]),
                    center: Point3::new(center.x, center.y, to[2]),
                    dir: if ccw {
                        cam_cldata::ArcDir::Ccw
                    } else {
                        cam_cldata::ArcDir::Cw
                    },
                    feed: op.params.feed,
                    tag,
                }),
            }
        }
    } else {
        for q in path.iter().skip(1) {
            out.program.push(Step::Linear {
                to: Point3::new(q[0], q[1], q[2]),
                feed: op.params.feed,
                tag: Tag::new(op.id, MoveKind::Cutting),
            });
        }
    }
    linker.previous = path.last().copied();
    linker.passes += 1;
    // The caller owns and clears its working buffer after this call.
}

pub(super) fn compute(
    grid: &Grid,
    selected: &Grid,
    linker: &mut PassLinker,
    op: &ParallelOp,
    lo: [f64; 2],
    hi: [f64; 2],
    cancel: &CancelToken,
) -> StrategyResult {
    let mut out = StrategyResult::default();
    let p = op.params;
    macro_rules! fail {
        ($message:expr) => {{
            out.program = Default::default();
            out.diagnostics.push(Diagnostic::error($message));
            return out;
        }};
    }
    if p.scallop_start
        .is_some_and(|point| point.iter().any(|v| !v.is_finite()))
        || !p.scallop_safe_distance.is_finite()
        || p.scallop_safe_distance < 0.
        || (p.scallop_smoothing
            && (!p.scallop_smoothing_tolerance.is_finite() || p.scallop_smoothing_tolerance <= 0.))
    {
        fail!("Scallop entry, safe distance, and smoothing tolerance must be finite and valid");
    }
    // Four samples per stepover. The graph distance and contours are an
    // approximation, so this is deliberately bounded and documented.
    let spacing = (p.stepover / 4.).min(0.5);
    let counts = [0, 1].map(|axis| ((hi[axis] - lo[axis]) / spacing).ceil());
    if counts.iter().any(|n| !n.is_finite() || *n > 99_999.) {
        fail!("Scallop exceeds the one-million sample budget; increase stepover or reduce the machining area");
    }
    let nx = counts[0] as usize + 1;
    let ny = counts[1] as usize + 1;
    if nx < 2 || ny < 2 || nx > 100_000 || ny > 100_000 || nx.saturating_mul(ny) > 1_000_000 {
        fail!("Scallop exceeds the one-million sample budget; increase stepover or reduce the machining area");
    }
    let dx = (hi[0] - lo[0]) / (nx - 1) as f64;
    let dy = (hi[1] - lo[1]) / (ny - 1) as f64;
    let margin = linker.error + p.tolerance + linker.axial;
    let reach = linker.radius + linker.error;
    let mut points = vec![None; nx * ny];
    for y in 0..ny {
        if cancel.is_cancelled() {
            out.cancelled = true;
            return out;
        }
        for x in 0..nx {
            let mut q = [lo[0] + x as f64 * dx, lo[1] + y as f64 * dy, 0.];
            if !selected.contact(q) {
                continue;
            }
            let Some(z) = grid.height(q, reach) else {
                continue;
            };
            let wall = grid
                .indices(q, reach)
                .iter()
                .filter_map(|i| flat_height(q, &grid.mesh[*i], reach))
                .fold(f64::NEG_INFINITY, f64::max);
            q[2] = (z + margin)
                .max(wall - linker.radius + margin)
                .max(linker.bottom);
            if q[2] <= linker.top + margin && q[2] < linker.clearance {
                points[y * nx + x] = Some(q);
            }
        }
    }
    let mut distance = vec![f64::INFINITY; points.len()];
    let mut queue = BinaryHeap::new();
    for y in 0..ny {
        for x in 0..nx {
            let i = y * nx + x;
            if points[i].is_none() {
                continue;
            }
            if x == 0
                || y == 0
                || x + 1 == nx
                || y + 1 == ny
                || points[i - 1].is_none()
                || points[i + 1].is_none()
                || points[i - nx].is_none()
                || points[i + nx].is_none()
            {
                distance[i] = 0.;
                queue.push(QueueItem {
                    distance: 0.,
                    index: i,
                });
            }
        }
    }
    while let Some(QueueItem {
        distance: d,
        index: i,
    }) = queue.pop()
    {
        if cancel.is_cancelled() {
            out.cancelled = true;
            return out;
        }
        if d > distance[i] {
            continue;
        }
        let x = i % nx;
        let y = i / nx;
        for (ox, oy) in [
            (-1isize, -1isize),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (xx, yy) = (x as isize + ox, y as isize + oy);
            if xx < 0 || yy < 0 || xx >= nx as isize || yy >= ny as isize {
                continue;
            }
            let j = yy as usize * nx + xx as usize;
            let (Some(a), Some(b)) = (points[i], points[j]) else {
                continue;
            };
            // Do not propagate diagonally across a gap or disconnected faces.
            if ox != 0
                && oy != 0
                && (points[y * nx + xx as usize].is_none()
                    || points[yy as usize * nx + x].is_none())
            {
                continue;
            }
            let candidate = d + dot(sub(a, b), sub(a, b)).sqrt();
            if candidate < distance[j] {
                distance[j] = candidate;
                queue.push(QueueItem {
                    distance: candidate,
                    index: j,
                });
            }
        }
    }
    let max_distance = distance
        .iter()
        .copied()
        .filter(|d| d.is_finite())
        .fold(0., f64::max);
    let levels = (max_distance / p.stepover).ceil() as usize;
    if levels > 100_000 || levels.saturating_mul(points.len()) > 100_000_000 {
        fail!("Scallop exceeds the contour budget; increase stepover or reduce the machining area");
    }
    for level_index in 0..levels {
        if cancel.is_cancelled() {
            out.cancelled = true;
            return out;
        }
        // A half-step begins near the boundary and covers the centre without
        // relying on a degenerate contour exactly on a sampled vertex.
        let level = (level_index as f64 + 0.500_001) * p.stepover;
        if level >= max_distance {
            break;
        }
        let mut positions = HashMap::<Edge, V>::new();
        let mut adjacency = HashMap::<Edge, Vec<Edge>>::new();
        for y in 0..ny - 1 {
            if y % 64 == 0 && cancel.is_cancelled() {
                out.cancelled = true;
                return out;
            }
            for x in 0..nx - 1 {
                let a = y * nx + x;
                let b = a + 1;
                let c = a + nx;
                let d = c + 1;
                for tri in [[a, b, d], [a, d, c]] {
                    if tri.iter().any(|i| points[*i].is_none()) {
                        continue;
                    }
                    let mut crossings = Vec::with_capacity(2);
                    for (u, v) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                        if (distance[u] < level) == (distance[v] < level) {
                            continue;
                        }
                        let key = edge(u, v);
                        let t = (level - distance[u]) / (distance[v] - distance[u]);
                        let point = add(
                            points[u].unwrap(),
                            mul(sub(points[v].unwrap(), points[u].unwrap()), t),
                        );
                        positions.entry(key).or_insert(point);
                        crossings.push(key);
                    }
                    if crossings.len() == 2 && crossings[0] != crossings[1] {
                        adjacency
                            .entry(crossings[0])
                            .or_default()
                            .push(crossings[1]);
                        adjacency
                            .entry(crossings[1])
                            .or_default()
                            .push(crossings[0]);
                    }
                }
            }
        }
        // Trace each contour once. Prefer open endpoints, then closed loops.
        while !adjacency.is_empty() {
            let start = adjacency
                .iter()
                .filter(|(_, v)| v.len() == 1)
                .map(|(k, _)| *k)
                .min()
                .or_else(|| adjacency.keys().copied().min())
                .unwrap();
            let mut chain = vec![start];
            let mut current = start;
            loop {
                let Some(next) = adjacency.get_mut(&current).and_then(|v| v.pop()) else {
                    adjacency.remove(&current);
                    break;
                };
                if adjacency.get(&current).is_some_and(|v| v.is_empty()) {
                    adjacency.remove(&current);
                }
                if let Some(back) = adjacency.get_mut(&next) {
                    if let Some(index) = back.iter().position(|v| *v == current) {
                        back.swap_remove(index);
                    }
                    if back.is_empty() {
                        adjacency.remove(&next);
                    }
                }
                chain.push(next);
                current = next;
                if current == start {
                    break;
                }
            }
            preferred_start(&mut chain, &positions, p.scallop_start);
            let mut path = Vec::new();
            for key in chain {
                let mut q = positions[&key];
                let Some(z) = grid.height(q, reach) else {
                    emit_path(&mut out, &mut path, linker, op, cancel);
                    path.clear();
                    continue;
                };
                let wall = grid
                    .indices(q, reach)
                    .iter()
                    .filter_map(|i| flat_height(q, &grid.mesh[*i], reach))
                    .fold(f64::NEG_INFINITY, f64::max);
                q[2] = (z + margin)
                    .max(wall - linker.radius + margin)
                    .max(linker.bottom);
                if q[2] > linker.top + margin || q[2] >= linker.clearance {
                    emit_path(&mut out, &mut path, linker, op, cancel);
                    path.clear();
                    continue;
                }
                if let Some(prev) = path.last().copied() {
                    let length = path.len();
                    if !selected.covers_segment(prev, q, cancel)
                        || !refine(
                            grid,
                            prev,
                            q,
                            linker.radius,
                            linker.error,
                            margin,
                            p.tolerance,
                            linker.bottom,
                            0,
                            &mut path,
                            cancel,
                        )
                    {
                        path.truncate(length);
                        emit_path(&mut out, &mut path, linker, op, cancel);
                        path.clear();
                        path.push(q);
                    }
                } else {
                    path.push(q);
                }
            }
            emit_path(&mut out, &mut path, linker, op, cancel);
            if out.program.steps().len() > 2_000_000 {
                fail!("Scallop exceeds the two-million motion budget");
            }
        }
    }
    linker.retract(&mut out, op);
    if cancel.is_cancelled() {
        out.cancelled = true;
        return out;
    }
    if linker.passes == 0 {
        fail!("Scallop produced no surface contact passes; the region may be too narrow for this stepover");
    }
    out.diagnostics.push(Diagnostic::info(format!(
        "3D Scallop: {} sampled surface-offset passes at {:.3} mm stepover",
        linker.passes, p.stepover
    )));
    if p.scallop_smoothing {
        let cutting = out
            .program
            .steps()
            .iter()
            .filter(|step| {
                matches!(step,
            Step::Linear { tag, .. } | Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting)
            })
            .count();
        let arcs = out
            .program
            .steps()
            .iter()
            .filter(|step| {
                matches!(step,
            Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting)
            })
            .count();
        out.diagnostics.push(Diagnostic::info(format!(
            "Scallop smoothing: {cutting} cutting motions including {arcs} arcs; filtering tolerance {:.4} mm",
            p.scallop_smoothing_tolerance
        )));
    }
    out.diagnostics.push(Diagnostic::warning(
        "Scallop stepover is approximate; vertical walls and narrow features may remain unfinished. Inspect the simulated result.",
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_fits_only_certified_planar_arcs() {
        let triangle = [[-10., -10., 0.], [30., -10., 0.], [-10., 30., 0.]];
        let cancel = CancelToken::new();
        let selected_mesh = [triangle];
        let selected = Grid::new(&selected_mesh, 4., &cancel).unwrap();
        let grid = Grid::new(&selected_mesh, 4., &cancel).unwrap();
        let linker = PassLinker {
            grid: &grid,
            selected: &selected,
            radius: 1.,
            error: 0.0025,
            axial: 0.,
            clearance: 10.,
            bottom: -1.,
            top: 1.,
            previous: None,
            passes: 0,
        };
        let mut operation = ParallelOp {
            id: 1,
            tool: 1,
            spindle_rpm: 1000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        };
        operation.params.scallop_smoothing = true;
        operation.params.scallop_smoothing_tolerance = 0.05;
        let path: Vec<_> = (0..=100)
            .map(|i| {
                let angle = std::f64::consts::FRAC_PI_2 * i as f64 / 100.;
                [5. * angle.cos(), 5. * angle.sin(), 0.0225]
            })
            .collect();
        let fitted = fitted_arcs(&path, &linker, &operation, &cancel)
            .expect("safe planar circle should fit an arc");
        assert!(fitted
            .iter()
            .any(|s| matches!(s, cam_geo::PathSeg::Arc { .. })));
        let mut emitter = PassLinker {
            grid: &grid,
            selected: &selected,
            radius: 1.,
            error: 0.0025,
            axial: 0.,
            clearance: 10.,
            bottom: -1.,
            top: 1.,
            previous: None,
            passes: 0,
        };
        let mut emitted = StrategyResult::default();
        emit_path(
            &mut emitted,
            &mut path.clone(),
            &mut emitter,
            &operation,
            &cancel,
        );
        assert!(emitted
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Arc { .. })));
        assert!(emitted.program.validate().is_ok());
        let mut nonplanar = path.clone();
        nonplanar[20][2] += 0.01;
        assert!(fitted_arcs(&nonplanar, &linker, &operation, &cancel).is_none());
        let obstructed_mesh = [triangle, [[3., 2., 2.], [5., 2., 2.], [4., 4., 2.]]];
        let obstructed = Grid::new(&obstructed_mesh, 4., &cancel).unwrap();
        let blocked = PassLinker {
            grid: &obstructed,
            ..linker
        };
        assert!(
            fitted_arcs(&path, &blocked, &operation, &cancel).is_none(),
            "arc must not cross a protected obstacle"
        );
    }

    #[test]
    fn smoothing_respects_filter_distance_and_avoids_unsafe_shortcuts() {
        let mesh = [[[-10., -10., 0.], [30., -10., 0.], [-10., 30., 0.]]];
        let cancel = CancelToken::new();
        let grid = Grid::new(&mesh, 4., &cancel).unwrap();
        let linker = PassLinker {
            grid: &grid,
            selected: &grid,
            radius: 1.,
            error: 0.0025,
            axial: 0.,
            clearance: 10.,
            bottom: -1.,
            top: 1.,
            previous: None,
            passes: 0,
        };
        let mut operation = ParallelOp {
            id: 1,
            tool: 1,
            spindle_rpm: 1000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        };
        operation.params.scallop_smoothing = true;
        operation.params.scallop_smoothing_tolerance = 0.05;
        let path: Vec<_> = (0..=100)
            .map(|i| {
                let x = 1. + 8. * i as f64 / 100.;
                [x, 1. + 0.02 * x.sin(), 0.0225]
            })
            .collect();
        let reduced = simplify(&path, &linker, &operation, &cancel);
        assert!(reduced.len() < path.len() / 2);
        for point in &path {
            let d2 = reduced
                .windows(2)
                .map(|s| {
                    let v = sub(s[1], s[0]);
                    let t = (dot(sub(*point, s[0]), v) / dot(v, v)).clamp(0., 1.);
                    let d = sub(*point, add(s[0], mul(v, t)));
                    dot(d, d)
                })
                .fold(f64::INFINITY, f64::min);
            assert!(d2.sqrt() <= operation.params.scallop_smoothing_tolerance + 1e-9);
        }
    }

    #[test]
    fn shortest_link_rises_over_a_protected_obstacle() {
        let selected_mesh = [[[-10., -10., 0.], [30., -10., 0.], [-10., 30., 0.]]];
        let protected = [selected_mesh[0], [[4., 3., 4.], [6., 3., 4.], [5., 7., 4.]]];
        let cancel = CancelToken::new();
        let grid = Grid::new(&protected, 4., &cancel).unwrap();
        let selected = Grid::new(&selected_mesh, 4., &cancel).unwrap();
        let mut linker = PassLinker {
            grid: &grid,
            selected: &selected,
            radius: 1.,
            error: 0.0025,
            axial: 0.,
            clearance: 10.,
            bottom: -1.,
            top: 5.,
            previous: Some([2., 5., 0.0225]),
            passes: 0,
        };
        let mut operation = ParallelOp {
            id: 1,
            tool: 1,
            spindle_rpm: 1000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        };
        operation.params.scallop_retraction = ScallopRetraction::Shortest;
        operation.params.scallop_safe_distance = 1.;
        let mut path = vec![[8., 5., 0.0225], [8., 6., 0.0225]];
        let mut out = StrategyResult::default();
        emit_path(&mut out, &mut path, &mut linker, &operation, &cancel);
        let links: Vec<_> = out
            .program
            .steps()
            .iter()
            .filter_map(|step| match step {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Link => Some(*to),
                _ => None,
            })
            .collect();
        assert_eq!(
            links.len(),
            2,
            "blocked direct path should use the checked raised route"
        );
        assert!(links.iter().all(|p| p.z >= 5.));
    }
}
