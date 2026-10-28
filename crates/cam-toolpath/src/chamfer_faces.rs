//! Finish a modeled bevel with successive bands fitting the cutter's actual flank.
use super::*;
use cam_geo::{Point, SegShape};
use cam_model::Tool;

pub(super) fn compute(
    op: &ChamferOp,
    env: &JobEnv,
    tool: &Tool,
    included_angle: f64,
    cancel: &CancelToken,
) -> StrategyResult {
    let mut diagnostics = Vec::new();
    let error = |reason: String| StrategyResult {
        diagnostics: vec![Diagnostic::error(format!("Operation {}: {reason}", op.id))],
        ..Default::default()
    };
    let alpha = (included_angle * 0.5).to_radians();
    let tan = alpha.tan();
    if !alpha.is_finite()
        || !(0. < alpha && alpha < std::f64::consts::FRAC_PI_2)
        || !op.stepdown.is_finite()
        || op.stepdown < 0.
        || !op.top.is_finite()
        || !op.depth.is_finite()
        || op.depth <= 0.
    {
        return error(
            "Enter a valid cutter angle, positive cutting depth and nonnegative stepdown".into(),
        );
    }
    // Use the actual straight cutting segment, excluding a rounded V-bit tip and
    // the non-cutting shaft. Overall stickout alone cannot qualify a face band.
    let profile = tool.profile();
    let mut previous = profile.start;
    let mut flank = None;
    for segment in &profile.segs {
        let dz = segment.end.y - previous.y;
        if segment.cutting
            && matches!(segment.shape, SegShape::Line)
            && dz > 1e-8
            && ((segment.end.x - previous.x) / dz - tan).abs() < 1e-6
        {
            flank = Some((previous.y, segment.end.y, previous.x));
            break;
        }
        previous = segment.end;
    }
    let Some((low, high, radius_at_contact)) = flank else {
        return error("The selected tool has no usable conical cutting flank".into());
    };
    let available = high - low;
    if available <= 1e-6 {
        return error("The selected cutter has no usable chamfer band height".into());
    }
    let step = if op.stepdown > 0. {
        op.stepdown.min(available)
    } else {
        available
    };
    let bottom = op.top - op.depth;
    let count = (op.depth / step).ceil() as usize;
    if count == 0 || count > 10_000 || op.faces.len().saturating_mul(count) > 100_000 {
        return error(
            "Chamfer stepdown exceeds the pass resource budget; use a larger stepdown".into(),
        );
    }
    for face in &op.faces {
        if let Err(reason) = face.validate() {
            return error(reason);
        }
        let modeled_angle = 2.
            * (face.width() / (face.top - face.bottom))
                .atan()
                .to_degrees();
        if (modeled_angle - included_angle).abs() > 1e-3 {
            diagnostics.push(Diagnostic::warning(format!("Operation {}: modeled chamfer matches a {modeled_angle:.3}° included-angle cutter; the selected cutter is {included_angle:.3}°. The cut will follow the selected cutter's profile.", op.id)).for_op(op.id));
        }
        if op.top > face.top + 1e-5
            || bottom < face.bottom - 1e-5
            || op.top <= face.bottom
            || bottom >= face.top
        {
            return error("Cutting Top and Bottom must stay between the modeled chamfer's upper and lower boundaries".into());
        }
    }
    let mut program = Program::new();
    program.push(Step::Comment(format!(
        "Modeled chamfer: {} faces; {} per face; maximum stepdown {:.3} mm",
        op.faces.len(),
        crate::passes_phrase(count),
        step
    )));
    for (face_index, face) in op.faces.iter().enumerate() {
        for pass in 1..=count {
            if cancel.is_cancelled() {
                return StrategyResult {
                    program,
                    diagnostics,
                    cancelled: true,
                    ..Default::default()
                };
            }
            let band_bottom = if pass == count {
                bottom
            } else {
                op.top - step * pass as f64
            };
            let points = face.at_height(band_bottom);
            // The lowest straight-flank point touches the lower edge of this band.
            // The cutter's tip can sit slightly lower when it has a rounded nose.
            let z = band_bottom - low;
            let axis = |p: [f64; 2]| {
                Point::new(
                    p[0] + face.outward[0] * radius_at_contact,
                    p[1] + face.outward[1] * radius_at_contact,
                )
            };
            let start = axis(points[0]);
            let end = axis(points[1]);
            // Enter beyond the face's lower boundary and feed inward. A flat,
            // non-cutting chamfer tip must not plunge vertically into raw stock.
            let mut approach = (band_bottom - face.bottom) * tan + tool.radius() + 0.5;
            if let Some((min, max)) = env.stock {
                let stock_limit: f64 = (0..2).map(|axis| if face.outward[axis] >= 0. { max[axis] } else { min[axis] } * face.outward[axis]).sum();
                for point in [start, end] {
                    approach = approach.max(
                        stock_limit + tool.radius() + 0.5
                            - point.x * face.outward[0]
                            - point.y * face.outward[1],
                    );
                }
            }
            let air = |point: Point| {
                Point::new(
                    point.x + face.outward[0] * approach,
                    point.y + face.outward[1] * approach,
                )
            };
            let entry = air(start);
            let exit = air(end);
            program.push(Step::Comment(format!(
                "Chamfer face {} band {}: Z{:.6}",
                face_index + 1,
                pass,
                band_bottom
            )));
            program.push(Step::Rapid {
                to: Point3::new(entry.x, entry.y, env.heights.clearance),
                tag: Tag::new(op.id, MoveKind::Link),
            });
            crate::emit::descend_to(&mut program, entry, op.top, &env.heights, op.feed, op.id);
            program.push(Step::Linear {
                to: Point3::new(entry.x, entry.y, z),
                feed: op.plunge_feed,
                tag: Tag::new(op.id, MoveKind::Plunge),
            });
            program.push(Step::Linear {
                to: Point3::new(start.x, start.y, z),
                feed: op.entry_feeds.lead_in(op.feed),
                tag: Tag::new(op.id, MoveKind::LeadIn),
            });
            program.push(Step::Linear {
                to: Point3::new(end.x, end.y, z),
                feed: op.feed,
                tag: Tag::new(op.id, MoveKind::Cutting),
            });
            program.push(Step::Linear {
                to: Point3::new(exit.x, exit.y, z),
                feed: op.entry_feeds.lead_out(op.feed),
                tag: Tag::new(op.id, MoveKind::LeadIn),
            });
            program.push(Step::Rapid {
                to: Point3::new(exit.x, exit.y, env.heights.clearance),
                tag: Tag::new(op.id, MoveKind::Retract),
            });
        }
    }
    StrategyResult {
        program,
        diagnostics,
        ..Default::default()
    }
}
