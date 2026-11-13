//! The chamfering strategy: run a chamfer/V mill along an edge or connected chain at a
//! computed depth so its cone flank forms a bevel, or a radius mill forms a roundover.
//!
//! ## Geometry
//!
//! A chamfer mill grinds a point of half-angle `α` from the axis (half the
//! included angle) with an optional flat tip of radius `rt`. To leave a bevel of
//! horizontal width `w` along a top edge at Z `top`, run the tool:
//!
//! - offset to the **air side** of the edge by `rt` (the tool axis clears the
//!   corner by the tip radius), and
//! - at Z `top − w/tan(α)`.
//!
//! Then the flank meets the top face `w` inside the edge and the tip sits at the
//! bevel's lower corner — one pass, since a chamfer is shallow. Derived by placing
//! the cone so the tip lands at the bevel bottom (see the project notes).
//!
//! ## Chamfer mill *or* V-bit
//!
//! Either tool chamfers, because a chamfer is cut by the **flank**, which both have.
//! They differ only in what sits at the bottom: a chamfer mill has a flat tip of
//! radius `rt`, a V-bit a *rounded* one. Extending the V-bit's flank line down to the
//! tool's lowest point gives an equivalent flat radius of `rt·(1 − sin α)/cos α` — the
//! ball tucks **under** the flank — after which every formula here is unchanged.
//! (Note the asymmetry: engraving does *not* accept a chamfer mill, because it cuts
//! with the tip, which on a chamfer mill does not cut at all.)

use cam_cldata::{MoveKind, Point3, Program, Step, Tag};
use cam_geo::{offset, Contour, JoinStyle, Polygon};
use cam_model::{ChamferOp, Side, ToolKind};

use crate::{CancelToken, Diagnostic, JobEnv, Strategy, StrategyResult};
#[path = "chamfer_faces.rs"]
mod faces;

/// Chamfers an open or closed edge chain. Construct from a [`ChamferOp`].
#[derive(Clone, Debug)]
pub struct ChamferStrategy {
    op: ChamferOp,
}

impl ChamferStrategy {
    /// Build a chamfering strategy for `op`.
    pub fn new(op: ChamferOp) -> Self {
        Self { op }
    }
}

impl Strategy for ChamferStrategy {
    fn name(&self) -> &str {
        "chamfer"
    }

    fn compute(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        if self.op.parameter_expressions.is_empty()
            && self.op.contour_roughing.stepover_expression.is_none()
        {
            return self.compute_resolved(env, cancel);
        }
        let mut operation = cam_model::Operation::Chamfer(self.op.clone());
        if let Err(message) = operation.resolve_parameter_expressions(env.tools) {
            return StrategyResult {
                diagnostics: vec![Diagnostic::error(message)],
                ..Default::default()
            };
        }
        let cam_model::Operation::Chamfer(op) = operation else {
            unreachable!()
        };
        Self::new(op).compute_resolved(env, cancel)
    }
}
impl ChamferStrategy {
    fn compute_resolved(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        let rough = &self.op.contour_roughing;
        if let Err(message) = rough.validate() {
            return StrategyResult {
                diagnostics: vec![Diagnostic::error(message)],
                ..Default::default()
            };
        }
        if rough.enabled {
            if self.op.faces.is_empty() && self.op.side == Side::On {
                return StrategyResult {
                    diagnostics: vec![Diagnostic::error(
                        "Contour roughing requires an Inside or Outside edge chain",
                    )],
                    ..Default::default()
                };
            }
            let mut result = StrategyResult::default();
            for i in (0..=rough.stepovers).rev() {
                let part = self.compute_offset(env, cancel, i as f64 * rough.stepover);
                result.diagnostics.extend(part.diagnostics.clone());
                if part.cancelled || part.has_errors() {
                    result.program = Program::new();
                    result.cancelled = part.cancelled;
                    return result;
                }
                if result
                    .program
                    .steps()
                    .len()
                    .saturating_add(part.program.steps().len())
                    > cam_model::MAX_PROFILE_MOVES
                {
                    return StrategyResult {
                        diagnostics: vec![Diagnostic::error("Chamfer motion budget exceeded")],
                        ..Default::default()
                    };
                }
                result.program.extend(part.program);
            }
            return result;
        }
        self.compute_offset(env, cancel, 0.)
    }
}

