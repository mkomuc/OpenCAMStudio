//! Analytic swept cutter samples, shared by rasterisation and contour recovery.
use crate::{ProfileShape, ToolProfile};

#[derive(Clone, Copy, Debug)]
pub(super) struct Sweep {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub tool: ToolProfile,
    pub operation: Option<u32>,
    dx: f64,
    dy: f64,
    len2: f64,
}
impl Sweep {
    pub fn new(a: [f64; 3], b: [f64; 3], tool: ToolProfile) -> Self {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        Self {
            a,
            b,
            tool,
            operation: None,
            dx,
            dy,
            len2: dx * dx + dy * dy,
        }
    }
    pub fn projection(&self, x: f64, y: f64) -> (f64, f64) {
        let t = if self.len2 < 1e-12 {
            0.
        } else {
            (((x - self.a[0]) * self.dx + (y - self.a[1]) * self.dy) / self.len2).clamp(0., 1.)
        };
        let d2 = (x - self.a[0] - t * self.dx).powi(2) + (y - self.a[1] - t * self.dy).powi(2);
        (t, d2)
    }
    pub fn bottom(&self, t: f64, d2: f64) -> f64 {
        let z = if self.len2 < 1e-12 {
            self.a[2].min(self.b[2])
        } else {
            self.a[2] + (self.b[2] - self.a[2]) * t
        };
        z + if self.tool.shape == ProfileShape::Flat {
            0.
        } else {
            self.tool.offset(d2.sqrt())
        }
    }
    /// Negative inside the cutter's swept volume at a horizontal slicing plane.
    pub fn implicit(&self, x: f64, y: f64, level: f64) -> f64 {
        let (t, d2) = self.projection(x, y);
        (d2.sqrt() - self.tool.radius).max(self.bottom(t, d2) - level)
    }
    /// Exact horizontal slice of the capsule: endpoint discs plus the swept
    /// rectangle. Long diagonal moves visit their narrow band, not their AABB.
    pub fn row_span(&self, y: f64) -> Option<(f64, f64)> {
        let r = self.tool.radius;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for p in [self.a, self.b] {
            let d = y - p[1];
            if d.abs() <= r {
                let x = (r * r - d * d).max(0.).sqrt();
                lo = lo.min(p[0] - x);
                hi = hi.max(p[0] + x);
            }
        }
        if self.len2 > 1e-12 {
            let mut left = f64::NEG_INFINITY;
            let mut right = f64::INFINITY;
            // Restrict A*x+B to [min,max], relative to the segment's start.
            let mut restrict = |a: f64, b: f64, min: f64, max: f64| {
                if a.abs() < 1e-12 {
                    if b < min || b > max {
                        left = f64::INFINITY;
                        right = f64::NEG_INFINITY;
                    }
                } else {
                    let x0 = (min - b) / a;
                    let x1 = (max - b) / a;
                    left = left.max(x0.min(x1));
                    right = right.min(x0.max(x1));
                }
            };
            let dy = y - self.a[1];
            let width = r * self.len2.sqrt();
            restrict(-self.dy, self.dx * dy, -width, width);
            restrict(self.dx, self.dy * dy, 0., self.len2);
            if left <= right {
                lo = lo.min(self.a[0] + left);
                hi = hi.max(self.a[0] + right);
            }
        }
        (lo <= hi).then_some((lo, hi))
    }
}
