//! Exact rational Bezier spans, independent of either display mesh.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CadBezierSpan {
    pub poles: Vec<[f64; 3]>,
    pub weights: Vec<f64>,
}

impl CadBezierSpan {
    pub fn is_valid(&self) -> bool {
        (2..=26).contains(&self.poles.len())
            && self.weights.len() == self.poles.len()
            && self.poles.iter().flatten().all(|v| v.is_finite())
            && self.weights.iter().all(|w| w.is_finite() && *w > 0.)
            && self.weights.iter().copied().fold(f64::INFINITY, f64::min)
                / self.weights.iter().copied().fold(0., f64::max)
                >= 1e-12
    }
}

/// Bound each chord using the positive-weight control hull, rather than midpoint
/// sampling, which can miss inflections and entire loops. Subdivision happens in
/// homogeneous coordinates so rational curves retain their exact definition.
pub fn approximate_bezier_spans(
    spans: &[CadBezierSpan],
    tolerance: f64,
) -> Result<Vec<[f64; 3]>, String> {
    const BUDGET: usize = 65536;
    if !tolerance.is_finite()
        || tolerance <= 0.
        || spans.is_empty()
        || spans.iter().map(|s| s.poles.len()).sum::<usize>() > BUDGET
        || spans.iter().any(|s| !s.is_valid())
    {
        return Err("invalid exact spline curve or approximation tolerance".into());
    }
    let project = |p: [f64; 4]| [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut work = 0;
    for span in spans {
        let max_weight = span.weights.iter().copied().fold(0., f64::max);
        let poles: Vec<[f64; 4]> = span
            .poles
            .iter()
            .zip(&span.weights)
            .map(|(p, w)| {
                let w = w / max_weight;
                [p[0] * w, p[1] * w, p[2] * w, w]
            })
            .collect();
        let start = project(poles[0]);
        if let Some(previous) = points.last() {
            if (0..3)
                .map(|i| (start[i] - previous[i]).powi(2))
                .sum::<f64>()
                > tolerance.min(1e-6).powi(2)
            {
                return Err("exact spline spans are disconnected".into());
            }
            // Retain the actual endpoint of each span. Replacing it with the
            // preceding endpoint would add its rounding gap to the chord error.
            if *previous != start {
                points.push(start);
            }
        } else {
            points.push(start);
        }
        let mut stack = vec![(poles, 0)];
        while let Some((mut hull, depth)) = stack.pop() {
            work += 1;
            if work > 2 * BUDGET || depth > 48 || points.len() > BUDGET {
                return Err("machining curve exceeds approximation resource budget".into());
            }
            let start = project(hull[0]);
            let end = project(*hull.last().unwrap());
            let delta: [f64; 3] = std::array::from_fn(|i| end[i] - start[i]);
            let length2 = delta.iter().map(|v| v * v).sum::<f64>();
            let flat = hull.iter().all(|p| {
                let p = project(*p);
                let along = if length2 > 0. {
                    ((0..3).map(|i| (p[i] - start[i]) * delta[i]).sum::<f64>() / length2)
                        .clamp(0., 1.)
                } else {
                    0.
                };
                (0..3)
                    .map(|i| (p[i] - start[i] - along * delta[i]).powi(2))
                    .sum::<f64>()
                    <= tolerance * tolerance
            });
            if flat {
                if points.last() != Some(&end) {
                    points.push(end);
                }
                continue;
            }
            let mut left = vec![hull[0]];
            let mut right = vec![*hull.last().unwrap()];
            for remaining in (1..hull.len()).rev() {
                for i in 0..remaining {
                    hull[i] = std::array::from_fn(|j| hull[i][j] * 0.5 + hull[i + 1][j] * 0.5);
                }
                left.push(hull[0]);
                right.push(hull[remaining - 1]);
            }
            right.reverse();
            stack.push((right, depth + 1));
            stack.push((left, depth + 1));
        }
    }
    if points.len() < 2 {
        return Err("degenerate exact spline curve".into());
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_quarter_circle_stays_within_the_requested_chord_error() {
        let span = CadBezierSpan {
            poles: vec![[1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
            weights: vec![1., 0.5f64.sqrt(), 1.],
        };
        let points = approximate_bezier_spans(std::slice::from_ref(&span), 0.001).unwrap();
        assert_eq!(points.first(), span.poles.first());
        assert_eq!(points.last(), span.poles.last());
        assert!(points.iter().all(|p| (p[0].hypot(p[1]) - 1.).abs() < 1e-12));
        for pair in points.windows(2) {
            let midpoint = [
                (pair[0][0] + pair[1][0]) * 0.5,
                (pair[0][1] + pair[1][1]) * 0.5,
            ];
            assert!(1. - midpoint[0].hypot(midpoint[1]) <= 0.001);
        }
    }

    #[test]
    fn inflections_cannot_hide_between_endpoints_and_midpoint() {
        let span = CadBezierSpan {
            poles: vec![[0., 0., 0.], [1., 10., 0.], [2., -10., 0.], [3., 0., 0.]],
            weights: vec![1.; 4],
        };
        let points = approximate_bezier_spans(&[span], 0.01).unwrap();
        assert!(points.iter().any(|p| p[1] > 2.));
        assert!(points.iter().any(|p| p[1] < -2.));
    }

    #[test]
    fn invalid_weights_disconnected_spans_and_excessive_precision_are_rejected() {
        let mut span = CadBezierSpan {
            poles: vec![[0., 0., 0.], [1., 1., 0.], [2., 0., 0.]],
            weights: vec![1.; 3],
        };
        assert!(approximate_bezier_spans(std::slice::from_ref(&span), 1e-30).is_err());
        assert!(approximate_bezier_spans(&[span.clone(), span.clone()], 0.01).is_err());
        for weight in [-1., 0., f64::NAN, f64::INFINITY] {
            span.weights[1] = weight;
            assert!(approximate_bezier_spans(std::slice::from_ref(&span), 0.01).is_err());
        }
    }
}
