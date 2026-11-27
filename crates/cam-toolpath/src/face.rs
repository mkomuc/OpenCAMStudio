//! Face the setup stock or a selected region with parallel passes.
//!
//! Passes run along the chosen axis, stepping over by `diameter·(1−overlap)`
//! and centered across the machining area. Selected regions clip passes and
//! check leads and transitions against the cutter-radius-compensated boundary.
//!
//! Z uses the magnitude convention: `start_offset` is the top cutting plane above
//! the drawing reference, `depth` is a positive amount removed downward, and the
//! final faced plane is `start_offset − depth`.

use cam_cldata::{ArcDir, MoveKind, Point3, Program, Step, Tag};
use cam_geo::{clip_path, offset, JoinStyle, Point, Polygon, Polyline, GRID_MM};
use cam_model::{Axis, FaceOp, Lead, ParallelTransition};

use crate::leads::{emit_lead, lead_end_point, lead_samples_n, lead_start_point};
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
    pub const PASS_LAYOUT_REVISION: u32 = 6;

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
        if !op.overshoot.is_finite() {
            fail!("operation {}: facing overshoot must be finite", op.id);
        }
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
            let passes = match if self.face_runout {
                face_paths(op, r, p, env.stock, cancel)
            } else {
                bounded_paths(op, r, p, cancel)
            } {
                Ok(paths) => paths,
                Err(reason) => fail!("operation {}: {reason}", op.id),
            };
            return cut_passes(op, env, &levels, passes, diagnostics, cancel);
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

        // Keep the centered pass layout if its natural edge overhang is enough.
        // Otherwise spread the outer rows to produce the requested overhang and
        // add intermediate rows as needed to preserve the maximum stepover.
        let stock_width = v_max - v_min;
        let nominal_intervals = (stock_width / p).ceil().max(1.) - 1.;
        let nominal_span = nominal_intervals * p;
        let row_span = nominal_span.max(stock_width + 2. * (op.overshoot.max(0.) - r));
        let intervals = if row_span > nominal_span {
            (row_span / p).ceil()
        } else {
            nominal_intervals
        };
        if intervals >= cam_model::MAX_PROFILE_LEVELS as f64 {
            fail!(
                "operation {}: too many facing passes; increase stepover",
                op.id
            );
        }
        let midpoint = v_min.midpoint(v_max);
        let step = if row_span > nominal_span {
            row_span / intervals
        } else {
            p
        };
        let centre_at = |i: usize| midpoint + (i as f64 - intervals / 2.) * step;
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

        if !u_lo.is_finite() || !u_hi.is_finite() || u_lo >= u_hi {
            fail!(
                "operation {}: overshoot leaves no facing pass length",
                op.id
            );
        }
        let paths = (0..=intervals as usize)
            .map(|i| {
                let v = centre_at(i);
                let (start, end) = if i % 2 == 0 {
                    (u_lo, u_hi)
                } else {
                    (u_hi, u_lo)
                };
                vec![
                    orient(op.direction, Point::new(start, v)),
                    orient(op.direction, Point::new(end, v)),
                ]
            })
            .collect();
        cut_passes(
            op,
            env,
            &levels,
            FacePasses {
                paths,
                link_domain: None,
                rectangular_runouts: Vec::new(),
            },
            diagnostics,
            cancel,
        )
    }
}

#[derive(Default)]
struct FacePasses {
    paths: Vec<Vec<Point>>,
    // Cutter-center polygons in travel/step coordinates. Whole-stock facing
    // has no selected region to protect.
    link_domain: Option<Vec<Polygon>>,
    rectangular_runouts: Vec<RectangularRunout>,
}

struct RectangularRunout {
    ends: (f64, f64),
    sides: (f64, f64),
    row_spacing: f64,
}

impl RectangularRunout {
    fn connects_rows(&self, from: Point, to: Point) -> bool {
        let end =
            (from.x - self.ends.0).abs() <= GRID_MM || (from.x - self.ends.1).abs() <= GRID_MM;
        end && [from.y, to.y]
            .into_iter()
            .all(|v| (self.sides.0 - GRID_MM..=self.sides.1 + GRID_MM).contains(&v))
            && ((from.y - to.y).abs() - self.row_spacing).abs() <= GRID_MM
    }
}