impl ChamferStrategy {
    fn compute_offset(
        &self,
        env: &JobEnv,
        cancel: &CancelToken,
        rough_offset: f64,
    ) -> StrategyResult {
        let mut effective = self.op.clone();
        let entry_enabled = effective
            .lead_in_enabled
            .unwrap_or(effective.lead_in != cam_model::Lead::None);
        if effective.same_lead_out && entry_enabled {
            effective.lead_out = effective.lead_in;
        }
        if !entry_enabled {
            effective.lead_in = cam_model::Lead::None;
        }
        if effective.lead_out_enabled == Some(false) {
            effective.lead_out = cam_model::Lead::None;
        }
        let op = &effective;
        if !op.tip_offset.is_finite() || op.tip_offset < 0. {
            return StrategyResult {
                diagnostics: vec![Diagnostic::error(format!(
                    "Operation {}: chamfer tip offset must be finite and nonnegative",
                    op.id
                ))],
                ..Default::default()
            };
        }
        for lead in [op.lead_in, op.lead_out] {
            let valid = match lead {
                cam_model::Lead::None => true,
                cam_model::Lead::Linear { length } => length.is_finite() && length > 0.,
                cam_model::Lead::Arc { radius } => radius.is_finite() && radius > 0.,
                cam_model::Lead::ArcSweep { radius, angle_deg } => {
                    radius.is_finite()
                        && radius > 0.
                        && angle_deg.is_finite()
                        && angle_deg > 0.
                        && angle_deg <= 170.
                }
            };
            if !valid {
                return StrategyResult {
                    diagnostics: vec![Diagnostic::error(format!("Operation {}: lead size must be positive and finite, and arc sweep must be greater than 0 and at most 170 degrees", op.id))],
                    ..Default::default()
                };
            }
        }
        if let Err(reason) = op.entry_feeds.validate() {
            return StrategyResult {
                diagnostics: vec![Diagnostic::error(reason)],
                ..Default::default()
            };
        }
        let mut diagnostics = Vec::new();

        macro_rules! bail {
            () => {
                return StrategyResult {
                    diagnostics,
                    ..Default::default()
                }
            };
        }

        let Some(tool) = env.tool(op.tool) else {
            diagnostics.push(Diagnostic::error(format!(
                "operation {} references tool {} which is not in the setup",
                op.id, op.tool
            )));
            bail!();
        };

        // A chamfer needs an angled tool — the bevel angle comes from the tool. Both a
        // chamfer mill and a **V-bit** qualify: a chamfer is cut by the *flank*, which
        // both have. (The reverse is not true — engraving needs a cutting tip, so it
        // rejects a chamfer mill; see [`crate::engrave`].)
        //
        // `tip_offset` is the radius at which the cutting flank meets the tool's
        // lowest point — how far the axis must clear the corner:
        //
        // - **chamfer mill**: the flat tip's radius, directly.
        // - **V-bit**: the flat that its *rounded* tip is equivalent to. Extending the
        //   flank line `r = rt·cos α + (z − z_t)·tan α` down to `z = 0` (with the
        //   tangent point `z_t = rt·(1 − sin α)`) gives an intercept of
        //   `rt·(1 − sin α)/cos α` — so the ball tucks *under* the flank and the tool
        //   sits closer to the edge than its tip radius would suggest. Every formula
        //   below then carries over unchanged, since the flank line is identical.
        let radius_mill = match tool.kind {
            ToolKind::RadiusMill {
                corner_radius,
                tip_length,
            } => Some((corner_radius, tip_length)),
            _ => None,
        };
        let (included_angle_deg, tip_offset) = match tool.kind {
            ToolKind::RadiusMill {
                corner_radius,
                tip_length,
            } => {
                if !corner_radius.is_finite()
                    || corner_radius <= 0.0
                    || !tip_length.is_finite()
                    || tip_length < 0.0
                    || !tool.diameter.is_finite()
                    || tool.diameter <= 0.0
                    || !tool.flute_len().is_finite()
                    || tool.flute_len() + 1e-9 < corner_radius + tip_length
                {
                    diagnostics.push(Diagnostic::error(format!(
                        "operation {}: invalid radius mill geometry or insufficient flute length",
                        op.id
                    )));
                    bail!();
                }
                if !op.faces.is_empty() {
                    diagnostics.push(Diagnostic::error(format!("operation {}: radius mills require an edge chain; modeled chamfer faces require a straight-flank cutter", op.id)));
                    bail!();
                }
                if op.side == Side::On || op.tip_offset > 0.0 {
                    diagnostics.push(Diagnostic::error(format!(
                        "operation {}: radius mills require Inside or Outside and zero tip offset",
                        op.id
                    )));
                    bail!();
                }
                (90.0, tool.radius())
            }
            ToolKind::ChamferMill {
                included_angle_deg,
                tip_diameter,
            } => (included_angle_deg, 0.5 * tip_diameter),
            ToolKind::VBit {
                included_angle_deg,
                tip_radius,
            } => {
                let a = 0.5 * included_angle_deg.to_radians();
                let cos_a = a.cos();
                let equiv = if cos_a > 1e-9 {
                    tip_radius * (1.0 - a.sin()) / cos_a
                } else {
                    0.0
                };
                (included_angle_deg, equiv)
            }
            _ => {
                diagnostics.push(Diagnostic::error(format!(
                    "operation {}: tool {} is a {}; a chamfer needs a chamfer mill, V-bit, or radius mill",
                    op.id, op.tool, tool.kind
                )));
                bail!();
            }
        };

        if !op.faces.is_empty() {
            return faces::compute(op, env, tool, included_angle_deg, rough_offset, cancel);
        }
        if (op.closed && !op.chain.is_valid()) || (!op.closed && op.chain.len() < 2) {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: chamfer needs at least two points for an open chain or three for a closed rim",
                op.id
            )));
            bail!();
        }
        if !op.width.is_finite()
            || op.width <= 0.0
            || !op.depth.is_finite()
            || !op.top.is_finite()
            || !op.step.is_finite()
        {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: chamfer width must be positive",
                op.id
            )));
            bail!();
        }
        // Half the included angle, measured from the tool axis; the flank rises
        // `1/tan(α)` per mm of radius. Guard the degenerate cone.
        let alpha = 0.5 * included_angle_deg.to_radians();
        let tan_a = alpha.tan();
        if !(alpha > 0.0 && alpha < std::f64::consts::FRAC_PI_2) || tan_a <= 1e-9 {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: tool included angle {} is not a valid cone (0–180°)",
                op.id, included_angle_deg
            )));
            bail!();
        }

        // The tool flank shares the bevel's angle, so it lies along the finished
        // bevel plane; sliding the tool down that plane changes which flank section
        // cuts. `d_min` is the tip's natural depth (tip exactly at the bevel bottom).
        // A chosen `depth` beyond that plunges the tip `delta` deeper — into the air
        // on an external edge — so a higher flank section does the work. Sliding the
        // tool down the plane by `delta` also shifts its axis to the air side by
        // `delta·tan α`, and — crucially — that shift is the *same* for every
        // partial-width pass, so a multi-pass chamfer is one XY contour cut at a
        // sequence of Z depths.
        let width = radius_mill.map_or(op.width, |(radius, _)| radius);
        if radius_mill.is_some() && (op.width - width).abs() > 1e-6 {
            diagnostics.push(Diagnostic::warning(format!("operation {}: radius mill produces a {:.3} mm roundover; chamfer width {:.3} mm is ignored", op.id, width, op.width)));
        }
        let d_min = width / tan_a;
        let tip_depth = if let Some((_, tip_length)) = radius_mill {
            let depth = width + tip_length;
            if op.depth > 0.0 && (op.depth - depth).abs() > 1e-6 {
                diagnostics.push(Diagnostic::warning(format!("operation {}: radius mill uses a {:.3} mm tip depth; the selected depth {:.3} mm is ignored", op.id, depth, op.depth)));
            }
            depth
        } else {
            op.depth.max(d_min) + op.tip_offset
        };
        let delta = tip_depth - d_min;
        if op.side == Side::On && op.tip_offset > 0. {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: chamfer tip offset requires an Inside or Outside side so the tool can shift away from the edge",
                op.id
            )));
            bail!();
        }
        if radius_mill.is_none() && op.depth > 0.0 && op.depth < d_min {
            diagnostics.push(Diagnostic::warning(format!(
                "operation {}: depth {:.3} is above the tip's natural depth {:.3}; using the tip",
                op.id, op.depth, d_min
            )));
        }
        if radius_mill.is_some() && op.side == Side::Inside {
            diagnostics.push(Diagnostic::warning(format!("operation {}: check that the radius mill's full cutting diameter and shank clear the opposite wall and floor", op.id)));
        }
        if radius_mill.is_none() && op.side == Side::Inside && delta > 1e-9 {
            diagnostics.push(Diagnostic::warning(format!(
                "operation {}: internal chamfer plunges the tip {:.3} mm past the bevel — not collision-checked against the bore floor/opposite wall",
                op.id, delta
            )));
        }

        // The bevel is cut by the flank between the tip and `tip_depth` above it, so
        // that whole section must still be cutting surface. This is the upper bound on
        // tip depth that was previously missing (there was no flank-length geometry to
        // check against); the generatrix now supplies it.
        crate::guards::warn_axial_reach(op.id, "chamfer", tool, tip_depth, &mut diagnostics);

        // Offset the tool axis to the air side by the deep-plunge shift plus the tip
        // radius (0 for a sharp V at the tip keeps it on the edge). Sign follows the
        // profile convention.
        let off = if radius_mill.is_some() {
            tip_offset
        } else {
            delta * tan_a + tip_offset
        };
        let signed = match op.side {
            Side::Outside => off + rough_offset,
            Side::Inside => -off - rough_offset,
            Side::On => 0.0,
        };
        let tolerance = if rough_offset > 0. {
            op.contour_roughing.smoothing_deviation
        } else {
            crate::emit::ARCFIT_TOL
        };
        if !op.closed {
            return open_chamfer(
                op,
                env,
                cancel,
                signed,
                tan_a,
                delta,
                width,
                tolerance,
                diagnostics,
            );
        }
        let region = match Polygon::new(op.chain.clone()) {
            Ok(p) => p,
            Err(e) => {
                diagnostics.push(Diagnostic::error(format!(
                    "operation {}: edge is not a valid region: {e}",
                    op.id
                )));
                bail!();
            }
        };

        // The cleared/air side the lead eases in from: outward for an external edge,
        // the bore interior for an internal one (flip the normal, or the lead swings
        // onto the material side and gouges).
        let air_sign = if op.side == Side::Inside { -1.0 } else { 1.0 };
        let loops = if signed == 0.0 {
            vec![region]
        } else {
            match offset(&[region], signed, JoinStyle::Round) {
                Ok(v) => v,
                Err(e) => {
                    diagnostics.push(Diagnostic::error(format!(
                        "operation {}: offset failed: {e}",
                        op.id
                    )));
                    bail!();
                }
            }
        };
        if loops.is_empty() {
            diagnostics.push(Diagnostic::error(format!(
                "operation {}: tip offset consumed the whole edge",
                op.id
            )));
            bail!();
        }

        let link = Tag::new(op.id, MoveKind::Link);
        let plunge = Tag::new(op.id, MoveKind::Plunge);
        let cut = Tag::new(op.id, MoveKind::Cutting);
        let lead = Tag::new(op.id, MoveKind::LeadIn);
        let retract = Tag::new(op.id, MoveKind::Retract);

        // Cumulative bevel widths per pass, ending exactly at the target width. The
        // deepest (final) pass reaches `tip_depth`; shallower passes ride the same
        // XY contour at a smaller plunge.
        let widths = pass_widths(width, op.step, op.gradual);

        let mut program = Program::new();
        program.push(Step::Comment(if radius_mill.is_some() {
            format!(
                "Radius roundover: R{width:.3} mm in {}",
                crate::passes_phrase(widths.len())
            )
        } else {
            format!(
                "Chamfer: {:.3} mm wide at {}\u{00b0} in {}",
                width,
                included_angle_deg,
                crate::passes_phrase(widths.len())
            )
        }));

        for poly in &loops {
            if cancel.is_cancelled() {
                return StrategyResult {
                    program,
                    diagnostics,
                    cancelled: true,
                };
            }
            let pts = poly.outer().points();
            if pts.len() < 3 {
                continue;
            }
            // Begin the loop at the operator's chosen start (object-snap point),
            // so the plunge/entry lands there; `None` keeps the offset's first vertex.
            let rotated = crate::profile::rotate_to_start(pts, op.start);
            let pts = rotated.as_slice();
            let start = pts[0];

            // Lead geometry: the tool eases onto the edge at the start (off the air
            // side when there's a lead-in) and off it after the cut. The cut runs
            // the loop plus any closure overlap to a point `exit_on`; the lead-off
            // recomputes its outward normal there. With no leads and no overlap this
            // reduces to a straight plunge at the start and an in-place retract —
            // byte-identical to before.
            let tan_in = crate::profile::start_tangent(pts);
            let out = {
                let o = crate::profile::outward_normal(pts);
                (o.0 * air_sign, o.1 * air_sign)
            };
            // An internal chamfer eases in from the bounded bore interior — guard the
            // lead against overshooting it; an external one leads into open air.
            let guard: Vec<Polygon> = if air_sign < 0.0 {
                Polygon::new(Contour::new(pts.to_vec()))
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            };
            let (loop_pts, exit_on, tan_out) =
                crate::emit::loop_with_exit(pts, op.lead_overlap, op.end);
            // The lead-off normal follows the arrival tangent (see the profile note):
            // at a corner start it differs from the start's, so deriving it here avoids
            // a degenerate lead; mid-edge the two coincide.
            let out_exit = {
                let o = crate::profile::outward_normal_at(tan_out, crate::profile::is_ccw(pts));
                (o.0 * air_sign, o.1 * air_sign)
            };
            let lead_in = crate::leads::guard_lead(&guard, start, tan_in, out, op.lead_in, true);
            let lead_out =
                crate::leads::guard_lead(&guard, exit_on, tan_out, out_exit, op.lead_out, false);
            let entry = crate::leads::lead_start_point(start, tan_in, out, lead_in);
            let exit = crate::leads::lead_end_point(exit_on, tan_out, out_exit, lead_out);

            // One pass per cumulative width, shallow to deep, every pass over the same
            // XY loop — only Z changes, which is what widens the bevel. Each: plunge to
            // the pass depth, lead on, cut the loop (+ overlap), lead off.
            //
            // The first pass rapids down to the **retract plane**, not to the stock
            // top: ending a rapid exactly on the surface leaves no margin, so slightly
            // proud stock or a small Z-zero error means rapiding into material. Later
            // passes return to the previous width's depth, which is the same no-margin
            // case one pass down — `emit::descend_to` stops the rapid short of it and
            // feeds the last fraction.
            //
            // Better still is not to leave. The lift-and-return between passes exists
            // only to reposition, and since the loop is identical at every width there
            // is somewhere to reposition *to* only when a lead or a closure overlap puts
            // the exit somewhere other than the entry. Without one the tool stays down
            // and plunges to the next width, as the unleaded profile path does — and a
            // descent that never happens cannot end in metal.
            let repositions = (exit.x - entry.x).abs() > 1e-9 || (exit.y - entry.y).abs() > 1e-9;
            let mut prev_z = env.heights.retract.max(op.top);
            for (i, &wk) in widths.iter().enumerate() {
                let z = op.top - (wk / tan_a + delta);
                if i == 0 || repositions {
                    program.push(Step::Rapid {
                        to: Point3::new(entry.x, entry.y, env.heights.clearance),
                        tag: link,
                    });
                    crate::emit::descend_to(
                        &mut program,
                        entry,
                        prev_z,
                        &env.heights,
                        op.feed,
                        op.id,
                    );
                }
                program.push(Step::Linear {
                    to: Point3::new(entry.x, entry.y, z),
                    feed: op.plunge_feed,
                    tag: plunge,
                });
                crate::leads::emit_lead(
                    &mut program,
                    entry,
                    start,
                    start,
                    out,
                    lead_in,
                    z,
                    op.entry_feeds.lead_in(op.feed),
                    lead,
                );
                crate::emit::cut_polyline_tolerance(
                    &mut program,
                    &loop_pts,
                    op.feed,
                    cut,
                    z,
                    tolerance,
                );
                crate::leads::emit_lead(
                    &mut program,
                    exit_on,
                    exit,
                    exit_on,
                    out_exit,
                    lead_out,
                    z,
                    op.entry_feeds.lead_out(op.feed),
                    Tag::new(op.id, MoveKind::LeadOut),
                );
                // Lift only when the next pass starts somewhere else; the final retract
                // is unconditional (the operation must leave the tool clear).
                if repositions || i + 1 == widths.len() {
                    program.push(Step::Rapid {
                        to: Point3::new(exit.x, exit.y, env.heights.clearance),
                        tag: retract,
                    });
                }
                prev_z = z;
            }
        }

        StrategyResult {
            program,
            diagnostics,
            cancelled: false,
        }
    }
}

