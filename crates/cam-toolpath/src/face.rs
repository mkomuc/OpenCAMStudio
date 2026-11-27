//! Face the setup stock or a selected region with parallel passes.
//!
//! Passes run along the chosen axis, stepping over by `diameter·(1−overlap)`
//! and centered across the machining area. Whole-stock facing uses a continuous
//! serpentine with 180° turnarounds and one plunge per depth level. Selected
//! regions clip passes to the cutter-radius-compensated boundary and retract
//! between disconnected cuts.
//!
//! Z uses the magnitude convention: `start_offset` is the top cutting plane above
//! the drawing reference, `depth` is a positive amount removed downward, and the
//! final faced plane is `start_offset − depth`.

use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_geo::{clip_path, offset, JoinStyle, Point, Polygon, Polyline, GRID_MM};
use cam_model::{Axis, FaceOp};

use crate::profile::depth_levels;
use crate::{CancelToken, Diagnostic, JobEnv, Strategy, StrategyResult};

/// Faces a surface. Construct from a [`FaceOp`].
#[derive(Clone, Debug)]
pub struct FaceStrategy {
    op: FaceOp,
    face_runout: bool,
}

impl FaceStrategy {
    /// Revision of the pass layout used to invalidate saved toolpaths.
    pub const PASS_LAYOUT_REVISION: u32 = 3;

    /// Build a facing strategy for `op`.
    pub fn new(op: FaceOp) -> Self {
        Self {
            op,
            face_runout: false,
        }
    }

    pub(crate) fn with_face_runout(mut self, enabled: bool) -> Self {
        self.face_runout = enabled;
        self
    }
}

impl Strategy for FaceStrategy {
    fn name(&self) -> &str {
        "face"
    }

