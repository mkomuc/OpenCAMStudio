//! Scheduling adapters for the existing profile emitter; no second cutting engine.
use super::*;
use cam_model::{ProfileOrder, ProfilePasses};

pub(super) fn compute(op: &ProfileOp, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
    let p = &op.passes;
    let advanced = p.finishing
        || p.floor_allowance != 0.
        || p.even_stepdowns
        || p.finishing_depths > 0
        || p.order != ProfileOrder::Contour
        || !p.preserve_order
        || p.taper_angle != 0.;
    let mut effective = op.clone();
    if p.same_lead_out {
        effective.lead_out = effective.lead_in;
    }
    let mut result = if advanced {
        scheduled(&effective, env, cancel)
    } else {
        ProfileStrategy::new(effective).compute_base(env, cancel)
    };
    if cancel.is_cancelled() || result.cancelled {
        return StrategyResult {
            cancelled: true,
            diagnostics: result.diagnostics,
            ..Default::default()
        };
    }
    if p.ramp_rpm > 0. && !result.has_errors() {
        let mut program = Program::new();
        let mut ramping = false;
        for step in result.program.steps() {
            if cancel.is_cancelled() {
                return StrategyResult {
                    cancelled: true,
                    diagnostics: result.diagnostics,
                    ..Default::default()
                };
            }
            let entry = matches!(step, Step::Linear { tag, .. } | Step::Arc { tag, .. } if tag.kind == MoveKind::Plunge);
            if entry != ramping {
                program.push(Step::Spindle {
                    rpm: if entry { p.ramp_rpm } else { op.spindle_rpm },
                    dir: env.spindle,
                });
                ramping = entry;
            }
            program.push(step.clone());
        }
        if ramping {
            program.push(Step::Spindle {
                rpm: op.spindle_rpm,
                dir: env.spindle,
            });
        }
        result.program = program;
    }
    if p.feed_retract {
        let mut program = Program::new();
        for step in result.program.steps() {
            if cancel.is_cancelled() {
                return StrategyResult {
                    cancelled: true,
                    diagnostics: result.diagnostics,
                    ..Default::default()
                };
            }
            program.push(match step {
                Step::Rapid { to, tag } if tag.kind == MoveKind::Retract => Step::Linear {
                    to: *to,
                    tag: *tag,
                    feed: op.entry_feeds.lead_out(op.feed),
                },
                step => step.clone(),
            });
        }
        result.program = program;
    }
    if result.program.steps().len() > cam_model::MAX_PROFILE_MOVES {
        result.program = Program::new();
        result
            .diagnostics
            .push(Diagnostic::error("Profile motion budget exceeded"));
    }
    if let Err(error) = result.program.validate() {
        result.program = Program::new();
        result.diagnostics.push(Diagnostic::error(format!(
            "Invalid generated profile motion: {error}"
        )));
    }
    result
}