fn can_connect_passes(op: &FaceOp) -> bool {
    match op.transition() {
        ParallelTransition::Smooth => true,
        ParallelTransition::NoContact => false,
        _ => op.keep_tool_down(),
    }
}

fn orient(direction: Axis, p: Point) -> Point {
    match direction {
        Axis::X => p,
        Axis::Y => Point::new(p.y, p.x),
    }
}

fn face_paths(
    op: &FaceOp,
    radius: f64,
    stepover: f64,
    stock: Option<([f64; 2], [f64; 2])>,
    cancel: &CancelToken,
) -> Result<FacePasses, String> {
    // Work in travel/step coordinates. Extend pass ends along travel and
    // allow cutter overhang at the exterior sides across the rows.
    let orient = |p| orient(op.direction, p);
    let ring = |c: &cam_geo::Contour| {
        cam_geo::Contour::new(c.points().iter().copied().map(orient).collect())
    };
    let runout = radius + op.overshoot;
    if !runout.is_finite() {
        return Err("facing overshoot must be finite".into());
    }
    let mut paths = Vec::new();
    let mut link_domain = Vec::new();
    let mut rectangular_runouts = Vec::new();
    let check_links = can_connect_passes(op)
        || op.linking.effective_lead_in() != Lead::None
        || op.linking.effective_lead_out() != Lead::None;
    for region in op.boundary_regions(op.curves)? {
        let outer = Polygon::new(ring(region.outer())).map_err(|e| e.to_string())?;
        let outer = face_step_outline(outer, op.overshoot)?;
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
        if op.transition() == ParallelTransition::Smooth
            && center_limit.is_none()
            && intervals > 0.
            && op.overshoot > 0.
        {
            rectangular_runouts.push(RectangularRunout {
                ends: (u_min - runout, u_max + runout),
                sides: (middle - width / 2., middle + width / 2.),
                row_spacing: width / intervals,
            });
        }
        if check_links {
            let limits = match &center_limit {
                Some(limit) => limit.clone(),
                None => rectangular_link_domain(
                    op,
                    stock,
                    (u_min, u_max),
                    (middle, width),
                    radius,
                    runout,
                    stepover,
                )?,
            };
            if keepouts.is_empty() {
                link_domain.extend(limits);
            } else {
                link_domain
                    .extend(cam_geo::difference(&limits, &keepouts).map_err(|e| e.to_string())?);
            }
        }
        for row in 0..=intervals as usize {
            if cancel.is_cancelled() {
                return Ok(FacePasses::default());
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
    Ok(FacePasses {
        paths,
        link_domain: Some(link_domain),
        rectangular_runouts,
    })
}

// Extend the outer sides parallel to travel. A transverse shoulder within
// the face's bounds still limits the pass, and holes use their original rings.
fn face_step_outline(outer: Polygon, overhang: f64) -> Result<Polygon, String> {
    if overhang < GRID_MM {
        return Ok(outer);
    }
    let points = outer.outer().points();
    let (_, (low, high)) = bounds(points);
    let mut extensions = Vec::new();
    for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
        let shift = if (a.y - low).max(b.y - low) <= GRID_MM {
            -overhang
        } else if (high - a.y).max(high - b.y) <= GRID_MM {
            overhang
        } else {
            continue;
        };
        let strip = cam_geo::Contour::new(vec![
            a,
            b,
            Point::new(b.x, b.y + shift),
            Point::new(a.x, a.y + shift),
        ]);
        if strip.is_valid() {
            extensions.push(Polygon::new(strip).map_err(|e| e.to_string())?);
        }
    }
    if extensions.is_empty() {
        return Ok(outer);
    }
    let mut outline =
        cam_geo::union(std::slice::from_ref(&outer), &extensions).map_err(|e| e.to_string())?;
    if outline.len() != 1 {
        return Err("unable to extend the selected facing outline across the passes".into());
    }
    Ok(outline.remove(0))
}

// Allow extra space for turns and leads past a rectangular face where the pass
// already clears the stock from that end. Use the layout's transverse limits.
#[allow(clippy::too_many_arguments)]
fn rectangular_link_domain(
    op: &FaceOp,
    stock: Option<([f64; 2], [f64; 2])>,
    (u_min, u_max): (f64, f64),
    (middle, width): (f64, f64),
    radius: f64,
    runout: f64,
    stepover: f64,
) -> Result<Vec<Polygon>, String> {
    let mut lo = u_min - runout;
    let mut hi = u_max + runout;
    if let Some((stock_min, stock_max)) = stock {
        let axis = if op.direction == Axis::X { 0 } else { 1 };
        let extra = lead_reach(op.linking.effective_lead_in())
            .max(lead_reach(op.linking.effective_lead_out()))
            .max(stepover / 2.);
        if lo + radius <= stock_min[axis] {
            lo -= extra;
        }
        if hi - radius >= stock_max[axis] {
            hi += extra;
        }
    }
    if lo >= hi {
        return Ok(Vec::new());
    }
    let half = width / 2. + GRID_MM;
    // Keep measurements at the path ends on the same side of the clipping
    // boundary when CAD sectioning and the link guard use different rounding.
    lo -= GRID_MM;
    hi += GRID_MM;
    let contour = cam_geo::Contour::new(vec![
        Point::new(lo, middle - half),
        Point::new(hi, middle - half),
        Point::new(hi, middle + half),
        Point::new(lo, middle + half),
    ]);
    Polygon::new(contour)
        .map(|p| vec![p])
        .map_err(|e| e.to_string())
}

fn lead_reach(lead: Lead) -> f64 {
    match lead {
        Lead::None => 0.,
        Lead::Linear { length } => length,
        Lead::Arc { radius } | Lead::ArcSweep { radius, .. } => radius,
    }
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
) -> Result<FacePasses, String> {
    // Work in travel/step coordinates so the same clipping applies to both axes.
    let orient = |p| orient(op.direction, p);
    let ring = |contour: &cam_geo::Contour| {
        cam_geo::Contour::new(contour.points().iter().copied().map(orient).collect())
    };
    let region = Polygon::with_holes(ring(&op.boundary), op.islands.iter().map(ring).collect())
        .map_err(|e| e.to_string())?;
    if let Some(path) = exact_fit_rectangle(&region, radius) {
        return Ok(FacePasses {
            paths: vec![path.into_iter().map(orient).collect()],
            link_domain: Some(Vec::new()),
            rectangular_runouts: Vec::new(),
        });
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
    for region in &centers {
        let ((u_min, u_max), (v_min, v_max)) = bounds(region.outer().points());
        let rows = ((v_max - v_min) / stepover).ceil().max(1.0);
        if rows > cam_model::MAX_PROFILE_LEVELS as f64 {
            return Err("too many facing passes; increase stepover".into());
        }
        for row in 0..rows as usize {
            if cancel.is_cancelled() {
                return Ok(FacePasses::default());
            }
            let v = v_min + (row as f64 + 0.5) * (v_max - v_min) / rows;
            let line = Polyline::new(vec![
                Point::new(u_min - GRID_MM, v),
                Point::new(u_max + GRID_MM, v),
            ]);
            let mut strokes =
                clip_path(&line, std::slice::from_ref(region), true).map_err(|e| e.to_string())?;
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
    Ok(FacePasses {
        paths,
        link_domain: Some(centers),
        rectangular_runouts: Vec::new(),
    })
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

fn cut_passes(
    op: &FaceOp,
    env: &JobEnv,
    levels: &[f64],
    mut passes: FacePasses,
    mut diagnostics: Vec<Diagnostic>,
    cancel: &CancelToken,
) -> StrategyResult {
    // Keep the rapid above the stock even when the cutter edge is tangent
    // and the nominal retract plane is below the approach surface.
    let floor = env
        .heights
        .retract
        .max(env.heights.top_of_stock.max(op.start_offset) + crate::emit::FLOOR_CLEARANCE);
    let connect_passes = can_connect_passes(op);
    let continue_at_depth = connect_passes && op.keep_tool_down() && !op.boundary_selected;
    let mut cut = FaceCut {
        op,
        floor,
        clearance: env.heights.clearance.max(floor),
        link_domain: passes.link_domain.as_deref(),
        rectangular_runouts: &passes.rectangular_runouts,
        program: Program::new(),
        omitted_lead: false,
    };
    'levels: for (level, &z) in levels.iter().enumerate() {
        let mut at_depth = continue_at_depth && level > 0;
        for i in 0..passes.paths.len() {
            if cancel.is_cancelled() {
                break 'levels;
            }
            let (done, remaining) = passes.paths.split_at_mut(i + 1);
            let path = &done[i];
            let turn = if connect_passes {
                remaining
                    .first_mut()
                    .and_then(|next| cut.transition(path, next))
            } else {
                None
            };
            let end = turn.map_or(path[path.len() - 1], |turn| turn.from);
            cut.cut_pass(path, end, z, at_depth, i == 0);
            at_depth = turn.is_some();
            let continue_to_next_level =
                remaining.is_empty() && continue_at_depth && level + 1 < levels.len();
            if let Some(turn) = turn {
                cut.emit_turn(turn, z);
            } else if !continue_to_next_level {
                cut.exit(path, z);
            }
        }
        if !op.boundary_selected {
            // The next depth starts from the previous depth's final endpoint.
            passes.paths.reverse();
            for row in &mut passes.paths {
                row.reverse();
            }
        }
    }
    if cut.omitted_lead {
        diagnostics.push(Diagnostic::warning(format!(
            "operation {}: a facing lead did not fit the machining area and was omitted",
            op.id
        )));
    }
    StrategyResult {
        program: cut.program,
        diagnostics,
        cancelled: cancel.is_cancelled(),
    }
}

struct FaceCut<'a> {
    op: &'a FaceOp,
    floor: f64,
    clearance: f64,
    link_domain: Option<&'a [Polygon]>,
    rectangular_runouts: &'a [RectangularRunout],
    program: Program,
    omitted_lead: bool,
}

impl FaceCut<'_> {
    fn cut_pass(&mut self, path: &[Point], end: Point, z: f64, at_depth: bool, first: bool) {
        let start = path[0];
        if !at_depth {
            let lead = self.lead(path, true);
            self.rapid(lead.off, self.clearance, MoveKind::Link);
            if self.floor < self.clearance {
                self.rapid(lead.off, self.floor, MoveKind::Link);
            }
            self.linear(lead.off, z, self.op.plunge_feed, MoveKind::Plunge);
            emit_lead(
                &mut self.program,
                lead.off,
                start,
                start,
                lead.normal,
                lead.kind,
                z,
                self.op.entry_feeds.lead_in(self.op.feed),
                Tag::new(self.op.id, MoveKind::LeadIn),
            );
        } else if first {
            self.linear(start, z, self.op.plunge_feed, MoveKind::Plunge);
        }
        if path.len() > 1 {
            for &point in &path[1..path.len() - 1] {
                self.linear(point, z, self.op.feed, MoveKind::Cutting);
            }
            self.linear(end, z, self.op.feed, MoveKind::Cutting);
        }
    }

    fn exit(&mut self, path: &[Point], z: f64) {
        let end = path[path.len() - 1];
        let lead = self.lead(path, false);
        emit_lead(
            &mut self.program,
            end,
            lead.off,
            end,
            lead.normal,
            lead.kind,
            z,
            self.op.entry_feeds.lead_out(self.op.feed),
            Tag::new(self.op.id, MoveKind::LeadOut),
        );
        self.rapid(lead.off, self.clearance, MoveKind::Retract);
    }

    fn lead(&mut self, path: &[Point], entering: bool) -> FaceLead {
        let (on, tangent, kind) = if entering {
            (
                path[0],
                path.windows(2).next().and_then(|p| tangent(p[0], p[1])),
                self.op.linking.effective_lead_in(),
            )
        } else {
            (
                path[path.len() - 1],
                path.windows(2)
                    .next_back()
                    .and_then(|p| tangent(p[0], p[1])),
                self.op.linking.effective_lead_out(),
            )
        };
        let endpoint = if entering {
            lead_start_point
        } else {
            lead_end_point
        };
        let reach = lead_reach(kind);
        if reach > 0. && reach.is_finite() {
            if let Some((tx, ty)) = tangent {
                let sweep = match kind {
                    Lead::Arc { .. } => 90.,
                    Lead::ArcSweep { angle_deg, .. } => angle_deg,
                    _ => 0.,
                };
                if matches!(kind, Lead::Linear { .. }) || (0. < sweep && sweep <= 170.) {
                    for normal in [(-ty, tx), (ty, -tx)] {
                        let off = endpoint(on, (tx, ty), normal, kind);
                        if !off.x.is_finite() || !off.y.is_finite() || off == on {
                            continue;
                        }
                        let fits = self.link_domain.is_none()
                            || if sweep == 0. {
                                self.fits(&[on, off])
                            } else {
                                arc_check_segments(reach, sweep.to_radians()).is_some_and(|n| {
                                    self.fits(&lead_samples_n(on, off, normal, kind, n))
                                })
                            };
                        if fits {
                            return FaceLead { off, normal, kind };
                        }
                    }
                }
            }
        }
        self.omitted_lead |= kind != Lead::None && reach != 0.;
        FaceLead {
            off: on,
            normal: (0., 0.),
            kind: Lead::None,
        }
    }

    fn transition(&self, path: &[Point], next: &mut [Point]) -> Option<FaceTurn> {
        let end = path[path.len() - 1];
        let mut start = next[0];
        let transition = self.op.transition();
        if transition == ParallelTransition::Smooth {
            if let Some(turn) = self.smooth_turn(path, next) {
                return Some(turn);
            }
        }
        let direct = self.fits(&[end, start]);
        if self.link_domain.is_some()
            && transition == ParallelTransition::ShortestPath
            && next.len() == 2
            && (!direct || end.distance(next[1]) < end.distance(start))
            && self.fits(&[end, next[1]])
        {
            next.reverse();
            start = next[0];
        } else if !direct {
            return None;
        }
        Some(FaceTurn {
            from: end,
            to: start,
            outward: None,
        })
    }

    fn smooth_turn(&self, path: &[Point], next: &[Point]) -> Option<FaceTurn> {
        let last = path.windows(2).next_back()?;
        let (a, b) = (last[1], next[0]);
        let out = tangent(last[0], a)?;
        let incoming = tangent(b, *next.get(1)?)?;
        let gap = a.distance(b);
        let facing_each_other = (out.0 + incoming.0).hypot(out.1 + incoming.1) <= 1e-9;
        let across_row = (out.0 * (b.x - a.x) + out.1 * (b.y - a.y)).abs() <= 1e-9;
        if gap <= 1e-9 || !facing_each_other || !across_row {
            return None;
        }

        if self.turn_fits(a, b, out) {
            return Some(FaceTurn {
                from: a,
                to: b,
                outward: Some(out),
            });
        }

        // If the full outward arc cannot clear the stock envelope, round
        // inside the existing runout. The tangent points must still clear
        // the selected face, and the apex reaches the original pass end.
        let inset = gap / 2.;
        if inset + GRID_MM > self.op.overshoot
            || last[0].distance(a) <= inset
            || next[1].distance(b) <= inset
            || !self.rectangular_runouts.iter().any(|runout| {
                runout.connects_rows(orient(self.op.direction, a), orient(self.op.direction, b))
            })
        {
            return None;
        }
        let inset_point = |p: Point| Point::new(p.x - out.0 * inset, p.y - out.1 * inset);
        let (from, to) = (inset_point(a), inset_point(b));
        self.turn_fits(from, to, out).then_some(FaceTurn {
            from,
            to,
            outward: Some(out),
        })
    }

    fn emit_turn(&mut self, turn: FaceTurn, z: f64) {
        if let Some(outward) = turn.outward {
            turnaround(
                &mut self.program,
                turn.from,
                turn.to,
                outward,
                z,
                self.op.feed,
                Tag::new(self.op.id, MoveKind::Cutting),
            );
        } else if turn.from != turn.to {
            self.linear(turn.to, z, self.op.feed, MoveKind::Cutting);
        }
    }

    fn turn_fits(&self, start: Point, end: Point, out: (f64, f64)) -> bool {
        if self.link_domain.is_none() {
            return true;
        }
        let center = Point::new(start.x.midpoint(end.x), start.y.midpoint(end.y));
        let radius = start.distance(end) / 2.;
        arc_check_segments(radius, std::f64::consts::PI).is_some_and(|n| {
            let samples: Vec<_> = std::iter::once(start)
                .chain((1..n).map(|i| {
                    let angle = std::f64::consts::PI * i as f64 / n as f64;
                    let (sin, cos) = angle.sin_cos();
                    Point::new(
                        center.x + (start.x - center.x) * cos + out.0 * radius * sin,
                        center.y + (start.y - center.y) * cos + out.1 * radius * sin,
                    )
                }))
                .chain(std::iter::once(end))
                .collect();
            self.fits(&samples)
        })
    }

    fn fits(&self, samples: &[Point]) -> bool {
        let Some(domain) = self.link_domain else {
            return true;
        };
        let points: Vec<_> = samples
            .iter()
            .map(|&p| orient(self.op.direction, p))
            .collect();
        // Check the whole polyline, including chords through holes or concavities.
        points
            .iter()
            .all(|&point| domain.iter().any(|poly| poly.contains(point)))
            && clip_path(&Polyline::new(points), domain, false)
                .is_ok_and(|outside| outside.is_empty())
    }

    fn linear(&mut self, point: Point, z: f64, feed: f64, kind: MoveKind) {
        self.program.push(Step::Linear {
            to: Point3::new(point.x, point.y, z),
            feed,
            tag: Tag::new(self.op.id, kind),
        });
    }

    fn rapid(&mut self, point: Point, z: f64, kind: MoveKind) {
        self.program.push(Step::Rapid {
            to: Point3::new(point.x, point.y, z),
            tag: Tag::new(self.op.id, kind),
        });
    }
}

#[derive(Clone, Copy)]
struct FaceTurn {
    from: Point,
    to: Point,
    outward: Option<(f64, f64)>,
}

struct FaceLead {
    off: Point,
    normal: (f64, f64),
    kind: Lead,
}

fn tangent(a: Point, b: Point) -> Option<(f64, f64)> {
    let length = a.distance(b);
    (length > 1e-9).then(|| ((b.x - a.x) / length, (b.y - a.y) / length))
}

// Bound chord error by the clipping grid, within the cutter-offset margin.
fn arc_check_segments(radius: f64, sweep: f64) -> Option<usize> {
    let segments = (sweep * (radius / (8. * GRID_MM)).sqrt()).ceil().max(2.);
    (segments <= 512.).then_some(segments as usize)
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
            linking: Default::default(),
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

    #[test]
    fn transition_type_and_keep_down_control_stock_turns() {
        let mut op = face_op();
        for (transition, keep_down, plunges, lines, arcs) in [
            (ParallelTransition::Smooth, true, 1, 8, 7),
            (ParallelTransition::StraightLine, true, 1, 15, 0),
            (ParallelTransition::ShortestPath, true, 1, 15, 0),
            (ParallelTransition::NoContact, true, 8, 8, 0),
            (ParallelTransition::StraightLine, false, 8, 8, 0),
            (ParallelTransition::Smooth, false, 1, 8, 7),
        ] {
            op.linking.transition = Some(transition);
            op.linking.keep_tool_down = Some(keep_down);
            let result = run(op.clone());
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            result.program.validate().unwrap();
            assert_eq!(
                count(
                    &result,
                    |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge)
                ),
                plunges
            );
            assert_eq!(
                count(
                    &result,
                    |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Cutting)
                ),
                lines
            );
            assert_eq!(
                count(
                    &result,
                    |s| matches!(s, Step::Arc { tag, .. } if tag.kind == MoveKind::Cutting)
                ),
                arcs
            );
            assert_eq!(
                count(
                    &result,
                    |s| matches!(s, Step::Rapid { tag, .. } if tag.kind == MoveKind::Retract)
                ),
                plunges
            );
        }
    }

    #[test]
    fn facing_emits_the_requested_leads_and_independent_feeds() {
        let mut op = face_op();
        op.linking = cam_model::FaceLinking::for_new_operation();
        op.linking.lead_in = Lead::ArcSweep {
            radius: 2.,
            angle_deg: 60.,
        };
        op.linking.lead_out = Lead::Linear { length: 4. };
        op.entry_feeds.lead_in = Some(210.);
        op.entry_feeds.lead_out = Some(230.);
        let mirrored = run(op.clone());
        mirrored.program.validate().unwrap();
        assert_eq!(
            count(
                &mirrored,
                |s| matches!(s, Step::Arc { feed, tag, .. } if tag.kind == MoveKind::LeadIn && *feed == 210.)
            ),
            1
        );
        assert_eq!(
            count(
                &mirrored,
                |s| matches!(s, Step::Arc { feed, tag, end, center, .. }
                if tag.kind == MoveKind::LeadOut && *feed == 230.
                && ((end.x - center.x).hypot(end.y - center.y) - 2.).abs() < 1e-9)
            ),
            1
        );
        op.linking.same_lead_out = false;
        op.linking.lead_in_enabled = false;
        let independent = run(op);
        independent.program.validate().unwrap();
        assert_eq!(
            count(
                &independent,
                |s| matches!(s, Step::Arc { tag, .. } if tag.kind == MoveKind::LeadIn)
            ),
            0
        );
        assert_eq!(
            count(&independent, |s| matches!(s, Step::Linear { to, feed, tag }
                if tag.kind == MoveKind::LeadOut && *feed == 230. && to.x == -11.)),
            1
        );
    }

    #[test]
    fn missing_face_linking_settings_retain_legacy_motion() {
        let op = face_op();
        let mut saved = serde_json::to_value(&op).unwrap();
        saved.as_object_mut().unwrap().remove("linking");
        for boundary_selected in [false, true] {
            saved["boundary_selected"] = serde_json::json!(boundary_selected);
            let restored = serde_json::from_value::<FaceOp>(saved.clone()).unwrap();
            assert_eq!(restored.keep_tool_down(), !boundary_selected);
            assert_eq!(restored.linking.effective_lead_in(), Lead::None);
            assert_eq!(restored.linking.effective_lead_out(), Lead::None);
            assert_eq!(
                restored.transition(),
                if boundary_selected {
                    ParallelTransition::NoContact
                } else {
                    ParallelTransition::Smooth
                }
            );
            assert_eq!(can_connect_passes(&restored), !boundary_selected);
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
    fn stock_overshoot_extends_the_outer_rows_in_either_direction() {
        let mut op = face_op();
        op.overshoot = 6.;
        let stock = ([5., -19.], [43., 19.]);
        for (direction, low, high) in [(Axis::X, -19., 19.), (Axis::Y, 5., 43.)] {
            op.direction = direction;
            let result = run_stock(op.clone(), stock);
            result.program.validate().unwrap();
            let rows = pass_centres(&result, direction);
            assert!((rows[0] - 5. - (low - op.overshoot)).abs() < 1e-9);
            assert!((rows.last().unwrap() + 5. - (high + op.overshoot)).abs() < 1e-9);
            assert!(rows.windows(2).all(|pair| pair[1] - pair[0] <= 5. + 1e-9));
        }
    }

    #[test]
    fn selected_facing_respects_the_outer_boundary_and_holes_at_cutting_depth() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.linking.transition = Some(ParallelTransition::ShortestPath);
        op.linking.keep_tool_down = Some(true);
        op.islands = vec![Contour::new(vec![
            Point::new(25., 15.),
            Point::new(35., 15.),
            Point::new(35., 25.),
            Point::new(25., 25.),
        ])];
        let result = run_stock(op, ([-20., -20.], [80., 60.]));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!result.program.is_empty());
        let plunges = count(
            &result,
            |s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge),
        );
        assert!(plunges > 1, "gaps still retract");
        assert!(plunges < 9, "connected passes stay down");
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
        let all = face_paths(&op, 5., 5., None, &token).unwrap().paths;
        assert!(all.iter().any(|path| (path[0].x + 7.).abs() < GRID_MM));
        assert!(all.iter().any(|path| (path[1].x - 67.).abs() < GRID_MM));
        let crosses_hole = |y| (15. ..=25.).contains(&y);
        let hole_rows: Vec<_> = all.iter().filter(|path| crosses_hole(path[0].y)).collect();
        assert!(!hole_rows.is_empty());
        for path in hole_rows {
            assert!(path.iter().all(|p| p.x < 19.) || path.iter().all(|p| p.x > 41.));
        }

        op.curves = ProfileCurves::Outer;
        let outer = face_paths(&op, 5., 5., None, &token).unwrap().paths;
        assert!(
            outer.iter().any(|path| {
                crosses_hole(path[0].y)
                    && (path[0].x.min(path[1].x) + 7.).abs() < GRID_MM
                    && (path[0].x.max(path[1].x) - 67.).abs() < GRID_MM
            }),
            "{outer:?}"
        );

        op.curves = ProfileCurves::Inner;
        let inner = face_paths(&op, 5., 5., None, &token).unwrap().paths;
        assert!(inner
            .iter()
            .flatten()
            .all(|p| (17. - GRID_MM..=43. + GRID_MM).contains(&p.x)));
        assert!(inner
            .iter()
            .flatten()
            .all(|p| (15. - GRID_MM..=25. + GRID_MM).contains(&p.y)));
    }

    #[test]
    fn rectangular_cad_face_overshoots_both_side_edges_in_either_direction() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.overshoot = 5.1;
        for (direction, width, length) in [(Axis::X, 40., 60.), (Axis::Y, 60., 40.)] {
            op.direction = direction;
            let paths = face_paths(&op, 5., 5., None, &CancelToken::new())
                .unwrap()
                .paths;
            let first = orient(direction, paths.first().unwrap()[0]);
            let last = orient(direction, paths.last().unwrap()[0]);
            let end = orient(direction, paths[0][1]);
            assert!((first.x + 5. + op.overshoot).abs() < GRID_MM);
            assert!((end.x - length - 5. - op.overshoot).abs() < GRID_MM);
            assert!((first.y - 5. + op.overshoot).abs() < GRID_MM);
            assert!((last.y + 5. - width - op.overshoot).abs() < GRID_MM);
        }
    }

    #[test]
    fn rectangular_cad_face_uses_leads_and_turns_at_clear_stock_ends() {
        let mut op = face_op();
        op.boundary_selected = true;
        op.overshoot = 0.;
        op.linking = cam_model::FaceLinking::for_new_operation();
        let tools = [tool(10.)];
        let env = JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &tools,
            stock: Some(([0., 0.], [60., 40.])),
            spindle: cam_cldata::SpindleDir::Cw,
        };
        let result = FaceStrategy::new(op)
            .with_face_runout(true)
            .compute(&env, &CancelToken::new());
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        result.program.validate().unwrap();
        for (kind, total) in [
            (MoveKind::LeadIn, 1),
            (MoveKind::Cutting, 6),
            (MoveKind::LeadOut, 1),
        ] {
            assert_eq!(
                count(
                    &result,
                    |s| matches!(s, Step::Arc { tag, .. } if tag.kind == kind)
                ),
                total
            );
        }
    }

    #[test]
    fn indexed_smooth_face_turns_fit_inside_runout_with_keep_tool_down_off() {
        let mut op = face_op();
        op.boundary = rect(10., 18.);
        op.boundary_selected = true;
        op.direction = Axis::Y;
        op.overshoot = 5.1;
        op.linking.keep_tool_down = Some(false);
        op.linking.transition = Some(ParallelTransition::Smooth);
        let tools = [tool(10.)];
        let env = JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &tools,
            // The stock rectangle extends well past the selected flat. There
            // is room to round inside the runout, even without extra air space.
            stock: Some(([-1., -10.], [51., 28.])),
            spindle: cam_cldata::SpindleDir::Cw,
        };
        let cancel = CancelToken::new();
        let result = FaceStrategy::new(op)
            .with_face_runout(true)
            .compute(&env, &cancel);
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let turns: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter_map(|step| match step {
                Step::Arc {
                    end, center, tag, ..
                } if tag.kind == MoveKind::Cutting => {
                    Some((end.y, center.y, (end.x - center.x).abs()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(turns.len(), 3, "three connections between four passes");
        for (end_y, center_y, radius) in turns {
            assert!((end_y - center_y).abs() <= 1e-9);
            assert!((radius - 1.7).abs() <= 1e-9);
            let apex = center_y + radius.copysign(center_y - 9.);
            assert!((apex - 28.1).abs() < GRID_MM || (apex + 10.1).abs() < GRID_MM);
        }

        let index = cam_model::RotaryIndex {
            angle: 90.,
            ..Default::default()
        };
        let indexed = crate::index_program(&result.program, &index, &cancel).unwrap();
        indexed.validate().unwrap();
        assert_eq!(
            indexed.steps().iter().filter(|step| matches!(step,
                Step::Rotary { angle: 90., tag, feed: Some(_), .. } if tag.kind == MoveKind::Plunge
            )).count(),
            1,
        );
        assert!(indexed.steps().windows(2).any(|pair| matches!(
            pair,
            [
                Step::Rotary { angle: 90., to: a, tag: first, .. },
                Step::Rotary { angle: 90., to: b, tag: second, .. }
            ] if first.kind == MoveKind::Cutting && second.kind == MoveKind::Cutting
                && a.x != b.x && a.y != b.y && a.z == b.z
        )));
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
        let paths = face_paths(&op, 5., 5., None, &CancelToken::new())
            .unwrap()
            .paths;
        assert!(
            paths
                .iter()
                .flatten()
                .any(|p| p.y < 12. && (p.x - 47.).abs() < 2. * GRID_MM),
            "{paths:?}"
        );
        let shoulder: Vec<_> = paths
            .iter()
            .flatten()
            .filter(|p| (15. ..25.).contains(&p.y))
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
