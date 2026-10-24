//! Polygon offsetting — the geometric core of tool-radius compensation.

use i_overlay::mesh::outline::offset::OutlineOffset;
use i_overlay::mesh::style::{LineJoin, OutlineStyle};

use crate::{polygon, GeoError, Polygon, GRID_MM, GRID_SCALE};

/// How offset corners are treated where two edges meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JoinStyle {
    /// Roll a fillet arc around the corner. This is the physically correct
    /// default for tool-radius compensation: a round tool cannot cut a sharp
    /// outside corner, so the offset boundary is an arc of the tool radius.
    Round,
    /// Extend the edges to a sharp point. Beyond a limiting angle the corner is
    /// bevelled to avoid runaway spikes.
    Miter,
    /// Cut the corner off flat.
    Bevel,
}

/// Round-join / round-cap arc resolution, expressed as `segment_length /
/// arc_radius`. About 0.03 % chord deviation (~126 facets on a full turn) —
/// well under any milling tolerance. Shared with [`crate::stroke_path`].
pub(crate) const ROUND_RATIO: f64 = 0.05;

/// Miter limit, as the minimum half-angle (radians) below which a sharp corner
/// is bevelled instead of spiking to a far-off point.
const MITER_MIN_ANGLE: f64 = 0.5;

impl JoinStyle {
    pub(crate) fn to_line_join(self) -> LineJoin<f64> {
        match self {
            JoinStyle::Round => LineJoin::Round(ROUND_RATIO),
            JoinStyle::Miter => LineJoin::Miter(MITER_MIN_ANGLE),
            JoinStyle::Bevel => LineJoin::Bevel,
        }
    }
}

/// Offset a set of filled regions by `distance` millimetres.
///
/// A **positive** distance grows the regions outward (as when compensating a
/// profile toolpath to the outside of a part by the tool radius); a **negative**
/// distance shrinks them inward (as when stepping a pocket in). Holes are
/// offset consistently with their outer boundary.
///
/// The result may split into several polygons (a thin neck pinching off) or
/// vanish entirely (shrinking past the medial axis), so a `Vec` is returned.
/// Offsetting an empty input yields an empty result.
pub fn offset(
    regions: &[Polygon],
    distance: f64,
    join: JoinStyle,
) -> Result<Vec<Polygon>, GeoError> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }
    // An inward offset cannot contain an area once its diameter exceeds the
    // outer boundary's width on either axis. Avoid sending those collapsed
    // contours to the outline engine: dense CAD circles can otherwise spend
    // minutes intersecting their inverted offset edges after disappearing.
    let shapes: Vec<_> = regions
        .iter()
        .filter(|region| {
            if !distance.is_finite() || distance >= 0.0 {
                return true;
            }
            let mut min = [f64::INFINITY; 2];
            let mut max = [f64::NEG_INFINITY; 2];
            for point in region.outer().points() {
                min[0] = min[0].min(point.x);
                min[1] = min[1].min(point.y);
                max[0] = max[0].max(point.x);
                max[1] = max[1].max(point.y);
            }
            -2.0 * distance <= (max[0] - min[0]).min(max[1] - min[1]) + 2.0 * GRID_MM
        })
        .map(Polygon::to_shape)
        .collect();
    if shapes.is_empty() {
        return Ok(Vec::new());
    }
    let style = OutlineStyle::new(distance).line_join(join.to_line_join());
    let result = shapes.outline_fixed_scale(&style, GRID_SCALE)?;
    Ok(polygon::from_shapes(result))
}
