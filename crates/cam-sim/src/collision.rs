//! Locate first stock contact without removing material. Swept bounds reject
//! clear intervals; subdivision resolves changing Z without treating a diagonal
//! approach as a horizontal traverse at its lowest point.
use super::*;
use crate::sweep::Sweep;

pub(super) fn first_contact(
    field: &Heightfield,
    a: [f64; 3],
    b: [f64; 3],
    tool: &ToolProfile,
    cancelled: &dyn Fn() -> bool,
) -> Option<(f64, [f64; 3], f32)> {
    if a[2].min(b[2]) >= f64::from(field.top()) {
        return None;
    }
    first_contact_along(a, b, cancelled, |start, end| {
        let (intrusion, stock) = field.max_intrusion_along_controlled(
            [start[0], start[1]],
            [end[0], end[1]],
            start[2].min(end[2]),
            tool,
            cancelled,
        )?;
        (intrusion > CLEARANCE_EPS).then_some(stock)
    })
}

/// Check the core, neck and shank against the existing stock. For plunges
/// and repositioning the teeth are also non-cutting.
#[allow(clippy::too_many_arguments)]
pub(super) fn first_thread_contact(
    field: &Heightfield,
    a: [f64; 3],
    b: [f64; 3],
    floor: f64,
    profile: ToolProfile,
    body: ThreadBody,
    check_teeth: bool,
    cancelled: &dyn Fn() -> bool,
) -> Option<(f64, [f64; 3], &'static str)> {
    let ProfileShape::Thread {
        root_radius,
        tooth_height,
        first_tooth_offset,
        pitch,
        tooth_count,
    } = profile.shape
    else {
        return None;
    };
    let cutting_end =
        first_tooth_offset + f64::from(tooth_count.saturating_sub(1)) * pitch + tooth_height;
    let sections = [
        ("core", root_radius, 0., cutting_end),
        ("neck", body.neck_radius, cutting_end, body.shoulder),
        ("shank", body.shank_radius, body.shoulder, body.length),
    ];
    first_contact_along(a, b, cancelled, |start, end| {
        let bottom = start[2].min(end[2]);
        let top = start[2].max(end[2]);
        for (part, radius, lower, upper) in sections {
            if radius <= 0.
                || upper <= lower
                || bottom + lower >= f64::from(field.top())
                || top + upper <= floor
            {
                continue;
            }
            let sweep = Sweep::new(start, end, ToolProfile::flat(radius));
            if contact_in_sweep(field, &sweep, cancelled, |x, y, column| {
                sweep
                    .cylinder_interval(x, y, lower, upper)
                    .is_some_and(|[lo, hi]| field.interval_contact(column, lo, hi, floor))
            }) {
                return Some(part);
            }
        }
        if !check_teeth
            || profile.radius - root_radius <= 1e-9
            || tooth_height <= 0.
            || bottom + first_tooth_offset >= f64::from(field.top())
            || top + cutting_end <= floor
        {
            return None;
        }
        let sweep = Sweep::new(start, end, profile);
        contact_in_sweep(field, &sweep, cancelled, |x, y, column| {
            sweep
                .thread_intervals(x, y, root_radius, tooth_height)
                .into_iter()
                .flatten()
                .any(|[lo, hi]| {
                    (0..tooth_count).any(|tooth| {
                        let offset = first_tooth_offset + f64::from(tooth) * pitch;
                        field.interval_contact(column, lo + offset, hi + offset, floor)
                    })
                })
        })
        .then_some("teeth")
    })
}

fn contact_in_sweep(
    field: &Heightfield,
    sweep: &Sweep,
    cancelled: &dyn Fn() -> bool,
    mut overlaps: impl FnMut(f64, f64, usize) -> bool,
) -> bool {
    let radius = sweep.tool.radius;
    let (y0, y1) = field.index_range(
        sweep.a[1].min(sweep.b[1]) - radius,
        sweep.a[1].max(sweep.b[1]) + radius,
        1,
    );
    let width = field.dims().0;
    for y in y0..=y1 {
        if cancelled() {
            return false;
        }
        let cy = field.center(0, y).1;
        let Some((lo, hi)) = sweep.row_span(cy) else {
            continue;
        };
        let (x0, x1) = field.index_range(lo, hi, 0);
        for x in x0..=x1 {
            if overlaps(field.center(x, y).0, cy, y * width + x) {
                return true;
            }
        }
    }
    false
}

fn first_contact_along<T>(
    a: [f64; 3],
    b: [f64; 3],
    cancelled: &dyn Fn() -> bool,
    mut contact: impl FnMut([f64; 3], [f64; 3]) -> Option<T>,
) -> Option<(f64, [f64; 3], T)> {
    let mut pending = vec![(0., 1., a, b)];
    while let Some((lo, hi, start, end)) = pending.pop() {
        if cancelled() {
            return None;
        }
        let Some(hit) = contact(start, end) else {
            continue;
        };
        if (end[0] - start[0])
            .hypot(end[1] - start[1])
            .hypot(end[2] - start[2])
            < 1e-5
            || hi - lo < 1e-10
        {
            return Some((hi, end, hit));
        }
        let mid = (lo + hi) * 0.5;
        let point = std::array::from_fn(|i| (start[i] + end[i]) * 0.5);
        pending.push((mid, hi, point, end));
        pending.push((lo, mid, start, point));
    }
    None
}
