//! Analytic swept cutter samples, shared by rasterisation and contour recovery.
use crate::{ProfileShape, ToolProfile};

#[derive(Clone, Copy, Debug)]
pub(super) struct Sweep {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub tool: ToolProfile,
    pub operation: Option<u32>,
    pub source: Option<crate::heightfield::CutSource>,
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
            source: None,
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

    /// Z ranges swept by one tooth, relative to its axial offset on the tool.
    /// A column can enter the tooth's cutting annulus on both sides of the
    /// non-cutting core, so the two encounters must be evaluated independently.
    pub(super) fn thread_intervals(
        &self,
        x: f64,
        y: f64,
        root: f64,
        height: f64,
    ) -> [Option<[f64; 2]>; 2] {
        let dx = x - self.a[0];
        let dy = y - self.a[1];
        let dz = self.b[2] - self.a[2];
        let radius = self.tool.radius;
        let flank_slope = height * 0.5 / (radius - root);
        if self.len2 < 1e-24 {
            let distance = dx.hypot(dy);
            if distance < root || distance > radius {
                return [None; 2];
            }
            let flank = (distance - root) * flank_slope;
            return [
                Some([
                    self.a[2].min(self.b[2]) + flank,
                    self.a[2].max(self.b[2]) + height - flank,
                ]),
                None,
            ];
        }

        let length = self.len2.sqrt();
        let along = (dx * self.dx + dy * self.dy) / length;
        let across = (dx * self.dy - dy * self.dx).abs() / length;
        if across > radius {
            return [None; 2];
        }
        let reach = (radius * radius - across * across).max(0.).sqrt();
        let start = (-along).max(-reach);
        let end = (length - along).min(reach);
        let z_slope = dz / length;
        let extreme = if z_slope.abs() < flank_slope {
            // The lower flank is convex along the move and the upper flank is
            // concave. Their extrema can be displaced from closest approach.
            -across * z_slope / (flank_slope * flank_slope - z_slope * z_slope).sqrt()
        } else if z_slope > 0. {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        let interval = |start: f64, end: f64| {
            if end < start {
                return None;
            }
            let lower = extreme.clamp(start, end);
            let upper = (-extreme).clamp(start, end);
            let z = |s: f64| self.a[2] + dz * ((along + s) / length).clamp(0., 1.);
            let flank = |s: f64| (s.hypot(across) - root).max(0.) * flank_slope;
            Some([z(lower) + flank(lower), z(upper) + height - flank(upper)])
        };
        if across < root {
            let core = (root * root - across * across).sqrt();
            [
                interval(start, end.min(-core)),
                interval(start.max(core), end),
            ]
        } else {
            [interval(start, end), None]
        }
    }
    pub fn surface_location(&self, point: [f32; 3]) -> Option<crate::SurfaceLocation> {
        let source = self.source?;
        let t = if self.len2 < 1e-12 && (self.b[2] - self.a[2]).abs() > 1e-12 {
            ((f64::from(point[2]) - self.a[2]) / (self.b[2] - self.a[2])).clamp(0., 1.)
        } else if self.len2 > 1e-12
            && self.tool.shape == ProfileShape::Flat
            && (self.b[2] - self.a[2]).abs() < 1e-12
        {
            // A flat floor is exposed when the leading cutter edge first
            // reaches it, not when the axis later passes over cleared stock.
            let dx = f64::from(point[0]) - self.a[0];
            let dy = f64::from(point[1]) - self.a[1];
            let along = (dx * self.dx + dy * self.dy) / self.len2;
            let perpendicular2 = ((dx * self.dy - dy * self.dx).powi(2) / self.len2).max(0.);
            (along - ((self.tool.radius.powi(2) - perpendicular2).max(0.) / self.len2).sqrt())
                .clamp(0., 1.)
        } else {
            self.projection(f64::from(point[0]), f64::from(point[1])).0
        };
        Some(crate::SurfaceLocation {
            step_index: source.step_index,
            fraction: source.range.0 + t * (source.range.1 - source.range.0),
        })
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
