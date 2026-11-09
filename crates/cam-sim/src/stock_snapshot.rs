//! Lossless, versioned material-state cache. Numeric stock arrays are stored as
//! little-endian bits; display meshes and transient tile revisions are rebuilt.
use super::*;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"OCAMSTK1";
const MAX_COLUMNS: usize = 4_000_000;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StockSnapshot {
    data: Arc<Vec<u8>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cutter {
    a: [f64; 3],
    b: [f64; 3],
    tool: ToolProfile,
    operation: Option<u32>,
    source: Option<CutSource>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Metadata {
    origin: [f64; 2],
    max: [f64; 2],
    resolution: f64,
    floor: f64,
    top: f32,
    cuts: Vec<Cutter>,
    layers: Vec<CutLayer>,
    cylinder: Option<HorizontalCylinder>,
    surface_tracking: bool,
}

impl Heightfield {
    pub fn snapshot(&self, floor: f64) -> Result<StockSnapshot, String> {
        if !floor.is_finite() {
            return Err("Invalid stock floor".into());
        }
        let metadata = Metadata {
            origin: self.origin,
            max: self.max,
            resolution: self.res,
            floor,
            top: self.top,
            cuts: self
                .cuts
                .iter()
                .map(|c| Cutter {
                    a: c.a,
                    b: c.b,
                    tool: c.tool,
                    operation: c.operation,
                    source: c.source,
                })
                .collect(),
            layers: self.layers.clone(),
            cylinder: self.horizontal_cylinder,
            surface_tracking: self.surface_tracking,
        };
        let json = serde_json::to_vec(&metadata).map_err(|e| e.to_string())?;
        let mut out = MAGIC.to_vec();
        put_count(&mut out, json.len())?;
        out.extend_from_slice(&json);
        for array in [&self.z, &self.uncut_z] {
            put_count(&mut out, array.len())?;
            for v in array {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for array in [&self.winners, &self.previous_layers] {
            put_count(&mut out, array.len())?;
            for v in array {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        put_count(&mut out, self.inherited_operations.len())?;
        for op in &self.inherited_operations {
            put_option(&mut out, *op);
        }
        put_count(&mut out, self.bounded.iter().count())?;
        for (index, spans) in self.bounded.iter() {
            put_count(&mut out, index)?;
            put_count(&mut out, spans.len())?;
            for span in spans {
                out.extend_from_slice(&span.lower.to_le_bytes());
                out.extend_from_slice(&span.upper.to_le_bytes());
                put_option(&mut out, span.lower_op);
                put_option(&mut out, span.upper_op);
                out.extend_from_slice(&span.lower_source.to_le_bytes());
                out.extend_from_slice(&span.upper_source.to_le_bytes());
            }
        }
        Ok(StockSnapshot {
            data: Arc::new(out),
        })
    }
}

impl StockSnapshot {
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        let snapshot = Self {
            data: Arc::new(data),
        };
        snapshot.restore()?;
        Ok(snapshot)
    }

    /// Restore exact stock at its saved resolution, ready for remeshing or cuts.
    pub fn restore(&self) -> Result<(Heightfield, f64), String> {
        let mut input = Input(&self.data);
        if input.take(8)? != MAGIC {
            return Err("Unsupported stock cache version".into());
        }
        let n = input.count(1)?;
        let meta: Metadata = serde_json::from_slice(input.take(n)?).map_err(|e| e.to_string())?;
        let nx = ((meta.max[0] - meta.origin[0]) / meta.resolution).ceil();
        let ny = ((meta.max[1] - meta.origin[1]) / meta.resolution).ceil();
        if !meta.resolution.is_finite()
            || meta.resolution <= 0.
            || !meta.floor.is_finite()
            || !meta.top.is_finite()
            || !meta.origin.into_iter().chain(meta.max).all(f64::is_finite)
            || !nx.is_finite()
            || !ny.is_finite()
            || nx < 1.
            || ny < 1.
            || nx * ny > MAX_COLUMNS as f64
        {
            return Err("Invalid cached stock grid".into());
        }
        let cells = nx as usize * ny as usize;
        let mut field =
            Heightfield::new(meta.origin, meta.max, meta.resolution, f64::from(meta.top));
        field.z = input.floats(cells, false)?;
        field.uncut_z = input.floats(cells, true)?;
        field.winners = input.integers(cells)?;
        field.previous_layers = input.integers(cells)?;
        let inherited = input.count(1)?;
        if inherited != 0 && inherited != cells {
            return Err("Invalid inherited stock columns".into());
        }
        field.inherited_operations = (0..inherited)
            .map(|_| input.option())
            .collect::<Result<_, _>>()?;
        for cut in meta.cuts {
            if !cut.a.into_iter().chain(cut.b).all(f64::is_finite)
                || !valid_tool(cut.tool)
                || cut.source.is_some_and(|s| {
                    !s.range.0.is_finite()
                        || !s.range.1.is_finite()
                        || s.range.0 < -1e-9
                        || s.range.1 > 1. + 1e-9
                        || s.range.0 > s.range.1
                })
            {
                return Err("Invalid cached cutter".into());
            }
            let mut sweep = Sweep::new(cut.a, cut.b, cut.tool);
            sweep.operation = cut.operation;
            sweep.source = cut.source;
            field.cuts.push(sweep);
        }
        let valid_cut = |id: u32| id == u32::MAX || (id as usize) < field.cuts.len();
        if field.winners.iter().any(|id| !valid_cut(*id))
            || field
                .previous_layers
                .iter()
                .any(|id| *id != u32::MAX && *id as usize >= meta.layers.len())
        {
            return Err("Invalid cached stock references".into());
        }
        for (i, layer) in meta.layers.iter().enumerate() {
            if !layer.height.is_finite()
                || !valid_cut(layer.cut)
                || (layer.previous != u32::MAX && layer.previous as usize >= i)
            {
                return Err("Invalid cached surface layer".into());
            }
        }
        field.layers = meta.layers;
        let columns = input.count(8)?;
        if columns > cells {
            return Err("Too many cached interval columns".into());
        }
        let mut previous = None;
        for _ in 0..columns {
            let index = input.u32()? as usize;
            if index >= cells || previous.is_some_and(|p| p >= index) {
                return Err("Invalid cached column order".into());
            }
            previous = Some(index);
            let count = input.count(18)?;
            let mut spans = Vec::with_capacity(count);
            let mut upper = f32::NEG_INFINITY;
            for _ in 0..count {
                let span = RemovedSpan {
                    lower: input.f32()?,
                    upper: input.f32()?,
                    lower_op: input.option()?,
                    upper_op: input.option()?,
                    lower_source: input.u32()?,
                    upper_source: input.u32()?,
                };
                if span.lower >= span.upper
                    || span.lower < upper
                    || !valid_cut(span.lower_source)
                    || !valid_cut(span.upper_source)
                {
                    return Err("Invalid cached material interval".into());
                }
                upper = span.upper;
                spans.push(span);
            }
            field.bounded.insert(index, spans);
        }
        if let Some(c) = meta.cylinder {
            if c.axis > 1
                || !c.center.into_iter().all(f64::is_finite)
                || !c.outer_radius.is_finite()
                || !c.inner_radius.is_finite()
                || c.outer_radius <= 0.
                || c.inner_radius < 0.
                || c.inner_radius >= c.outer_radius
            {
                return Err("Invalid cached stock cylinder".into());
            }
        }
        if !input.0.is_empty() {
            return Err("Trailing stock cache data".into());
        }
        field.horizontal_cylinder = meta.cylinder;
        field.surface_tracking = meta.surface_tracking;
        // Revisions are fresh; no old mesh tile can be confused with this snapshot.
        Ok((field, meta.floor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CylindricalStock, ProfileShape, StockMeshCache};

    fn roundtrip(field: &Heightfield, floor: f64) -> Heightfield {
        let saved = field.snapshot(floor).unwrap();
        let loaded = StockSnapshot::from_bytes(saved.as_bytes().to_vec()).unwrap();
        let (restored, loaded_floor) = loaded.restore().unwrap();
        assert_eq!(loaded_floor.to_bits(), floor.to_bits());
        assert_eq!(restored.snapshot(floor).unwrap(), saved);
        assert_eq!(field.to_solid_mesh(floor), restored.to_solid_mesh(floor));
        assert_eq!(
            StockMeshCache::default().mesh_controlled(field, floor, &|| false),
            StockMeshCache::default().mesh_controlled(&restored, floor, &|| false)
        );
        assert_eq!(
            field.removed_volume().to_bits(),
            restored.removed_volume().to_bits()
        );
        restored
    }

    #[test]
    fn snapshots_preserve_every_cutter_shape_layers_and_future_cuts() {
        let shapes = [
            ProfileShape::Flat,
            ProfileShape::Ball,
            ProfileShape::BullNose { corner_radius: 0.3 },
            ProfileShape::Cone {
                half_angle_rad: 0.4,
                flat_radius: 0.2,
            },
            ProfileShape::VTip {
                half_angle_rad: 0.4,
                tip_radius: 0.1,
            },
            ProfileShape::Thread {
                root_radius: 0.8,
                tooth_height: 0.4,
                pitch: 1.5,
                tooth_count: 3,
            },
        ];
        for shape in shapes {
            let mut field = Heightfield::new([-3., -3.], [3., 3.], 0.3, 3.);
            field.set_surface_tracking(true);
            field.set_cut_source(15, (0.123456789012345, 0.987654321098765));
            let tool = ToolProfile { radius: 1.2, shape };
            field.cut_segment_profile([-1., 0., 0.], [1., 0., 0.4], &tool);
            field.set_cut_source(16, (0., 1.));
            field.cut_segment_profile([-1., 0., 1.], [1., 0., 1.], &tool);
            let mut restored = roundtrip(&field, -3.);
            // Tracking cursors are transient; callers set the next motion source.
            field.set_cut_source(17, (0., 1.));
            restored.set_cut_source(17, (0., 1.));
            for f in [&mut field, &mut restored] {
                f.cut_segment_profile([0., -1., -0.5], [0., 1., -0.5], &tool);
            }
            assert_eq!(
                field.snapshot(-3.).unwrap(),
                restored.snapshot(-3.).unwrap()
            );
        }
    }

    #[test]
    fn snapshots_preserve_rotary_tube_voids_ownership_and_sources() {
        for axis in 0..=2 {
            let stock = CylindricalStock::new(axis, [0.; 3], [-3., 3.], 3., 1., 0.3).unwrap();
            let mut field = stock.field;
            field.set_surface_tracking(true);
            let source = field.rotary_source(
                [0.; 3],
                ToolProfile {
                    radius: 0.75,
                    shape: ProfileShape::Ball,
                },
                42,
                0.3141592653589793,
            );
            field.inherited_operations = vec![Some(12); field.z.len()];
            for index in (0..field.z.len()).step_by(3) {
                field.subtract_interval(index, -2.5, -1.5, Some(22), source);
                field.subtract_interval(index, 0.5, 2., Some(23), source);
            }
            roundtrip(&field, stock.min[2]);
        }
    }

    #[test]
    fn malformed_and_truncated_snapshots_are_rejected() {
        let field = Heightfield::new([0.; 2], [1.; 2], 0.5, 1.);
        let saved = field.snapshot(0.).unwrap();
        for n in 0..saved.as_bytes().len() {
            assert!(
                StockSnapshot::from_bytes(saved.as_bytes()[..n].to_vec()).is_err(),
                "accepted truncated length {n}"
            );
        }
        let mut bytes = saved.as_bytes().to_vec();
        bytes[7] = b'9';
        assert!(StockSnapshot::from_bytes(bytes).is_err());
        let mut bytes = saved.as_bytes().to_vec();
        bytes.push(0);
        assert!(StockSnapshot::from_bytes(bytes).is_err());
        let mut invalid = field.clone();
        invalid.winners[0] = 1234;
        assert!(
            StockSnapshot::from_bytes(invalid.snapshot(0.).unwrap().as_bytes().to_vec()).is_err()
        );
        invalid = field;
        invalid.z[0] = f32::NAN;
        assert!(
            StockSnapshot::from_bytes(invalid.snapshot(0.).unwrap().as_bytes().to_vec()).is_err()
        );
    }
}

fn valid_tool(tool: ToolProfile) -> bool {
    if !tool.radius.is_finite() || tool.radius <= 0. {
        return false;
    }
    match tool.shape {
        crate::ProfileShape::Flat | crate::ProfileShape::Ball => true,
        crate::ProfileShape::BullNose { corner_radius } => {
            corner_radius.is_finite() && corner_radius >= 0.
        }
        crate::ProfileShape::Cone {
            half_angle_rad,
            flat_radius,
        } => half_angle_rad.is_finite() && flat_radius.is_finite() && flat_radius >= 0.,
        crate::ProfileShape::VTip {
            half_angle_rad,
            tip_radius,
        } => half_angle_rad.is_finite() && tip_radius.is_finite() && tip_radius >= 0.,
        crate::ProfileShape::Thread {
            root_radius,
            tooth_height,
            pitch,
            tooth_count,
        } => {
            root_radius.is_finite()
                && root_radius >= 0.
                && tooth_height.is_finite()
                && tooth_height > 0.
                && pitch.is_finite()
                && pitch > 0.
                && tooth_count > 0
        }
    }
}

fn put_count(out: &mut Vec<u8>, value: usize) -> Result<(), String> {
    out.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| "Stock cache too large")?
            .to_le_bytes(),
    );
    Ok(())
}
fn put_option(out: &mut Vec<u8>, value: Option<u32>) {
    out.push(u8::from(value.is_some()));
    if let Some(v) = value {
        out.extend_from_slice(&v.to_le_bytes());
    }
}
struct Input<'a>(&'a [u8]);
impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.0.len() {
            return Err("Truncated stock cache".into());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() {
            return Err("Non-finite cached stock height".into());
        }
        Ok(v)
    }
    fn count(&mut self, stride: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > self.0.len() / stride {
            return Err("Invalid stock cache array length".into());
        }
        Ok(n)
    }
    fn floats(&mut self, cells: usize, optional: bool) -> Result<Vec<f32>, String> {
        let n = self.count(4)?;
        if n != cells && !(optional && n == 0) {
            return Err("Stock grid/array mismatch".into());
        }
        (0..n).map(|_| self.f32()).collect()
    }
    fn integers(&mut self, cells: usize) -> Result<Vec<u32>, String> {
        let n = self.count(4)?;
        if n != cells {
            return Err("Stock grid/array mismatch".into());
        }
        (0..n).map(|_| self.u32()).collect()
    }
    fn option(&mut self) -> Result<Option<u32>, String> {
        match self.take(1)?[0] {
            0 => Ok(None),
            1 => self.u32().map(Some),
            _ => Err("Invalid cached optional ID".into()),
        }
    }
}