    fn compute(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        if self.op.parameter_expressions.is_empty() {
            return self.compute_resolved_parameters(env, cancel);
        }
        let mut operation = cam_model::Operation::Face(self.op.clone());
        if let Err(message) = operation.resolve_parameter_expressions(env.tools) {
            return StrategyResult {
                diagnostics: vec![crate::Diagnostic::error(message)],
                ..Default::default()
            };
        }
        let cam_model::Operation::Face(op) = operation else {
            unreachable!()
        };
        Self::new(op)
            .with_face_runout(self.face_runout)
            .compute_resolved_parameters(env, cancel)
    }
}
impl FaceStrategy {
    fn compute_resolved_parameters(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        let op = &self.op;
        let mut diagnostics = Vec::new();

        macro_rules! fail {
            ($($arg:tt)*) => {{
                diagnostics.push(Diagnostic::error(format!($($arg)*)));
                return StrategyResult { diagnostics, ..Default::default() };
            }};
        }

        let Some(tool) = env.tool(op.tool) else {
            fail!("operation {}: tool {} is not in the setup", op.id, op.tool);
        };
        // Report floor and reach differences; simulation uses the actual cutter.
        crate::guards::warn_flat_floor(op.id, "face", tool, &mut diagnostics);
        if (op.boundary_selected || env.stock.is_none()) && !op.boundary.is_valid() {
            fail!("operation {}: face boundary is not a closed area", op.id);
        }
        if op.multiple_depths.enabled && (!op.stepdown.is_finite() || op.stepdown <= 0.0) {
            fail!("operation {}: stepdown must be positive", op.id);
        }
        if op.depth <= 0.0 {
            diagnostics.push(Diagnostic::warning(format!(
                "operation {}: depth is not positive; nothing to face",
                op.id
            )));
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }

        let r = tool.radius();
        let overlap = op.absolute_overlap.unwrap_or(tool.diameter * op.overlap);
        if !overlap.is_finite() || overlap < 0.0 || overlap >= tool.diameter {
            fail!(
                "operation {}: overlap must be nonnegative and less than the tool diameter",
                op.id
            );
        }
        let p = tool.diameter - overlap; // pass spacing
        if !p.is_finite() || p <= 1e-9 {
            fail!("operation {}: overlap leaves no stepover", op.id);
        }

        let top = op.start_offset;
        let bottom = op.start_offset - op.depth;
        let stepdown = if !op.multiple_depths.enabled {
            op.depth
        } else if op.multiple_depths.even_stepdowns {
            let count = (op.depth / op.stepdown).ceil();
            if !count.is_finite() || count < 1.0 || count > cam_model::MAX_PROFILE_LEVELS as f64 {
                fail!(
                    "operation {}: invalid or excessive facing levels; check depth and stepdown",
                    op.id
                );
            }
            op.depth / count
        } else {
            op.stepdown
        };
        let levels = depth_levels(top, bottom, stepdown);
        if levels.is_empty() {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: invalid or excessive facing levels; check depth and stepdown",
                op.id
            )));
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }

        crate::guards::warn_tool_reach(op.id, "face", tool, op.depth, &mut diagnostics);
        crate::guards::warn_cut_depth(
            op.id,
            "face",
            tool,
            crate::guards::max_stepdown(top, &levels),
            &mut diagnostics,
        );

        if op.boundary_selected {
            crate::guards::warn_plunge(op.id, "face", tool, &mut diagnostics);
            let paths = match if self.face_runout {
                face_paths(op, r, p, cancel)
            } else {
                bounded_paths(op, r, p, cancel)
            } {
                Ok(paths) => paths,
                Err(reason) => fail!("operation {}: {reason}", op.id),
            };
            return cut_bounded(op, env, &levels, paths, diagnostics, cancel);
        }

        // Face the whole **stock** top (fall back to the picked boundary only when
        // there is no stock). Bounds in the (travel, step) frame for the pass direction.
        let (bx, by) = match env.stock {
            Some((smin, smax)) => ((smin[0], smax[0]), (smin[1], smax[1])),
            None => bounds(op.boundary.points()),
        };
        if ![bx.1 - bx.0, by.1 - by.0]
            .into_iter()
            .all(|span| span.is_finite() && span > 0.0)
        {
            fail!(
                "operation {}: facing requires a stock footprint with positive width and length",
                op.id
            );
        }
        let (u_min, u_max, v_min, v_max) = match op.direction {
            Axis::X => (bx.0, bx.1, by.0, by.1), // pass along X, step in Y
            Axis::Y => (by.0, by.1, bx.0, bx.1), // pass along Y, step in X
        };

        // Use one pass per stepover of stock width and center the rows so the
        // cutter extends equally past both edges while preserving the stepover.
        let n = ((v_max - v_min) / p).ceil().max(1.0) as usize;
        let midpoint = v_min.midpoint(v_max);
        let half_steps = (n - 1) as f64 / 2.0;
        let centre_at = |i: usize| midpoint + (i as f64 - half_steps) * p;
        // Travel ends: `overshoot` is the clearance between the cutter **edge** and the
        // material edge at the plunge and turnarounds, so the tool always plunges clear
        // and feeds in — the centre therefore sits a radius plus the overshoot outside
        // each end. `overshoot = 0` is tangent (just clears) — note that is a coincidence
        // of geometry, not a margin, which is why the entry below keeps a vertical one
        // regardless. A **negative** overshoot plunges into the stock. That's the user's
        // prerogative (a soft or pre-cleared top), so we warn rather than forbid.
        let u_lo = u_min - r - op.overshoot;
        let u_hi = u_max + r + op.overshoot;
        if env.stock.is_some() && op.overshoot < 0.0 {
            // Say where the tool *is* and by how much, not just that a number is
            // negative — the operator's next question is always "by how much", and the
            // answer is not on screen anywhere else.
            //
            // It is the **plunge** that enters material, not the descent: the approach
            // rapid now stops `FLOOR_CLEARANCE` above the cutting plane (see the entry
            // below), so with the usual `start_offset` = stock top it comes to rest in
            // air and the feed below it does the cutting. Worth being exact about, since
            // a warning that named the wrong move would send someone looking at the
            // wrong line of the program.
            //
            // Deliberately still a warning: a soft or pre-cleared top is the user's
            // prerogative, and `overshoot` is never clamped.
            diagnostics.push(Diagnostic::warning(format!(
                "operation {}: overshoot {:.3} mm is negative — the cutter edge starts \
                 that far inside the stock edge, so the tool enters over material and \
                 feeds down into it instead of entering clear of the edge",
                op.id, -op.overshoot
            )));
        }

        // Map a (travel u, step v) coordinate to world XY, and the world travel dir.
        let to_world = |u: f64, v: f64| match op.direction {
            Axis::X => Point::new(u, v),
            Axis::Y => Point::new(v, u),
        };
        let tdir = match op.direction {
            Axis::X => (1.0, 0.0),
            Axis::Y => (0.0, 1.0),
        };

        let cut = Tag::new(op.id, MoveKind::Cutting);
        let link = Tag::new(op.id, MoveKind::Link);
        let plunge = Tag::new(op.id, MoveKind::Plunge);
        let retract = Tag::new(op.id, MoveKind::Retract);

        let mut program = Program::new();
        // Serpentine order flips each level so the next level continues from the
        // corner where the previous one ended (plunge in place, no retract).
        let mut order: Vec<usize> = (0..n).collect();
        let mut start_forward = true;
        let mut cur = to_world(u_lo, centre_at(0));

        for (li, &z) in levels.iter().enumerate() {
            if cancel.is_cancelled() {
                return StrategyResult {
                    program,
                    diagnostics,
                    cancelled: true,
                };
            }

            let v_first = centre_at(order[0]);
            let s0 = if start_forward {
                to_world(u_lo, v_first)
            } else {
                to_world(u_hi, v_first)
            };
            if li == 0 {
                // Approach over the first pass at clearance, then rapid down — but never
                // onto the top cutting plane itself, which for facing is normally the
                // stock surface exactly (`start_offset` is set to where the material
                // starts).
                //
                // `retract.max(top)` was not enough. Whenever `start_offset >= retract`
                // the `max` picks `top` and the rapid comes to rest precisely on the
                // skin — stock standing 3 mm proud with a 2 mm retract is enough to do
                // it. What kept that safe was purely *horizontal*: the descent sits at
                // `u_lo`, a radius plus `overshoot` outside the stock edge. But the two
                // margins are independent, so at `overshoot = 0` — documented as
                // "tangent (just clears)" — both are zero at once, and the tool arrives
                // at the stock's top corner at rapid speed with clearance in neither
                // axis. Tangency is a coincidence, not a margin.
                //
                // So the vertical margin is now unconditional: stop `FLOOR_CLEARANCE`
                // above the cutting plane and let the plunge below feed the rest. Same
                // number and the same argument as `emit::rapid_floor`, which the five
                // stepdown descents use. Note this could *not* be delegated to
                // `emit::descend_to`: that helper clamps `.max(from_z)` because it ends
                // at a floor it is about to cut, so it would return this very Z.
                //
                // Costs one short feed per facing operation (not per pass), and only
                // when `top + FLOOR_CLEARANCE` clears the retract plane at all — in the
                // ordinary `start_offset = 0` setup the retract plane still wins and
                // nothing changes.
                program.push(Step::Rapid {
                    to: Point3::new(s0.x, s0.y, env.heights.clearance),
                    tag: link,
                });
                program.push(Step::Rapid {
                    to: Point3::new(
                        s0.x,
                        s0.y,
                        env.heights.retract.max(top + crate::emit::FLOOR_CLEARANCE),
                    ),
                    tag: link,
                });
            }
            // Plunge to this level (a vertical move in place for levels after the
            // first, since `s0` is exactly where the previous level ended).
            program.push(Step::Linear {
                to: Point3::new(s0.x, s0.y, z),
                feed: op.plunge_feed,
                tag: plunge,
            });

            let mut forward = start_forward;
            let mut last_forward = start_forward;
            for (pos, &ci) in order.iter().enumerate() {
                let v = centre_at(ci);
                let end = if forward {
                    to_world(u_hi, v)
                } else {
                    to_world(u_lo, v)
                };
                program.push(Step::Linear {
                    to: Point3::new(end.x, end.y, z),
                    feed: op.feed,
                    tag: cut,
                });
                if pos + 1 < n {
                    let nv = centre_at(order[pos + 1]);
                    let next_start = if forward {
                        to_world(u_hi, nv)
                    } else {
                        to_world(u_lo, nv)
                    };
                    let outward = if forward { tdir } else { (-tdir.0, -tdir.1) };
                    turnaround(&mut program, end, next_start, outward, z, op.feed, cut);
                } else {
                    cur = end;
                }
                last_forward = forward;
                forward = !forward;
            }

            order.reverse();
            start_forward = !last_forward;
        }

        program.push(Step::Rapid {
            to: Point3::new(cur.x, cur.y, env.heights.clearance),
            tag: retract,
        });

        StrategyResult {
            program,
            diagnostics,
            cancelled: false,
        }
    }
}