/// Open paths end at their last selected vertex and retract before each return.
/// Side follows Profile: Outside offsets left, Inside offsets right.
fn open_chamfer(
    op: &ChamferOp,
    env: &JobEnv,
    cancel: &CancelToken,
    offset: f64,
    tan_a: f64,
    delta: f64,
    width: f64,
    tolerance: f64,
    mut diagnostics: Vec<Diagnostic>,
) -> StrategyResult {
    let points = match crate::profile::offset_open_chain(op.chain.points(), offset) {
        Ok(points) => points,
        Err(error) => {
            diagnostics.push(Diagnostic::error(format!("operation {}: {error}", op.id)));
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }
    };
    if op.lead_overlap != 0. {
        diagnostics.push(Diagnostic::error(
            "Open chamfer chains cannot use closure overlap",
        ));
        return StrategyResult {
            diagnostics,
            ..Default::default()
        };
    }
    let start = points[0];
    let end = *points.last().unwrap();
    let tangent = |a: cam_geo::Point, b: cam_geo::Point| {
        let length = (b.x - a.x).hypot(b.y - a.y);
        ((b.x - a.x) / length, (b.y - a.y) / length)
    };
    let tin = tangent(points[0], points[1]);
    let tout = tangent(points[points.len() - 2], end);
    let sign = if op.side == Side::Inside { -1. } else { 1. };
    let normal = |t: (f64, f64)| (-t.1 * sign, t.0 * sign);
    let nin = normal(tin);
    let nout = normal(tout);
    let entry = crate::leads::lead_start_point(start, tin, nin, op.lead_in);
    let exit = crate::leads::lead_end_point(end, tout, nout, op.lead_out);
    let mut program = Program::new();
    for width in pass_widths(width, op.step, op.gradual) {
        if cancel.is_cancelled() {
            return StrategyResult {
                program,
                diagnostics,
                cancelled: true,
            };
        }
        let z = op.top - (width / tan_a + delta);
        program.push(Step::Rapid {
            to: Point3::new(entry.x, entry.y, env.heights.clearance),
            tag: Tag::new(op.id, MoveKind::Link),
        });
        crate::emit::descend_to(
            &mut program,
            entry,
            env.heights.retract.max(op.top),
            &env.heights,
            op.feed,
            op.id,
        );
        program.push(Step::Linear {
            to: Point3::new(entry.x, entry.y, z),
            feed: op.plunge_feed,
            tag: Tag::new(op.id, MoveKind::Plunge),
        });
        crate::leads::emit_lead(
            &mut program,
            entry,
            start,
            start,
            nin,
            op.lead_in,
            z,
            op.entry_feeds.lead_in(op.feed),
            Tag::new(op.id, MoveKind::LeadIn),
        );
        crate::emit::cut_polyline_tolerance(
            &mut program,
            &points,
            op.feed,
            Tag::new(op.id, MoveKind::Cutting),
            z,
            tolerance,
        );
        crate::leads::emit_lead(
            &mut program,
            end,
            exit,
            end,
            nout,
            op.lead_out,
            z,
            op.entry_feeds.lead_out(op.feed),
            Tag::new(op.id, MoveKind::LeadOut),
        );
        program.push(Step::Rapid {
            to: Point3::new(exit.x, exit.y, env.heights.clearance),
            tag: Tag::new(op.id, MoveKind::Retract),
        });
    }
    StrategyResult {
        program,
        diagnostics,
        cancelled: false,
    }
}

