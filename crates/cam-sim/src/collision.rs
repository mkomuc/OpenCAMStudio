//! Locate first stock contact without removing material. Swept bounds reject
//! clear intervals; subdivision resolves changing Z without treating a diagonal
//! approach as a horizontal traverse at its lowest point.
use super::*;

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
    let mut pending = vec![(0., 1., a, b)];
    while let Some((lo, hi, start, end)) = pending.pop() {
        if cancelled() {
            return None;
        }
        let Some((intrusion, stock)) = field.max_intrusion_along_controlled(
            [start[0], start[1]],
            [end[0], end[1]],
            start[2].min(end[2]),
            tool,
            cancelled,
        ) else {
            continue;
        };
        if intrusion <= CLEARANCE_EPS {
            continue;
        }
        if (end[0] - start[0])
            .hypot(end[1] - start[1])
            .hypot(end[2] - start[2])
            < 1e-5
            || hi - lo < 1e-10
        {
            return Some((hi, end, stock));
        }
        let mid = (lo + hi) * 0.5;
        let point = std::array::from_fn(|i| (start[i] + end[i]) * 0.5);
        pending.push((mid, hi, point, end));
        pending.push((lo, mid, start, point));
    }
    None
}