fn face_paths(
    op: &FaceOp,
    radius: f64,
    stepover: f64,
    cancel: &CancelToken,
) -> Result<Vec<Vec<Point>>, String> {
    // Work in travel/step coordinates. Overshoot is measured along travel
    // from the face edge to the cutter edge, as it is for whole-stock facing.
    let orient = |p: Point| match op.direction {
        Axis::X => p,
        Axis::Y => Point::new(p.y, p.x),
    };
    let ring = |c: &cam_geo::Contour| {
        cam_geo::Contour::new(c.points().iter().copied().map(orient).collect())
    };
    let runout = radius + op.overshoot;
    if !runout.is_finite() {
        return Err("facing overshoot must be finite".into());
    }
    let mut paths = Vec::new();
    for region in op.boundary_regions(op.curves)? {
        let outer = Polygon::new(ring(region.outer())).map_err(|e| e.to_string())?;
        let holes = region
            .holes()
            .iter()
            .map(|c| Polygon::new(ring(c)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let inset = radius + 2.0 * GRID_MM + radius * 0.0004;
        let keepouts = offset(&holes, inset, JoinStyle::Round).map_err(|e| e.to_string())?;
        let ((u_min, u_max), (v_min, v_max)) = bounds(outer.outer().points());
        let center_limit = face_runout_limit(&outer, radius, runout)?;
        let width = v_max - v_min - 2.0 * radius;
        if width < -cam_geometry3d::RECOGNITION_TOLERANCE {
            return Err(
                "the cutter is wider than the selected facing boundary across the passes".into(),
            );
        }
        let intervals = (width / stepover).ceil().max(0.0);
        if intervals >= cam_model::MAX_PROFILE_LEVELS as f64 {
            return Err("too many facing passes; increase stepover".into());
        }
        let middle = v_min.midpoint(v_max);
        let width = width.max(0.0);
        for row in 0..=intervals as usize {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            let v = if intervals == 0.0 {
                middle
            } else {
                middle + (row as f64 / intervals - 0.5) * width
            };
            let line = |lo, hi| Polyline::new(vec![Point::new(lo, v), Point::new(hi, v)]);
            let mut spans: Vec<_> = clip_path(
                &line(u_min - GRID_MM, u_max + GRID_MM),
                std::slice::from_ref(&outer),
                true,
            )
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|span| {
                let (extent, _) = bounds(span.points());
                extent
            })
            .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some(first) = spans.first_mut() {
                first.0 -= runout;
            }
            if let Some(last) = spans.last_mut() {
                last.1 += runout;
            }
            let mut strokes = Vec::new();
            for (lo, hi) in spans {
                if hi <= lo {
                    continue;
                }
                let pass = line(lo, hi);
                let allowed = match &center_limit {
                    Some(limit) => clip_path(&pass, limit, true).map_err(|e| e.to_string())?,
                    None => vec![pass],
                };
                for path in allowed {
                    if keepouts.is_empty() {
                        strokes.push(path);
                    } else {
                        strokes
                            .extend(clip_path(&path, &keepouts, false).map_err(|e| e.to_string())?);
                    }
                }
            }
            let mut strokes: Vec<_> = strokes
                .into_iter()
                .map(|stroke| bounds(stroke.points()).0)
                .filter(|(lo, hi)| lo < hi)
                .collect();
            strokes.sort_by(|a, b| a.0.total_cmp(&b.0));
            if row % 2 != 0 {
                strokes.reverse();
            }
            for (lo, hi) in strokes {
                let (from, to) = if row % 2 == 0 { (lo, hi) } else { (hi, lo) };
                paths.push(vec![orient(Point::new(from, v)), orient(Point::new(to, v))]);
            }
        }
    }
    if paths.is_empty() {
        return Err("no facing passes fit the selected contours and overshoot".into());
    }
    Ok(paths)
}

fn face_runout_limit(
    outer: &Polygon,
    radius: f64,
    runout: f64,
) -> Result<Option<Vec<Polygon>>, String> {
    let ((u_min, u_max), (v_min, v_max)) = bounds(outer.outer().points());
    if (outer.area() - (u_max - u_min) * (v_max - v_min)).abs() <= GRID_MM * GRID_MM {
        return Ok(None);
    }
    // Sweep the outline along travel only, then compensate for the cutter.
    // This lets a pass run out past its end while limiting lateral overhang
    // at shoulders and other nonrectangular edges.
    let reach = (runout + radius).max(0.0);
    let mut swept = vec![outer.clone()];
    if reach > 0.0 {
        let points = outer.outer().points();
        for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
            if (a.y - b.y).abs() <= f64::EPSILON {
                continue;
            }
            let contour = cam_geo::Contour::new(vec![
                Point::new(a.x - reach, a.y),
                Point::new(b.x - reach, b.y),
                Point::new(b.x + reach, b.y),
                Point::new(a.x + reach, a.y),
            ]);
            swept.push(Polygon::new(contour).map_err(|e| e.to_string())?);
        }
    }
    let swept = cam_geo::union(&swept, &[]).map_err(|e| e.to_string())?;
    // Keep tangent passes through the fixed-grid clipper.
    offset(&swept, -(radius - GRID_MM).max(0.0), JoinStyle::Round)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn bounded_paths(
    op: &FaceOp,
    radius: f64,
    stepover: f64,
    cancel: &CancelToken,
) -> Result<Vec<Vec<Point>>, String> {
    // Work in travel/step coordinates so the same clipping applies to both axes.
    let orient = |p: Point| match op.direction {
        Axis::X => p,
        Axis::Y => Point::new(p.y, p.x),
    };
    let ring = |contour: &cam_geo::Contour| {
        cam_geo::Contour::new(contour.points().iter().copied().map(orient).collect())
    };
    let region = Polygon::with_holes(ring(&op.boundary), op.islands.iter().map(ring).collect())
        .map_err(|e| e.to_string())?;
    if let Some(path) = exact_fit_rectangle(&region, radius) {
        return Ok(vec![path.into_iter().map(orient).collect()]);
    }
    // Allow for fixed-grid rounding and for the chord error of the rounded
    // offset, especially where the cutter follows an island.
    let inset = radius + 2.0 * GRID_MM + radius * 0.0004;
    let centers = offset(std::slice::from_ref(&region), -inset, JoinStyle::Round)
        .map_err(|e| e.to_string())?;
    if centers.is_empty() {
        return Err("the cutter does not fit inside the selected facing boundary".into());
    }
    let mut paths = Vec::new();
    for region in centers {
        let ((u_min, u_max), (v_min, v_max)) = bounds(region.outer().points());
        let rows = ((v_max - v_min) / stepover).ceil().max(1.0);
        if rows > cam_model::MAX_PROFILE_LEVELS as f64 {
            return Err("too many facing passes; increase stepover".into());
        }
        for row in 0..rows as usize {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            let v = v_min + (row as f64 + 0.5) * (v_max - v_min) / rows;
            let line = Polyline::new(vec![
                Point::new(u_min - GRID_MM, v),
                Point::new(u_max + GRID_MM, v),
            ]);
            let mut strokes =
                clip_path(&line, std::slice::from_ref(&region), true).map_err(|e| e.to_string())?;
            strokes.retain(Polyline::is_valid);
            strokes.sort_by(|a, b| a.points()[0].x.total_cmp(&b.points()[0].x));
            if row % 2 != 0 {
                strokes.reverse();
            }
            for stroke in strokes {
                let mut points = stroke.points().to_vec();
                if (points[0].x > points[points.len() - 1].x) != (row % 2 != 0) {
                    points.reverse();
                }
                paths.push(points.into_iter().map(orient).collect());
            }
        }
        // Follow the compensated perimeter to cover the edge between raster rows.
        for contour in std::iter::once(region.outer()).chain(region.holes()) {
            let mut points: Vec<_> = contour.points().iter().copied().map(orient).collect();
            points.push(points[0]);
            paths.push(points);
        }
    }
    Ok(paths)
}

fn exact_fit_rectangle(region: &Polygon, radius: f64) -> Option<Vec<Point>> {
    let ((x_min, x_max), (y_min, y_max)) = bounds(region.outer().points());
    let width = x_max - x_min;
    let height = y_max - y_min;
    let diameter = 2.0 * radius;
    let tolerance = cam_geometry3d::RECOGNITION_TOLERANCE;
    if !region.holes().is_empty()
        || (width.min(height) - diameter).abs() > tolerance
        || (region.area() - width * height).abs() > tolerance * width.max(height)
    {
        return None;
    }
    // Exactly fitting rectangular patches leave a line or point after
    // compensation, which a polygon offset cannot represent.
    let (start, end) = if width <= height {
        let x = x_min.midpoint(x_max);
        let pad = radius.min(height / 2.0);
        (Point::new(x, y_min + pad), Point::new(x, y_max - pad))
    } else {
        let y = y_min.midpoint(y_max);
        let pad = radius.min(width / 2.0);
        (Point::new(x_min + pad, y), Point::new(x_max - pad, y))
    };
    if start.distance(end) <= tolerance {
        Some(vec![start])
    } else {
        Some(vec![start, end])
    }
}

fn cut_bounded(
    op: &FaceOp,
    env: &JobEnv,
    levels: &[f64],
    paths: Vec<Vec<Point>>,
    diagnostics: Vec<Diagnostic>,
    cancel: &CancelToken,
) -> StrategyResult {
    let floor = env
        .heights
        .retract
        .max(env.heights.top_of_stock.max(op.start_offset) + crate::emit::FLOOR_CLEARANCE);
    let clearance = env.heights.clearance.max(floor);
    let mut program = Program::new();
    for &z in levels {
        for path in &paths {
            if cancel.is_cancelled() {
                return StrategyResult {
                    program,
                    diagnostics,
                    cancelled: true,
                };
            }
            let start = path[0];
            let rapid = |point: Point, z, kind| Step::Rapid {
                to: Point3::new(point.x, point.y, z),
                tag: Tag::new(op.id, kind),
            };
            program.push(rapid(start, clearance, MoveKind::Link));
            if floor < clearance {
                program.push(rapid(start, floor, MoveKind::Link));
            }
            program.push(Step::Linear {
                to: Point3::new(start.x, start.y, z),
                feed: op.plunge_feed,
                tag: Tag::new(op.id, MoveKind::Plunge),
            });
            for point in &path[1..] {
                program.push(Step::Linear {
                    to: Point3::new(point.x, point.y, z),
                    feed: op.feed,
                    tag: Tag::new(op.id, MoveKind::Cutting),
                });
            }
            program.push(rapid(path[path.len() - 1], clearance, MoveKind::Retract));
        }
    }
    StrategyResult {
        program,
        diagnostics,
        cancelled: cancel.is_cancelled(),
    }
}

/// XY bounding box over a point list, as `((xmin, xmax), (ymin, ymax))`.
fn bounds(pts: &[Point]) -> ((f64, f64), (f64, f64)) {
    let (mut xmin, mut ymin) = (f64::MAX, f64::MAX);
    let (mut xmax, mut ymax) = (f64::MIN, f64::MIN);
    for p in pts {
        xmin = xmin.min(p.x);
        ymin = ymin.min(p.y);
        xmax = xmax.max(p.x);
        ymax = ymax.max(p.y);
    }
    ((xmin, xmax), (ymin, ymax))
}

/// Emit a 180° turnaround arc `s → e` at height `z`, bulging toward `outward` (a
/// unit direction) so the semicircle swings clear of the part. The centre is the
/// midpoint of `s`/`e` (they are one pass-spacing apart).
fn turnaround(
    prog: &mut Program,
    s: Point,
    e: Point,
    outward: (f64, f64),
    z: f64,
    feed: f64,
    tag: Tag,
) {
    let centre = Point::new(0.5 * (s.x + e.x), 0.5 * (s.y + e.y));
    let radius = 0.5 * ((e.x - s.x).hypot(e.y - s.y));
    let apex = Point::new(centre.x + outward.0 * radius, centre.y + outward.1 * radius);
    // Orientation of the path s → apex → e picks G2/G3 (CCW when the turn is left).
    let cross = (apex.x - s.x) * (e.y - s.y) - (apex.y - s.y) * (e.x - s.x);
    let dir = if cross > 0.0 { ArcDir::Ccw } else { ArcDir::Cw };
    prog.push(Step::Arc {
        end: Point3::new(e.x, e.y, z),
        center: Point3::new(centre.x, centre.y, z),
        dir,
        feed,
        tag,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_geo::Contour;
    use cam_model::{Heights, ProfileCurves, Tool, ToolKind};

    fn rect(w: f64, h: f64) -> Contour {
        Contour::new(vec![
            Point::new(0.0, 0.0),
            Point::new(w, 0.0),
            Point::new(w, h),
            Point::new(0.0, h),
        ])
    }

    fn tool(diameter: f64) -> Tool {
        Tool {
            number: 1,
            diameter,
            length: 30.0,
            flutes: 2,
            kind: ToolKind::EndMill,
            ..Default::default()
        }
    }

    fn face_op() -> FaceOp {
        FaceOp {
            curves: Default::default(),
            boundary_selected: false,
            islands: Vec::new(),
            absolute_overlap: None,
            multiple_depths: Default::default(),
            parameter_expressions: Default::default(),
            spindle_rpm: 0.0,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            id: 0,
            tool: 1,
            boundary: rect(60.0, 40.0),
            start_offset: 1.0,
            depth: 1.0,
            stepdown: 2.0,
            overlap: 0.5,
            overshoot: 2.0,
            direction: Axis::X,
            feed: 400.0,
            plunge_feed: 150.0,
        }
    }

    fn run(op: FaceOp) -> StrategyResult {
        let tools = [tool(10.0)];
        let env = JobEnv {
            heights: Heights::new(5.0, 2.0, 0.0),
            tools: &tools,
            stock: None,
            spindle: cam_cldata::SpindleDir::Cw,
        };
        FaceStrategy::new(op).compute(&env, &CancelToken::new())
    }

    fn count<F: Fn(&Step) -> bool>(r: &StrategyResult, f: F) -> usize {
        r.program.steps().iter().filter(|s| f(s)).count()
    }

    #[test]
    fn facing_multiple_depths_and_absolute_overlap() {
        let mut op = face_op();
        op.depth = 5.0;
        op.stepdown = 2.0;
        let plunge_levels = |result: &StrategyResult| -> Vec<f64> {
            result
                .program
                .steps()
                .iter()
                .filter_map(|s| match s {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(to.z),
                    _ => None,
                })
                .collect()
        };
        let legacy = run(op.clone());
        assert_eq!(plunge_levels(&legacy), vec![-1.0, -3.0, -4.0]);
        op.absolute_overlap = Some(5.0);
        assert_eq!(legacy.program, run(op.clone()).program);
        op.multiple_depths.even_stepdowns = true;
        let even = run(op.clone());
        let z = plunge_levels(&even);
        assert_eq!(z.len(), 3);
        assert!((z[0] - (1.0 - 5.0 / 3.0)).abs() < 1e-9);
        assert!((z[1] - (1.0 - 10.0 / 3.0)).abs() < 1e-9);
        assert!((z[2] + 4.0).abs() < 1e-9);
        op.multiple_depths.enabled = false;
        assert_eq!(plunge_levels(&run(op)), vec![-4.0]);
    }

    #[test]
    fn facing_checks_total_reach_and_the_depth_of_each_pass() {
        let mut op = face_op();
        op.depth = 5.0;
        let mut cutter = tool(10.0);
        cutter.flute_length = 2.0;
        cutter.shoulder_length = 7.0;
        cutter.length_below_holder = 4.0;
        let tools = [cutter];
        let env = JobEnv {
            heights: Heights::new(5.0, 2.0, 0.0),
            tools: &tools,
            stock: None,
            spindle: cam_cldata::SpindleDir::Cw,
        };

        let stepped = FaceStrategy::new(op.clone()).compute(&env, &CancelToken::new());
        assert!(!stepped.program.is_empty());
        assert_eq!(stepped.diagnostics.len(), 1, "{:?}", stepped.diagnostics);
        assert!(stepped.diagnostics[0].message.contains("4.000 mm reach"));

        op.multiple_depths.enabled = false;
        let single = FaceStrategy::new(op).compute(&env, &CancelToken::new());
        assert!(!single.program.is_empty());
        assert_eq!(single.diagnostics.len(), 2, "{:?}", single.diagnostics);
        assert!(single.diagnostics[1]
            .message
            .contains("2.000 mm length of cut"));
    }

    #[test]
    fn continuous_serpentine_plunges_once_per_level() {
        // ⌀10, 50% overlap ⇒ p=5, step span [0,40] ⇒ centres 2.5,7.5,…,37.5 = 8 passes.
        // start_offset 1, depth 1, stepdown 2 ⇒ one level (final Z 0).
        let r = run(face_op());
        assert!(!r.has_errors(), "{:?}", r.diagnostics);

        let plunges = count(
            &r,
            |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge),
        );
        assert_eq!(
            plunges, 1,
            "one plunge for the single level, not one per pass"
        );

        let passes = count(
            &r,
            |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Cutting),
        );
        assert_eq!(passes, 8, "eight parallel passes");

        let arcs = count(
            &r,
            |s| matches!(s, Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting),
        );
        assert_eq!(arcs, 7, "seven turnaround arcs linking the passes");

        // The whole cut sits on the faced plane Z=0.
        for s in r.program.steps() {
            if let Step::Linear { to, tag, .. } = s {
                if tag.kind == MoveKind::Cutting {
                    assert!((to.z - 0.0).abs() < 1e-9, "cut at the faced plane");
                }
            }
        }
    }

    fn pass_centres(r: &StrategyResult, direction: Axis) -> Vec<f64> {
        r.program
            .steps()
            .iter()
            .filter_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => {
                    Some(match direction {
                        Axis::X => to.y,
                        Axis::Y => to.x,
                    })
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn facing_passes_are_centered_on_the_boundary_without_stock() {
        assert_eq!(
            pass_centres(&run(face_op()), Axis::X),
            vec![2.5, 7.5, 12.5, 17.5, 22.5, 27.5, 32.5, 37.5]
        );

        // 80% overlap on ⌀10 gives twenty passes at a 2 mm stepover.
        let mut op = face_op();
        op.overlap = 0.8;
        assert_eq!(
            pass_centres(&run(op), Axis::X),
            (1..40).step_by(2).map(f64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn facing_passes_are_centered_on_stock_with_a_partial_stepover() {
        let x_passes = run_stock(face_op(), ([-1.0, -19.0], [51.0, 19.0]));
        assert_eq!(
            pass_centres(&x_passes, Axis::X),
            vec![-17.5, -12.5, -7.5, -2.5, 2.5, 7.5, 12.5, 17.5]
        );

        let mut op = face_op();
        op.direction = Axis::Y;
        let y_passes = run_stock(op, ([5.0, -1.0], [43.0, 51.0]));
        assert_eq!(
            pass_centres(&y_passes, Axis::Y),
            vec![6.5, 11.5, 16.5, 21.5, 26.5, 31.5, 36.5, 41.5]
        );
    }

    #[test]
    fn selected_facing_respects_the_outer_boundary_and_holes_at_cutting_depth() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.islands = vec![Contour::new(vec![
            Point::new(25., 15.),
            Point::new(35., 15.),
            Point::new(35., 25.),
            Point::new(25., 25.),
        ])];
        let result = run_stock(op, ([-20., -20.], [80., 60.]));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!result.program.is_empty());
        let mut position = Point3::new(0., 0., 5.);
        for step in result.program.steps() {
            let (Step::Rapid { to, .. } | Step::Linear { to, .. }) = step else {
                panic!("bounded paths should only use lines");
            };
            if to.z == 0. {
                for i in 0..=100 {
                    let t = f64::from(i) / 100.;
                    let x = position.x + t * (to.x - position.x);
                    let y = position.y + t * (to.y - position.y);
                    assert!((5. - GRID_MM..=55. + GRID_MM).contains(&x));
                    assert!((5. - GRID_MM..=35. + GRID_MM).contains(&y));
                    let distance = (x - x.clamp(25., 35.)).hypot(y - y.clamp(15., 25.));
                    assert!(
                        distance >= 5. - GRID_MM,
                        "cutter enters the hole at {x}, {y}"
                    );
                }
            }
            position = *to;
        }
    }

    #[test]
    fn selected_face_runout_uses_overshoot_and_preserves_unselected_inner_areas() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.islands = vec![Contour::new(vec![
            Point::new(24., 12.),
            Point::new(36., 12.),
            Point::new(36., 28.),
            Point::new(24., 28.),
        ])];
        let token = CancelToken::new();
        let all = face_paths(&op, 5., 5., &token).unwrap();
        assert!(all.iter().any(|path| (path[0].x + 7.).abs() < GRID_MM));
        assert!(all.iter().any(|path| (path[1].x - 67.).abs() < GRID_MM));
        for path in all.iter().filter(|path| path[0].y == 20.) {
            assert!(path.iter().all(|p| p.x < 19.) || path.iter().all(|p| p.x > 41.));
        }

        op.curves = ProfileCurves::Outer;
        let outer = face_paths(&op, 5., 5., &token).unwrap();
        assert!(
            outer.iter().any(|path| {
                (path[0].y - 20.).abs() < GRID_MM
                    && (path[0].x.min(path[1].x) + 7.).abs() < GRID_MM
                    && (path[0].x.max(path[1].x) - 67.).abs() < GRID_MM
            }),
            "{outer:?}"
        );

        op.curves = ProfileCurves::Inner;
        let inner = face_paths(&op, 5., 5., &token).unwrap();
        assert!(inner
            .iter()
            .flatten()
            .all(|p| (17. - GRID_MM..=43. + GRID_MM).contains(&p.x)));
        assert!(inner
            .iter()
            .flatten()
            .all(|p| (17. - GRID_MM..=23. + GRID_MM).contains(&p.y)));
    }

    #[test]
    fn selected_face_runout_preserves_lateral_limits_at_a_shoulder() {
        let mut op = face_op();
        op.boundary = Contour::new(vec![
            Point::new(0., 0.),
            Point::new(40., 0.),
            Point::new(40., 17.),
            Point::new(15., 17.),
            Point::new(15., 40.),
            Point::new(0., 40.),
        ]);
        let paths = face_paths(&op, 5., 5., &CancelToken::new()).unwrap();
        assert!(
            paths
                .iter()
                .flatten()
                .any(|p| (p.y - 10.).abs() < GRID_MM && (p.x - 47.).abs() < 2. * GRID_MM),
            "{paths:?}"
        );
        let shoulder: Vec<_> = paths
            .iter()
            .flatten()
            .filter(|p| (p.y - 15.).abs() < GRID_MM)
            .collect();
        assert!(!shoulder.is_empty());
        assert!(shoulder.iter().all(|p| p.x < 25.), "{shoulder:?}");
    }

    #[test]
    fn an_exactly_fitting_cutter_faces_a_rectangle_on_its_centerline() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.boundary = rect(10., 18.);
        let result = run_stock(op, ([-5., -5.], [65., 45.]));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let path: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter_map(|step| match step {
                Step::Linear { to, .. } => Some((to.x, to.y)),
                _ => None,
            })
            .collect();
        assert_eq!(path, vec![(5., 5.), (5., 13.)]);
    }

    #[test]
    fn overshoot_is_edge_clearance_past_the_edge() {
        // `overshoot` is the cutter-*edge* clearance past the material edge, so the far
        // pass end (tool centre) sits at edge + radius + overshoot. No stock ⇒ the
        // boundary is the fallback: [0,60] X, r=5, overshoot=2 ⇒ far end centre 67, and
        // the trailing edge (67-5=62) clears the boundary by the 2 mm overshoot.
        let r = run(face_op());
        let first_pass_end_x = r.program.steps().iter().find_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(to.x),
            _ => None,
        });
        assert_eq!(
            first_pass_end_x,
            Some(67.0),
            "far end = edge + radius + overshoot"
        );
    }

    fn run_stock(op: FaceOp, stock: ([f64; 2], [f64; 2])) -> StrategyResult {
        let tools = [tool(10.0)];
        let env = JobEnv {
            heights: Heights::new(5.0, 2.0, 0.0),
            tools: &tools,
            stock: Some(stock),
            spindle: cam_cldata::SpindleDir::Cw,
        };
        FaceStrategy::new(op).compute(&env, &CancelToken::new())
    }

    fn first_plunge_x(r: &StrategyResult) -> f64 {
        r.program
            .steps()
            .iter()
            .find_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(to.x),
                _ => None,
            })
            .expect("a plunge move")
    }

    /// The Z the approach rapid comes to rest at — the first rapid that leaves the
    /// clearance plane. (`run_stock` uses clearance 5, retract 2.)
    fn approach_rapid_z(r: &StrategyResult) -> f64 {
        r.program
            .steps()
            .iter()
            .find_map(|s| match s {
                Step::Rapid { to, .. } if to.z < 5.0 - 1e-9 => Some(to.z),
                _ => None,
            })
            .expect("a descending approach rapid")
    }

    #[test]
    fn the_approach_rapid_never_lands_on_the_top_cutting_plane() {
        // The hazard this guards: whenever `start_offset >= retract` the old
        // `retract.max(top)` picked `top`, and for facing the top cutting plane *is* the
        // stock surface — so the rapid came to rest exactly on the skin. What kept that
        // safe was only that the descent sits outside the stock in XY, which at
        // `overshoot = 0` is itself a zero margin. Two zero margins at once is not a
        // margin.
        //
        // Retract 2, start_offset 3: the old rule gave 3.000, the plane itself.
        let mut op = face_op();
        op.start_offset = 3.0;
        op.depth = 1.0;
        op.overshoot = 0.0; // the horizontal margin gone too — the worst case
        let r = run_stock(op, ([-5.0, -5.0], [65.0, 45.0]));
        let z = approach_rapid_z(&r);
        assert!(
            z >= 3.0 + crate::emit::FLOOR_CLEARANCE - 1e-9,
            "the approach rapid stopped at Z{z:.3}, on or under the 3.000 cutting plane"
        );
    }

    #[test]
    fn the_ordinary_facing_setup_still_stops_at_the_retract_plane() {
        // The other half: the new floor must not lift every facing job. With
        // `start_offset = 0` and a 2 mm retract the retract plane is still the higher of
        // the two, so nothing moves — which is why no golden changed.
        let mut op = face_op();
        op.start_offset = 0.0;
        let r = run_stock(op, ([-5.0, -5.0], [65.0, 45.0]));
        assert!(
            (approach_rapid_z(&r) - 2.0).abs() < 1e-9,
            "expected the 2 mm retract plane, got {}",
            approach_rapid_z(&r)
        );
    }

    #[test]
    fn overshoot_is_measured_from_the_cutter_edge_to_the_stock_edge() {
        // Stock spans x=[-5,65] (proud of the 0..60 boundary). The r=5 mill plunges with
        // its edge `overshoot` clear of the stock edge (-5): centre = -5 - r - overshoot.
        let os = 3.0;
        let mut op = face_op();
        op.overshoot = os;
        let x = first_plunge_x(&run_stock(op, ([-5.0, -5.0], [65.0, 45.0])));
        assert!(
            (x - (-5.0 - 5.0 - os)).abs() < 1e-6,
            "plunge centre -5-r-overshoot: {x}"
        );
        // Edge (= centre + r) sits `overshoot` clear of the stock edge.
        assert!(
            (x + 5.0 - (-5.0 - os)).abs() < 1e-6,
            "edge clears stock by overshoot"
        );
    }

    #[test]
    fn zero_overshoot_leaves_the_cutter_edge_tangent_to_the_stock_and_warns_nothing() {
        let mut op = face_op();
        op.overshoot = 0.0;
        let r = run_stock(op, ([-5.0, -5.0], [65.0, 45.0]));
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        assert!(
            r.diagnostics.is_empty(),
            "tangent is not a warning: {:?}",
            r.diagnostics
        );
        // Edge at the stock edge: centre = -5 - r.
        assert!((first_plunge_x(&r) - (-10.0)).abs() < 1e-6);
    }

    #[test]
    fn negative_overshoot_plunges_into_the_stock_and_warns() {
        let mut op = face_op();
        op.overshoot = -2.0;
        let r = run_stock(op, ([-5.0, -5.0], [65.0, 45.0]));
        assert!(
            !r.has_errors(),
            "a warning, not an error: {:?}",
            r.diagnostics
        );
        // Checked for substance rather than a phrase: the operator needs to be told
        // *how far* inside the edge the cutter starts, and that the whole entry is over
        // material — not merely that a number was negative.
        let warning = r
            .diagnostics
            .iter()
            .find(|d| d.message.contains("overshoot"))
            .unwrap_or_else(|| panic!("an overshoot warning: {:?}", r.diagnostics));
        assert!(
            warning.message.contains("2.000"),
            "names the distance inside the edge: {}",
            warning.message
        );
        assert!(
            warning.message.contains("inside the stock edge"),
            "says where the cutter starts: {}",
            warning.message
        );
        // Edge 2 mm *inside* the stock edge (-5): centre = -5 - r - (-2) = -8.
        assert!((first_plunge_x(&r) - (-8.0)).abs() < 1e-6);
    }

    #[test]
    fn y_direction_passes_run_along_y() {
        let mut op = face_op();
        op.direction = Axis::Y;
        let r = run(op);
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        // First Y-pass steps in X: centre x = 2.5, far end y = edge + radius + overshoot
        // = 40 + 5 + 2 = 47 (boundary fallback, no stock).
        let first = r.program.steps().iter().find_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some((to.x, to.y)),
            _ => None,
        });
        assert_eq!(first, Some((2.5, 47.0)), "pass runs along Y at x=2.5");
    }

    #[test]
    fn multiple_levels_plunge_once_each_without_retracting() {
        // depth 3 at stepdown 1.5 ⇒ two levels; one plunge each, one final retract.
        let mut op = face_op();
        op.start_offset = 0.0;
        op.depth = 3.0;
        op.stepdown = 1.5;
        let r = run(op);
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        let plunges = count(
            &r,
            |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge),
        );
        assert_eq!(plunges, 2, "one plunge per level");
        let retracts = count(
            &r,
            |s| matches!(s, Step::Rapid { tag, .. } if tag.kind == MoveKind::Retract),
        );
        assert_eq!(retracts, 1, "a single retract at the very end");
    }

    #[test]
    fn path_is_continuous_and_arcs_are_well_formed() {
        // Every cutting/plunge move begins where the previous ended (no hidden
        // repositioning), and each turnaround is a true semicircle: centre
        // equidistant from both ends, diameter equal to the pass spacing (p=5).
        let r = run(face_op());
        let mut pos: Option<Point3> = None;
        for s in r.program.steps() {
            match s {
                Step::Rapid { to, .. } => pos = Some(*to),
                Step::Linear { to, tag, .. } => {
                    if tag.kind == MoveKind::Cutting {
                        let from = pos.expect("a prior position");
                        assert!(
                            (from.z - to.z).abs() < 1e-9,
                            "cutting moves stay at depth (no lift)"
                        );
                    }
                    pos = Some(*to);
                }
                Step::Arc { end, center, .. } => {
                    let from = pos.expect("an arc needs a prior position");
                    let rs = (from.x - center.x).hypot(from.y - center.y);
                    let re = (end.x - center.x).hypot(end.y - center.y);
                    assert!(
                        (rs - re).abs() < 1e-6,
                        "arc centre equidistant ({rs} vs {re})"
                    );
                    assert!((rs - 2.5).abs() < 1e-6, "turnaround radius is p/2 = 2.5");
                    assert!((from.z - end.z).abs() < 1e-9, "turnaround stays at depth");
                    pos = Some(*end);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn bad_overlap_errors() {
        let mut op = face_op();
        op.overlap = 1.0;
        assert!(run(op).has_errors(), "overlap must be < 1");
    }
}