/// The cumulative bevel width at each pass, always ending exactly at `width`.
///
/// - `step <= 0` or `step >= width`: a single pass at the full width.
/// - **uniform** (`!gradual`): equal width increments `step, 2·step, …` — simplest,
///   but the material removed grows each pass (bevel area ∝ width²).
/// - **gradual**: equal *material* per pass, so widths follow `step·√k` (the first
///   pass is `step` wide); the increments shrink as the bevel widens.
fn pass_widths(width: f64, step: f64, gradual: bool) -> Vec<f64> {
    if step <= 0.0 || step >= width {
        return vec![width];
    }
    let mut ws = Vec::new();
    if gradual {
        let mut k = 1.0_f64;
        loop {
            let w = step * k.sqrt();
            if w >= width {
                break;
            }
            ws.push(w);
            k += 1.0;
        }
    } else {
        let mut w = step;
        while w < width {
            ws.push(w);
            w += step;
        }
    }
    // Land the final pass exactly on the target (avoid a duplicate/sliver pass).
    if ws.last().is_some_and(|&l| (width - l).abs() < 1e-9) {
        ws.pop();
    }
    ws.push(width);
    ws
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::Step;
    use cam_geo::{Contour, Point};
    use cam_model::{Heights, Lead, Tool};

    fn vbit_tool(included_angle_deg: f64, tip_radius: f64) -> Tool {
        Tool {
            number: 1,
            diameter: 6.0,
            length: 30.0,
            flutes: 1,
            kind: ToolKind::VBit {
                included_angle_deg,
                tip_radius,
            },
            ..Default::default()
        }
    }

    fn chamfer_tool(included_angle_deg: f64, tip_diameter: f64) -> Tool {
        Tool {
            number: 1,
            diameter: 6.0,
            length: 30.0,
            flutes: 1,
            kind: ToolKind::ChamferMill {
                included_angle_deg,
                tip_diameter,
            },
            ..Default::default()
        }
    }

    fn square() -> Contour {
        Contour::new(vec![
            Point::new(0.0, 0.0),
            Point::new(20.0, 0.0),
            Point::new(20.0, 20.0),
            Point::new(0.0, 20.0),
        ])
    }

    fn op(width: f64) -> ChamferOp {
        ChamferOp {
            parameter_expressions: Default::default(),
            contour_roughing: Default::default(),
            tip_offset: 0.0,
            lead_in_enabled: None,
            same_lead_out: false,
            lead_out_enabled: None,
            closed: true,
            faces: Vec::new(),
            stepdown: 0.,
            spindle_rpm: 0.0,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            id: 0,
            tool: 1,
            chain: square(),
            side: Side::Outside,
            width,
            top: 0.0,
            depth: 0.0,
            step: 0.0,
            gradual: false,
            feed: 200.0,
            plunge_feed: 100.0,
            end: None,
            start: None,
            lead_in: Lead::None,
            lead_out: Lead::None,
            lead_overlap: 0.0,
        }
    }

    fn run(op: ChamferOp, tool: Tool) -> StrategyResult {
        let tools = [tool];
        let env = JobEnv {
            heights: Heights::new(5.0, 2.0, 0.0),
            tools: &tools,
            stock: None,
            spindle: cam_cldata::SpindleDir::Cw,
        };
        ChamferStrategy::new(op).compute(&env, &CancelToken::new())
    }

    fn cut_depth(prog: &Program) -> Option<f64> {
        prog.steps().iter().find_map(|s| match s {
            Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(to.z),
            _ => None,
        })
    }

    #[test]
    fn contour_roughing_offsets_open_chamfers_and_retains_final_bevel() {
        for (side, expected) in [
            (Side::Outside, vec![2., 1., 0.]),
            (Side::Inside, vec![-2., -1., 0.]),
        ] {
            let mut op = op(1.);
            op.closed = false;
            op.chain = Contour::new(vec![Point::new(0., 0.), Point::new(20., 0.)]);
            op.side = side;
            op.contour_roughing.enabled = true;
            op.contour_roughing.stepovers = 2;
            let result = run(op, chamfer_tool(90., 0.));
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let cuts: Vec<_> = result
                .program
                .steps()
                .iter()
                .filter_map(|step| match step {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
                    _ => None,
                })
                .collect();
            assert_eq!(cuts.len(), 3);
            for (cut, y) in cuts.iter().zip(expected) {
                assert!((cut.y - y).abs() < 1e-8);
                assert!((cut.z + 1.).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn closed_chamfer_roughing_preserves_the_final_bevel() {
        for side in [Side::Inside, Side::Outside] {
            let mut op = op(1.);
            op.side = side;
            let baseline = run(op.clone(), chamfer_tool(90., 0.));
            assert!(!baseline.has_errors(), "{:?}", baseline.diagnostics);
            op.contour_roughing.enabled = true;
            op.contour_roughing.stepovers = 2;
            let rough = run(op, chamfer_tool(90., 0.));
            assert!(!rough.has_errors(), "{:?}", rough.diagnostics);
            assert!(rough.program.steps().len() > baseline.program.steps().len());
            assert!(rough.program.steps().ends_with(baseline.program.steps()));
        }
    }

    #[test]
    fn contour_roughing_rejects_invalid_spacing_and_on_side() {
        let mut op = op(1.);
        op.contour_roughing.enabled = true;
        op.contour_roughing.stepover = 0.;
        let result = run(op.clone(), chamfer_tool(90., 0.));
        assert!(result.has_errors() && result.program.steps().is_empty());
        op.contour_roughing.stepover = 1.;
        op.side = Side::On;
        let result = run(op, chamfer_tool(90., 0.));
        assert!(result.has_errors() && result.program.steps().is_empty());
    }

    #[test]
    fn chamfer_tip_offset_lowers_and_shifts_profile_path_without_changing_width() {
        let baseline = op(1.5);
        let mut offset = baseline.clone();
        offset.tip_offset = 1.;
        let first = run(baseline, chamfer_tool(90., 0.));
        let shifted = run(offset, chamfer_tool(90., 0.));
        assert!(!first.has_errors() && !shifted.has_errors());
        assert!((cut_depth(&first.program).unwrap() + 1.5).abs() < 1e-8);
        assert!((cut_depth(&shifted.program).unwrap() + 2.5).abs() < 1e-8);
        let min_cut_x = |result: &StrategyResult| {
            result
                .program
                .steps()
                .iter()
                .filter_map(|step| match step {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(to.x),
                    Step::Arc { end, tag, .. } if tag.kind == MoveKind::Cutting => Some(end.x),
                    _ => None,
                })
                .fold(f64::INFINITY, f64::min)
        };
        assert!(
            (min_cut_x(&first) - min_cut_x(&shifted) - 1.).abs() < 1e-5,
            "a 1 mm tip offset must move the square's cutting path 1 mm outward"
        );
    }

    #[test]
    fn legacy_chamfer_without_tip_offset_preserves_its_depth() {
        let mut original = op(1.5);
        original.depth = 2.0;
        let mut stored = serde_json::to_value(&original).unwrap();
        stored.as_object_mut().unwrap().remove("tip_offset");
        let restored: ChamferOp = serde_json::from_value(stored).unwrap();
        assert_eq!(restored.tip_offset, 0.);
        let result = run(restored, chamfer_tool(90., 0.));
        assert!(!result.has_errors());
        assert!((cut_depth(&result.program).unwrap() + 2.0).abs() < 1e-8);
    }

    #[test]
    fn tip_offset_requires_a_valid_value_and_an_air_side() {
        let mut o = op(1.0);
        o.tip_offset = 1.0;
        o.side = Side::On;
        assert!(run(o.clone(), chamfer_tool(90., 0.)).has_errors());
        o.side = Side::Outside;
        o.tip_offset = -0.1;
        assert!(run(o, chamfer_tool(90., 0.)).has_errors());
    }

    #[test]
    fn modeled_face_tip_offset_shifts_axis_and_plunge_together() {
        let mut o = op(1.0);
        o.faces = vec![cam_model::ChamferFace {
            upper: [[0., 0.], [20., 0.]],
            lower: [[0., -1.], [20., -1.]],
            top: 0.,
            bottom: -1.,
            outward: [0., -1.],
        }];
        o.depth = 1.;
        let before = run(o.clone(), chamfer_tool(90., 0.));
        o.tip_offset = 0.4;
        let after = run(o, chamfer_tool(90., 0.));
        assert!(!before.has_errors() && !after.has_errors());
        let cut = |result: &StrategyResult| {
            result
                .program
                .steps()
                .iter()
                .find_map(|step| match step {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
                    _ => None,
                })
                .unwrap()
        };
        let (a, b) = (cut(&before), cut(&after));
        assert!((a.z - b.z - 0.4).abs() < 1e-8);
        assert!((a.y - b.y - 0.4).abs() < 1e-8);
        after.program.validate().unwrap();
    }

    #[test]
    fn modeled_face_roughing_offsets_each_band_toward_its_air_side() {
        let mut operation = op(1.);
        operation.side = Side::On; // Each modeled face supplies its own air direction.
        operation.depth = 1.;
        operation.stepdown = 0.5;
        operation.faces = vec![
            cam_model::ChamferFace {
                upper: [[0., 0.], [20., 0.]],
                lower: [[0., -1.], [20., -1.]],
                top: 0.,
                bottom: -1.,
                outward: [0., -1.],
            },
            cam_model::ChamferFace {
                upper: [[30., 20.], [30., 0.]],
                lower: [[31., 20.], [31., 0.]],
                top: 0.,
                bottom: -1.,
                outward: [1., 0.],
            },
        ];
        let baseline = run(operation.clone(), chamfer_tool(90., 0.));
        assert!(!baseline.has_errors(), "{:?}", baseline.diagnostics);
        operation.contour_roughing.enabled = true;
        operation.contour_roughing.stepovers = 2;
        let result = run(operation, chamfer_tool(90., 0.));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        result.program.validate().unwrap();
        assert!(result.program.steps().ends_with(baseline.program.steps()));
        let cuts: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter_map(|step| match step {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
                _ => None,
            })
            .collect();
        assert_eq!(cuts.len(), 12);
        for (stage, extra) in [2., 1., 0.].into_iter().enumerate() {
            let band = &cuts[stage * 4..stage * 4 + 4];
            assert!((band[0].y + 0.5 + extra).abs() < 1e-8);
            assert!((band[1].y + 1.0 + extra).abs() < 1e-8);
            assert!((band[2].x - 30.5 - extra).abs() < 1e-8);
            assert!((band[3].x - 31.0 - extra).abs() < 1e-8);
            assert_eq!(
                band.iter().map(|p| p.z).collect::<Vec<_>>(),
                vec![-0.5, -1., -0.5, -1.]
            );
        }
    }

    #[test]
    fn modeled_face_roughing_enforces_the_combined_band_budget() {
        let mut operation = op(1.);
        operation.depth = 1.;
        operation.stepdown = 0.001;
        operation.faces = vec![cam_model::ChamferFace {
            upper: [[0., 0.], [20., 0.]],
            lower: [[0., -1.], [20., -1.]],
            top: 0.,
            bottom: -1.,
            outward: [0., -1.],
        }];
        operation.contour_roughing.enabled = true;
        operation.contour_roughing.stepovers = 1000;
        let result = run(operation, chamfer_tool(90., 0.));
        assert!(result.has_errors());
        assert!(result.program.is_empty());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resource budget")));
    }

    #[test]
    fn modeled_face_band_approaches_cut_stock_instead_of_colliding() {
        let mut operation = op(10.);
        operation.id = 9;
        operation.top = 20.;
        operation.depth = 10.;
        operation.tip_offset = 1.;
        operation.stepdown = 2.;
        operation.lead_in = Lead::Linear { length: 2. };
        operation.faces = vec![cam_model::ChamferFace {
            upper: [[0., 0.], [20., 0.]],
            lower: [[0., -10.], [20., -10.]],
            top: 20.,
            bottom: 10.,
            outward: [0., -1.],
        }];
        let tools = [chamfer_tool(90., 0.)];
        let env = JobEnv {
            heights: Heights::new(25., 22., 20.),
            tools: &tools,
            stock: Some(([-5., -15.], [25., 5.])),
            spindle: cam_cldata::SpindleDir::Cw,
        };
        let result = ChamferStrategy::new(operation.clone()).compute(&env, &CancelToken::new());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        operation.contour_roughing.enabled = true;
        operation.contour_roughing.stepovers = 2;
        let rough_result = ChamferStrategy::new(operation).compute(&env, &CancelToken::new());
        assert!(!rough_result.has_errors(), "{:?}", rough_result.diagnostics);
        let mut program = result.program;
        program.steps.insert(0, Step::ToolChange { tool: 1 });
        let simulate = |program: &Program| {
            cam_sim::simulate(
                program,
                [-5., -15., 0.],
                [25., 5., 20.],
                &cam_sim::SimOptions {
                    resolution: 0.5,
                    tool_radius: 3.,
                },
                &[cam_sim::SimTool {
                    number: 1,
                    profile: cam_sim::ToolProfile {
                        radius: 3.,
                        shape: cam_sim::ProfileShape::Cone {
                            half_angle_rad: std::f64::consts::FRAC_PI_4,
                            flat_radius: 0.,
                        },
                    },
                    noncutting_tip_radius: None,
                }],
            )
        };
        let first_lead = program
            .steps
            .iter()
            .position(
                |step| matches!(step, Step::Linear { tag, .. } if tag.kind == MoveKind::LeadIn),
            )
            .unwrap();
        let approach = simulate(&Program {
            steps: program.steps[..=first_lead].to_vec(),
        });
        assert!(approach.is_clean(), "{:?}", approach.collisions);
        assert!(
            approach.removed_volume > 0.,
            "the approach must remove stock"
        );
        let simulation = simulate(&program);
        assert!(simulation.is_clean(), "{:?}", simulation.collisions);
        assert!(simulation.removed_volume > 0.);
        let mut rough_program = rough_result.program;
        rough_program.steps.insert(0, Step::ToolChange { tool: 1 });
        let rough_simulation = simulate(&rough_program);
        assert!(
            rough_simulation.is_clean(),
            "{:?}",
            rough_simulation.collisions
        );
        assert!(rough_simulation.removed_volume > 0.);
    }

    #[test]
    fn open_corner_offsets_both_sides_and_retracts_between_width_passes() {
        for (side, expected) in [
            (Side::Outside, [(0., 0.5), (9.5, 0.5), (9.5, 10.)]),
            (Side::Inside, [(0., -0.5), (10.5, -0.5), (10.5, 10.)]),
        ] {
            let mut operation = op(1.);
            operation.closed = false;
            operation.chain = Contour::new(vec![
                Point::new(0., 0.),
                Point::new(10., 0.),
                Point::new(10., 10.),
            ]);
            operation.side = side;
            operation.step = 0.5;
            let result = run(operation, chamfer_tool(90., 1.));
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let cut_points: Vec<_> = result
                .program
                .steps()
                .iter()
                .filter_map(|s| match s {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
                    _ => None,
                })
                .collect();
            assert_eq!(cut_points.len(), 4, "no closing cut");
            for (pass, points) in cut_points.chunks(2).enumerate() {
                for (actual, expected) in points.iter().zip(&expected[1..]) {
                    assert!(
                        (actual.x - expected.0).abs() < 1e-9
                            && (actual.y - expected.1).abs() < 1e-9
                    );
                    assert!((actual.z + 0.5 * (pass + 1) as f64).abs() < 1e-9);
                }
            }
            assert_eq!(
                result
                    .program
                    .steps()
                    .iter()
                    .filter(
                        |s| matches!(s, Step::Rapid { tag, .. } if tag.kind == MoveKind::Retract)
                    )
                    .count(),
                2
            );
        }
    }

    fn radius_tool() -> Tool {
        Tool {
            number: 1,
            diameter: 12.0,
            length: 30.0,
            flute_length: 12.0,
            shank_diameter: 24.0,
            flutes: 4,
            kind: ToolKind::RadiusMill {
                corner_radius: 6.0,
                tip_length: 2.0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn radius_mill_rounds_open_edge_with_correct_offsets_and_passes() {
        for (side, y) in [(Side::Outside, 6.0), (Side::Inside, -6.0)] {
            let mut operation = op(6.0);
            operation.closed = false;
            operation.chain = Contour::new(vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)]);
            operation.side = side;
            operation.step = 3.0;
            let result = run(operation, radius_tool());
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let cuts: Vec<_> = result
                .program
                .steps()
                .iter()
                .filter_map(|step| match step {
                    Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some(*to),
                    _ => None,
                })
                .collect();
            assert_eq!(cuts.len(), 2);
            assert!((cuts[0].y - y).abs() < 1e-9 && (cuts[1].y - y).abs() < 1e-9);
            assert!((cuts[0].z + 5.0).abs() < 1e-9);
            assert!((cuts[1].z + 8.0).abs() < 1e-9);
        }
    }

    #[test]
    fn radius_mill_rejects_unsupported_settings_and_invalid_geometry() {
        let mut operation = op(6.0);
        operation.side = Side::On;
        assert!(run(operation.clone(), radius_tool()).has_errors());
        operation.side = Side::Outside;
        operation.tip_offset = 1.0;
        assert!(run(operation.clone(), radius_tool()).has_errors());
        operation.tip_offset = 0.0;
        let mut tool = radius_tool();
        tool.flute_length = 7.0;
        assert!(run(operation, tool).has_errors());
    }

    #[test]
    fn open_radius_mill_exit_is_tagged_as_lead_out() {
        let mut operation = op(6.0);
        operation.closed = false;
        operation.chain = Contour::new(vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)]);
        operation.lead_out = Lead::Linear { length: 1.0 };
        let result = run(operation, radius_tool());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(result.program.steps().iter().any(|step| matches!(step,
            Step::Linear { tag, .. } if tag.kind == MoveKind::LeadOut)));
        assert!(!result.program.steps().iter().any(|step| matches!(step,
            Step::Linear { tag, .. } if tag.kind == MoveKind::LeadIn)));
    }

    #[test]
    fn radius_mill_rejects_modeled_bevel_faces() {
        let mut operation = op(6.0);
        operation.faces = vec![cam_model::ChamferFace {
            upper: [[0.0, 0.0], [20.0, 0.0]],
            lower: [[0.0, -1.0], [20.0, -1.0]],
            top: 0.0,
            bottom: -1.0,
            outward: [0.0, -1.0],
        }];
        let result = run(operation, radius_tool());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("edge chain")));
    }

    #[test]
    fn radius_mill_uses_tool_radius_even_when_chamfer_width_differs() {
        let result = run(op(1.0), radius_tool());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!((cut_depth(&result.program).unwrap() + 8.0).abs() < 1e-9);
        assert!(!result.diagnostics.is_empty());
    }

    /// A 90° mill (α=45°, tan=1) chamfers to a depth equal to the width.
    #[test]
    fn ninety_degree_depth_equals_width() {
        let r = run(op(1.5), chamfer_tool(90.0, 0.0));
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        let z = cut_depth(&r.program).expect("a plunge to depth");
        assert!(
            (z - (-1.5)).abs() < 1e-9,
            "90° chamfer of 1.5 → depth −1.5, got {z}"
        );
    }

    /// A 60° mill (α=30°, tan≈0.577) cuts deeper than its width for the same bevel.
    #[test]
    fn sharper_tool_cuts_deeper_for_same_width() {
        let r = run(op(1.0), chamfer_tool(60.0, 0.0));
        let z = cut_depth(&r.program).expect("a plunge to depth");
        let expected = -1.0 / (30.0_f64.to_radians().tan());
        assert!(
            (z - expected).abs() < 1e-9,
            "60° chamfer depth {z} vs {expected}"
        );
    }

    #[test]
    fn non_chamfer_tool_errors() {
        let end_mill = Tool {
            number: 1,
            diameter: 6.0,
            length: 30.0,
            flutes: 2,
            kind: ToolKind::EndMill,
            ..Default::default()
        };
        let r = run(op(1.0), end_mill);
        assert!(r.has_errors());
        assert!(r.program.is_empty());
    }

    #[test]
    fn nonpositive_width_errors() {
        let r = run(op(0.0), chamfer_tool(90.0, 0.0));
        assert!(r.has_errors());
    }

    /// The retract lifts from the start with no overlap, and 2 mm past it (along
    /// the +X first edge) with a 2 mm closure overlap. A sharp V (tip ⌀0) cuts the
    /// chain itself, so the start is (0,0) and the overlap point is exactly (2,0).
    #[test]
    fn overlap_retracts_past_the_start() {
        let retract_xy = |op: ChamferOp| -> Point3 {
            let r = run(op, chamfer_tool(90.0, 0.0));
            assert!(!r.has_errors(), "{:?}", r.diagnostics);
            r.program
                .steps()
                .iter()
                .find_map(|s| match s {
                    Step::Rapid { to, tag } if tag.kind == MoveKind::Retract => Some(*to),
                    _ => None,
                })
                .expect("a retract")
        };

        let at_zero = retract_xy(op(1.5));
        assert!(
            at_zero.x.abs() < 1e-9 && at_zero.y.abs() < 1e-9,
            "closes at the start"
        );

        let mut with_overlap = op(1.5);
        with_overlap.lead_overlap = 2.0;
        let past = retract_xy(with_overlap);
        assert!(
            (past.x - 2.0).abs() < 1e-9 && past.y.abs() < 1e-9,
            "should retract 2 mm past the start, got ({}, {})",
            past.x,
            past.y
        );
    }

    /// Arc leads ease the tool onto and off the edge: the plunge moves off the
    /// contour to the lead-in entry, a lead-in arc curves onto the start, and a
    /// lead-out arc curves back off. Sharp V (tip ⌀0) cuts the chain, so the
    /// CCW square starts at (0,0) with tangent +X and outward normal −Y; a radius-2
    /// tangent-arc lead-in therefore enters at (−2,−2).
    #[test]
    fn arc_leads_ease_on_and_off_the_edge() {
        let r = 2.0;
        let mut o = op(1.5);
        o.lead_in = Lead::Arc { radius: r };
        o.lead_out = Lead::Arc { radius: r };
        let result = run(o, chamfer_tool(90.0, 0.0));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let steps = result.program.steps();

        // The plunge lands off the contour, at the lead-in entry (−2, −2).
        let plunge = steps
            .iter()
            .find_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(*to),
                _ => None,
            })
            .expect("a plunge");
        assert!(
            (plunge.x + r).abs() < 1e-9 && (plunge.y + r).abs() < 1e-9,
            "plunge should sit at the lead-in entry, got ({}, {})",
            plunge.x,
            plunge.y
        );

        // One lead-in arc and one lead-out arc, both tagged as leads.
        let lead_arcs: Vec<Point3> = steps
            .iter()
            .filter_map(|s| match s {
                Step::Arc { end, tag, .. }
                    if matches!(tag.kind, MoveKind::LeadIn | MoveKind::LeadOut) =>
                {
                    Some(*end)
                }
                _ => None,
            })
            .collect();
        assert_eq!(lead_arcs.len(), 2, "a lead-in and a lead-out arc");
        // The lead-in arc lands on the edge start (0,0); cutting begins there.
        assert!(
            lead_arcs[0].x.abs() < 1e-9 && lead_arcs[0].y.abs() < 1e-9,
            "lead-in should end at the edge start, got ({}, {})",
            lead_arcs[0].x,
            lead_arcs[0].y
        );
    }

    #[test]
    fn chamfer_exit_can_copy_entry_or_be_disabled() {
        let mut o = op(1.5);
        o.lead_in = Lead::Arc { radius: 1.0 };
        o.lead_out = Lead::None;
        o.same_lead_out = true;
        let enabled = run(o.clone(), chamfer_tool(90.0, 0.0));
        assert!(!enabled.has_errors(), "{:?}", enabled.diagnostics);
        assert!(enabled
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Arc { tag, .. } if tag.kind == MoveKind::LeadOut)));
        o.lead_out_enabled = Some(false);
        let disabled = run(o, chamfer_tool(90.0, 0.0));
        assert!(!disabled.has_errors(), "{:?}", disabled.diagnostics);
        assert!(!disabled.program.steps().iter().any(|step| matches!(step, Step::Arc { tag, .. } | Step::Linear { tag, .. } if tag.kind == MoveKind::LeadOut)));
    }

    #[test]
    fn chamfer_entry_can_be_disabled_without_disabling_independent_exit() {
        let mut o = op(1.5);
        o.lead_in = Lead::Arc { radius: 0.6 };
        o.lead_out = Lead::Arc { radius: 1.0 };
        o.same_lead_out = true;
        o.lead_in_enabled = Some(false);
        o.lead_out_enabled = Some(true);
        let result = run(o, chamfer_tool(90.0, 0.0));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!result.program.steps().iter().any(|step| matches!(step, Step::Arc { tag, .. } | Step::Linear { tag, .. } if tag.kind == MoveKind::LeadIn)));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Arc { tag, .. } if tag.kind == MoveKind::LeadOut)));
    }

    #[test]
    fn modeled_face_leads_use_requested_arcs_and_safe_air_approach() {
        let mut o = op(1.0);
        o.faces = vec![cam_model::ChamferFace {
            upper: [[0., 0.], [20., 0.]],
            lower: [[0., -1.], [20., -1.]],
            top: 0.,
            bottom: -1.,
            outward: [0., -1.],
        }];
        o.depth = 1.;
        o.lead_in = Lead::ArcSweep {
            radius: 0.6,
            angle_deg: 60.,
        };
        o.lead_out = Lead::Arc { radius: 1.2 };
        o.same_lead_out = true;
        let result = run(o.clone(), chamfer_tool(90.0, 0.0));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        result.program.validate().unwrap();
        let arcs: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter_map(|s| match s {
                Step::Arc {
                    end, center, tag, ..
                } if matches!(tag.kind, MoveKind::LeadIn | MoveKind::LeadOut) => {
                    Some((*end, *center, tag.kind))
                }
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        assert_eq!(arcs[0].2, MoveKind::LeadIn);
        assert_eq!(arcs[1].2, MoveKind::LeadOut);
        for (end, center, _) in arcs {
            assert!(((end.x - center.x).hypot(end.y - center.y) - 0.6).abs() < 1e-8);
        }
        let plunge = result
            .program
            .steps()
            .iter()
            .find_map(|step| match step {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(*to),
                _ => None,
            })
            .unwrap();
        assert!(
            plunge.y < -1.0 - 3.0,
            "plunge must stay beyond the face and tool radius"
        );

        let tools = [chamfer_tool(90., 0.)];
        let env = JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &tools,
            stock: Some(([-5., -10.], [25., 5.])),
            spindle: cam_cldata::SpindleDir::Cw,
        };
        let stock_result = ChamferStrategy::new(o.clone()).compute(&env, &CancelToken::new());
        assert!(!stock_result.has_errors(), "{:?}", stock_result.diagnostics);
        stock_result.program.validate().unwrap();
        let stock_plunge = stock_result
            .program
            .steps()
            .iter()
            .find_map(|step| match step {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(*to),
                _ => None,
            })
            .unwrap();
        assert!(
            stock_plunge.y <= -13.5,
            "plunge must clear the stock by the tool radius and margin"
        );

        o.lead_in_enabled = Some(false);
        o.lead_out_enabled = Some(true);
        let result = run(o, chamfer_tool(90.0, 0.0));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        // Disabling the optional arc still leaves the required straight feed
        // from the safe approach point to the face.
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s, Step::Arc { tag, .. } if tag.kind == MoveKind::LeadIn)));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::LeadIn)));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s, Step::Arc { tag, .. } if tag.kind == MoveKind::LeadOut)));
    }

    #[test]
    fn internal_chamfer_leads_ease_in_from_the_bore_not_the_wall() {
        // An internal chamfer on a ⌀20 bore (the [0,20] edge) with a sharp V cuts the
        // edge itself. Starting mid-edge (top, x=10), the arc lead must ease in from
        // the bore interior (nearer the centre 10,10), not swing out past the wall.
        let mut o = op(1.5);
        o.side = Side::Inside;
        o.start = Some([10.0, 20.0]);
        o.lead_in = Lead::Arc { radius: 2.0 };
        o.lead_out = Lead::Arc { radius: 2.0 };
        let r = run(o, chamfer_tool(90.0, 0.0));
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        // The plunge lands at the lead-in entry; it must sit inside the bore [0,20]
        // and on the interior side of the (10,20) edge point (y < 20, toward centre).
        let plunge = r
            .program
            .steps()
            .iter()
            .find_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(*to),
                _ => None,
            })
            .expect("a plunge at the lead entry");
        assert!(
            plunge.y < 20.0 - 1e-6 && (0.0..=20.0).contains(&plunge.x) && plunge.y >= 0.0,
            "internal chamfer lead must enter from the bore, got ({}, {})",
            plunge.x,
            plunge.y
        );
        let leads = r
            .program
            .steps()
            .iter()
            .filter(|s| matches!(s, Step::Arc { tag, .. } if matches!(tag.kind, MoveKind::LeadIn | MoveKind::LeadOut)))
            .count();
        assert_eq!(leads, 2, "a lead-in and a lead-out on the bore edge");
    }

    fn plunge_count(r: &StrategyResult) -> usize {
        r.program
            .steps()
            .iter()
            .filter(|s| matches!(s, Step::Linear { tag, .. } if tag.kind == MoveKind::Plunge))
            .count()
    }

    #[test]
    fn pass_widths_uniform_and_gradual() {
        assert_eq!(
            pass_widths(3.0, 0.0, false),
            vec![3.0],
            "no step ⇒ one pass"
        );
        assert_eq!(
            pass_widths(3.0, 5.0, false),
            vec![3.0],
            "step ≥ width ⇒ one pass"
        );
        assert_eq!(
            pass_widths(3.0, 1.0, false),
            vec![1.0, 2.0, 3.0],
            "uniform steps"
        );

        // Gradual, equal material per pass: widths step·√k, ending on the target.
        // width 3, step 1 ⇒ √1..√9, i.e. exactly 9 passes (√9 = 3).
        let g = pass_widths(3.0, 1.0, true);
        assert_eq!(g.len(), 9, "equal-area passes: {g:?}");
        assert!(
            (g[1] - std::f64::consts::SQRT_2).abs() < 1e-9,
            "2nd pass at √2"
        );
        assert!(
            (g.last().unwrap() - 3.0).abs() < 1e-9,
            "ends exactly at width"
        );
    }

    #[test]
    fn deeper_depth_plunges_the_tip_lower() {
        // 90° tool (tan α = 1): the tip's natural depth for a 1.5 mm bevel is 1.5.
        // Setting depth 2.0 plunges the tip 0.5 mm deeper to use a higher flank.
        let base = cut_depth(&run(op(1.5), chamfer_tool(90.0, 0.0)).program).unwrap();
        assert!((base + 1.5).abs() < 1e-9, "tip at bevel bottom by default");

        let mut deep = op(1.5);
        deep.depth = 2.0;
        let z = cut_depth(&run(deep, chamfer_tool(90.0, 0.0)).program).unwrap();
        assert!((z + 2.0).abs() < 1e-9, "tip rides 0.5 mm deeper, got {z}");
    }

    #[test]
    fn depth_below_the_tip_minimum_falls_back_to_the_tip() {
        // depth under width/tanα can't cut the full bevel; clamp to the tip + warn.
        let mut shallow = op(1.5);
        shallow.depth = 0.5;
        let r = run(shallow, chamfer_tool(90.0, 0.0));
        assert!(
            (cut_depth(&r.program).unwrap() + 1.5).abs() < 1e-9,
            "clamped to the tip"
        );
        assert!(!r.diagnostics.is_empty(), "warns about the under-set depth");
    }

    #[test]
    fn stepping_cuts_one_loop_per_pass() {
        // width 3, step 1, uniform ⇒ 3 passes (one plunge each); the last reaches −3.
        let mut o = op(3.0);
        o.step = 1.0;
        let r = run(o, chamfer_tool(90.0, 0.0));
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        assert_eq!(plunge_count(&r), 3, "one plunge per pass");
        // Passes go shallow→deep: first at −1, last at −3.
        let plunge_zs: Vec<f64> = r
            .program
            .steps()
            .iter()
            .filter_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Plunge => Some(to.z),
                _ => None,
            })
            .collect();
        assert!(
            (plunge_zs[0] + 1.0).abs() < 1e-9,
            "first pass shallow at −1"
        );
        assert!(
            (plunge_zs[2] + 3.0).abs() < 1e-9,
            "final pass at the full depth −3"
        );
    }

    #[test]
    fn gradual_makes_more_but_gentler_passes() {
        let mut o = op(3.0);
        o.step = 1.0;
        o.gradual = true;
        let r = run(o, chamfer_tool(90.0, 0.0));
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
        assert_eq!(
            plunge_count(&r),
            9,
            "equal-area gradual ⇒ 9 passes for 3 mm @ step 1"
        );
    }

    fn xy_of_first_cut(prog: &Program) -> [f64; 2] {
        prog.steps()
            .iter()
            .find_map(|s| match s {
                Step::Linear { to, tag, .. } if tag.kind == MoveKind::Cutting => Some([to.x, to.y]),
                Step::Arc { end, tag, .. } if tag.kind == MoveKind::Cutting => Some([end.x, end.y]),
                _ => None,
            })
            .expect("a cutting move")
    }

    #[test]
    fn a_vbit_is_accepted_for_chamfering() {
        let r = run(op(1.0), vbit_tool(90.0, 0.0));
        assert!(
            r.diagnostics
                .iter()
                .all(|d| d.severity != crate::Severity::Error),
            "{:?}",
            r.diagnostics
        );
        assert!(!r.program.steps().is_empty());
    }

    #[test]
    fn a_sharp_vbit_matches_a_sharp_chamfer_mill_exactly() {
        // Same flank, no tip on either → byte-identical geometry.
        let a = run(op(1.0), vbit_tool(90.0, 0.0));
        let b = run(op(1.0), chamfer_tool(90.0, 0.0));
        assert_eq!(a.program.steps(), b.program.steps());
    }

    #[test]
    fn a_rounded_tip_is_equivalent_to_a_smaller_flat_not_its_own_radius() {
        // The bug this guards: treating the V-bit's tip radius as if it were a flat
        // radius would offset the tool too far from the edge, cutting a narrow bevel.
        // The correct equivalent flat is rt·(1 − sin α)/cos α, which is strictly
        // smaller (the ball tucks under the flank).
        let (deg, rt) = (90.0_f64, 0.5_f64);
        let a = 0.5 * deg.to_radians();
        let equiv = rt * (1.0 - a.sin()) / a.cos();
        assert!(equiv < rt, "equiv {equiv} should tuck under rt {rt}");

        let got = xy_of_first_cut(&run(op(1.0), vbit_tool(deg, rt)).program);
        let want = xy_of_first_cut(&run(op(1.0), chamfer_tool(deg, 2.0 * equiv)).program);
        let wrong = xy_of_first_cut(&run(op(1.0), chamfer_tool(deg, 2.0 * rt)).program);
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "got {got:?} want {want:?}"
        );
        assert!(
            (got[0] - wrong[0]).abs() > 1e-6 || (got[1] - wrong[1]).abs() > 1e-6,
            "the naive tip-radius-as-flat reading must differ"
        );
    }

    #[test]
    fn a_flat_end_mill_is_still_rejected() {
        let mut t = chamfer_tool(90.0, 0.0);
        t.kind = ToolKind::EndMill;
        let r = run(op(1.0), t);
        let errs: Vec<_> = r
            .diagnostics
            .iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .collect();
        assert_eq!(errs.len(), 1, "{:?}", r.diagnostics);
        assert!(errs[0]
            .message
            .contains("chamfer mill, V-bit, or radius mill"));
    }
}