fn scheduled(op: &ProfileOp, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
    let p = &op.passes;
    let top = env.heights.top_of_stock;
    let bottom = -op.depth + p.floor_allowance;
    let rough_bottom = bottom + p.finishing_depths as f64 * p.finishing_stepdown;
    let step = if p.even_stepdowns {
        (top - rough_bottom) / ((top - rough_bottom) / op.stepdown).ceil()
    } else {
        op.stepdown
    };
    let rough_step = if p.even_stepdowns {
        (top - bottom) / ((top - bottom) / op.stepdown).ceil()
    } else {
        op.stepdown
    };
    let rough_levels = depth_levels(top, bottom, rough_step);
    let mut levels = depth_levels(top, rough_bottom, step);
    levels.extend(
        (1..=p.finishing_depths)
            .map(|i| (rough_bottom - i as f64 * p.finishing_stepdown).max(bottom)),
    );
    let count = if p.finishing { p.finish_count } else { 1 } as usize;
    let repeats = if p.finishing {
        p.spring_passes as usize
    } else {
        0
    };
    let mut contours: Vec<_> = op.contours().enumerate().collect();
    if !p.preserve_order {
        let mut from = op.start.unwrap_or([0., 0.]);
        let mut ordered = Vec::new();
        while !contours.is_empty() {
            let nearest = contours
                .iter()
                .enumerate()
                .min_by(|(_, (_, a)), (_, (_, b))| {
                    let distance = |c: &cam_model::ProfileContour| {
                        c.chain
                            .points()
                            .iter()
                            .map(|q| (q.x - from[0]).powi(2) + (q.y - from[1]).powi(2))
                            .fold(f64::INFINITY, f64::min)
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(index, _)| index)
                .unwrap();
            let c = contours.remove(nearest);
            let q = c.1.chain.points()[0];
            from = [q.x, q.y];
            ordered.push(c);
        }
        contours = ordered;
    }
    let estimated = levels
        .len()
        .max(rough_levels.len())
        .saturating_mul(count + repeats)
        .saturating_mul(contours.len());
    if estimated > cam_model::MAX_PROFILE_LEVELS {
        return StrategyResult {
            diagnostics: vec![Diagnostic::error(
                "Too many scheduled profile passes; reduce finish/depth counts",
            )],
            ..Default::default()
        };
    }
    // Each radial stage selects its depth schedule; all orders share these jobs.
    // Without Rough final, intermediate cuts use the regular maximum stepdown.
    let mut jobs = Vec::with_capacity(estimated);
    for (order_index, _) in contours.iter().enumerate() {
        for radial in 0..count + repeats {
            let depths = if !p.rough_final && radial + 1 < count {
                &rough_levels
            } else {
                &levels
            };
            for (level, &z) in depths.iter().enumerate() {
                let final_depth = level + 1 == depths.len();
                if p.finish_only_final && !final_depth && (op.stepover > 0. || radial > 0) {
                    continue;
                }
                let previous = if level == 0 || p.finish_only_final {
                    top
                } else {
                    depths[level - 1]
                };
                jobs.push((order_index, z, radial, previous, final_depth, level));
            }
        }
    }
    jobs.sort_by(|a, b| {
        let contour = a.0.cmp(&b.0);
        let depth = b.1.total_cmp(&a.1);
        let radial = a.2.cmp(&b.2);
        match p.order {
            ProfileOrder::Contour => contour.then(depth).then(radial),
            ProfileOrder::Depth => depth.then(contour).then(radial),
            ProfileOrder::Radial => radial.then(contour).then(depth),
        }
    });
    let mut result = StrategyResult::default();
    // Rough first at the outermost finishing offset; explicit finishing follows.
    if op.stepover > 0. {
        let mut previous = top;
        for &z in if p.rough_final {
            &levels
        } else {
            &rough_levels
        } {
            if cancel.is_cancelled() {
                return StrategyResult {
                    cancelled: true,
                    ..Default::default()
                };
            }
            let mut rough = op.clone();
            rough.depth = -z;
            rough.stepdown = previous - z;
            rough.offset += (count.saturating_sub(1)) as f64 * p.finish_stepover;
            rough.passes = ProfilePasses {
                stock_boundary: p.stock_boundary.clone(),
                ..Default::default()
            };
            let rough_env = JobEnv {
                heights: Heights::new(env.heights.clearance, env.heights.retract, previous),
                ..*env
            };
            if !append(
                &mut result,
                ProfileStrategy::new(rough).compute_base(&rough_env, cancel),
            ) {
                return result;
            }
            previous = z;
        }
        if !p.finishing {
            return result;
        }
    }
    for (contour, z, radial, previous, final_depth, level) in jobs {
        if cancel.is_cancelled() {
            return StrategyResult {
                cancelled: true,
                diagnostics: result.diagnostics,
                ..Default::default()
            };
        }
        let (source_index, chain) = &contours[contour];
        let mut pass = op.clone();
        pass.chain = chain.chain.clone();
        pass.closed = chain.closed;
        pass.side = chain.side;
        pass.additional_chains.clear();
        pass.selections = None;
        pass.clearing.climb ^= op.contour_reversed(*source_index);
        if p.both_ways && !pass.closed && level % 2 == 1 {
            pass.clearing.climb = !pass.clearing.climb;
        }
        pass.offset += (count - 1 - radial.min(count - 1)) as f64 * p.finish_stepover
            + (z + op.depth) * p.taper_angle.to_radians().tan();
        pass.depth = -z;
        pass.stepdown = previous - z;
        pass.stepover = 0.;
        if p.finishing && p.finish_feed > 0. && (!p.finish_only_final || final_depth) {
            pass.feed = p.finish_feed;
        }
        // Keep only emission parameters; scheduling owns depth/offset stages.
        pass.passes = ProfilePasses {
            outer_corner: p.outer_corner,
            tolerance: p.tolerance,
            start_extension: p.start_extension,
            end_extension: p.end_extension,
            lead_end_distance: p.lead_end_distance,
            ..Default::default()
        };
        let pass_env = JobEnv {
            heights: Heights::new(env.heights.clearance, env.heights.retract, previous),
            ..*env
        };
        if !append(
            &mut result,
            ProfileStrategy::new(pass).compute_base(&pass_env, cancel),
        ) {
            return result;
        }
        if result.program.steps().len() > cam_model::MAX_PROFILE_MOVES {
            result.program = Program::new();
            result
                .diagnostics
                .push(Diagnostic::error("Profile motion budget exceeded"));
            return result;
        }
    }
    result
}

fn append(result: &mut StrategyResult, part: StrategyResult) -> bool {
    let over_budget = result
        .program
        .steps()
        .len()
        .saturating_add(part.program.steps().len())
        > cam_model::MAX_PROFILE_MOVES;
    let failed = part.has_errors() || part.cancelled || over_budget;
    if over_budget {
        result
            .diagnostics
            .push(Diagnostic::error("Profile motion budget exceeded"));
    }
    // A warning shared by every finishing level needs only one inspector notice.
    for diagnostic in part.diagnostics {
        if !result.diagnostics.contains(&diagnostic) {
            result.diagnostics.push(diagnostic);
        }
    }
    if failed {
        result.program = Program::new();
        result.cancelled = part.cancelled;
        false
    } else {
        result.program.extend(part.program);
        true
    }
}
