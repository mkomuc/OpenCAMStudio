//! CAD import preparation and the boundary between topology resolution and the
//! existing planar strategies. No source tessellation is used to create a cut.
use crate::{JobParams, OpKind};
use cam_model::*;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct CadImportPreview {
    pub name: String,
    pub items: usize,
    pub source_units: SourceUnits,
    pub dimensions_mm: [f64; 3],
    pub diagnostics: Vec<String>,
}
pub(crate) enum PreparedImport {
    Drawing {
        import: cam_import::Import,
        name: String,
    },
    Solid {
        asset: GeometryAsset,
        instances: Vec<GeometryInstance>,
    },
}
impl PreparedImport {
    pub fn preview(&self) -> CadImportPreview {
        match self {
            Self::Drawing { import, name } => {
                let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
                for p in import
                    .regions
                    .iter()
                    .flat_map(|r| r.outer().points())
                    .chain(import.open_chains.iter().flat_map(|p| p.points()))
                {
                    for (axis, value) in [p.x, p.y, 0.0].into_iter().enumerate() {
                        bounds[0][axis] = bounds[0][axis].min(value);
                        bounds[1][axis] = bounds[1][axis].max(value);
                    }
                }
                CadImportPreview {
                    name: name.clone(),
                    items: import.regions.len() + import.open_chains.len(),
                    source_units: SourceUnits::Millimetres,
                    dimensions_mm: std::array::from_fn(|i| (bounds[1][i] - bounds[0][i]).max(0.0)),
                    diagnostics: import.warnings.clone(),
                }
            }
            Self::Solid { asset, instances } => {
                let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
                for instance in instances {
                    if let Some(body) = asset
                        .topology
                        .bodies
                        .iter()
                        .find(|b| b.id == instance.body_id)
                    {
                        for x in [body.bounds[0][0], body.bounds[1][0]] {
                            for y in [body.bounds[0][1], body.bounds[1][1]] {
                                for z in [body.bounds[0][2], body.bounds[1][2]] {
                                    let p = instance.placement.transform_point([x, y, z]);
                                    for i in 0..3 {
                                        bounds[0][i] = bounds[0][i].min(p[i]);
                                        bounds[1][i] = bounds[1][i].max(p[i]);
                                    }
                                }
                            }
                        }
                    }
                }
                CadImportPreview {
                    name: asset.name.clone(),
                    items: instances.len(),
                    source_units: asset.source_units,
                    dimensions_mm: std::array::from_fn(|i| (bounds[1][i] - bounds[0][i]).max(0.0)),
                    diagnostics: asset.import_diagnostics.clone(),
                }
            }
        }
    }
}
pub(crate) fn prepare_import(path: &Path) -> Result<PreparedImport, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Imported model")
        .to_owned();
    // Snapshot source bytes once so provenance cannot race a CAD re-export.
    let source_data = std::fs::read(path).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    static NEXT_SNAPSHOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT_SNAPSHOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = std::env::temp_dir().join(format!(
        "ocam-import-{}-{stamp}-{serial}.{extension}",
        std::process::id()
    ));
    struct SourceSnapshot(std::path::PathBuf);
    impl Drop for SourceSnapshot {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let snapshot = SourceSnapshot(temporary);
    file.write_all(&source_data).map_err(|e| e.to_string())?;
    drop(file);
    match cam_import::read_geometry_file(&snapshot.0, &Default::default(), &Default::default())
        .map_err(|e| e.to_string())?
    {
        cam_import::GeometryImport::Drawing(import) => Ok(PreparedImport::Drawing { import, name }),
        cam_import::GeometryImport::Solid(import) => {
            let import = *import;
            let format = match path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "iges" | "igs" => CadFormat::Iges,
                _ => CadFormat::Step,
            };
            let instances = import
                .placements
                .into_iter()
                .enumerate()
                .map(|(index, p)| GeometryInstance {
                    id: index as u64 + 1,
                    asset_id: 1,
                    body_id: p.body_id,
                    name: format!("{name} / body {}", p.body_id),
                    placement: p.placement,
                    visible: true,
                })
                .collect();
            let options = cam_kernel::SolidImportOptions::default();
            let asset = GeometryAsset {
                import_options: Some(GeometryImportOptions {
                    import_tolerance: options.import_tolerance,
                    display_tolerance: options.display_tolerance,
                }),
                import_diagnostics: import.warnings,
                source_fingerprint: GeometryAsset::fingerprint_source(&source_data),
                id: 1,
                revision: 1,
                name,
                source_format: format,
                source_data: source_data.into(),
                source_units: import.source_units,
                backend: import.backend,
                topology: import.topology.into(),
            };
            Ok(PreparedImport::Solid { asset, instances })
        }
    }
}
pub(crate) fn recipe(
    kind: OpKind,
    geometry: &GeometryState,
    selected: &[EntityRef],
) -> Result<ExtractionRecipe, String> {
    Ok(match kind {
        OpKind::Pocket => ExtractionRecipe::PocketFloor,
        OpKind::Drill => ExtractionRecipe::DrillHoles,
        OpKind::Profile => ExtractionRecipe::ProfileLoop {
            side: cam_geometry3d::profile_side(geometry, selected)?,
        },
        OpKind::Face => ExtractionRecipe::FaceRegion,
        _ => return Err("CAD selections currently support Pocket, Drill, Profile and Face".into()),
    })
}

/// Source topology stays immutable. Only instance placements are rotated into
/// the setup axes, preserving semantic references and approximation budgets.
pub(crate) fn geometry_for_setup(document: &Document) -> Result<GeometryState, String> {
    let setup = document.setup();
    let mut geometry = document.geometry.geometry_in_setup(&setup.frame)?;
    let ids: std::collections::BTreeSet<_> = setup.operations.iter().map(Operation::id).collect();
    geometry.bindings.retain(|id, _| ids.contains(id));
    if !setup.model_instances.is_empty() {
        geometry
            .instances
            .retain(|instance| setup.model_instances.contains(&instance.id));
    }
    Ok(geometry)
}

/// Conservative model bounds from exact source body bounds; display mesh quality
/// cannot alter stock dimensions or NC. Evaluate each body in setup axes.
pub(crate) fn model_bounds(geometry: &GeometryState, setup: &Setup) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut include = |p: [f64; 3]| {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    };
    for instance in &geometry.instances {
        if !setup.model_instances.is_empty() && !setup.model_instances.contains(&instance.id) {
            continue;
        }
        let Some(asset) = geometry.asset(instance.asset_id) else {
            continue;
        };
        let Some(body) = asset
            .topology
            .bodies
            .iter()
            .find(|b| b.id == instance.body_id)
        else {
            continue;
        };
        for x in [body.bounds[0][0], body.bounds[1][0]] {
            for y in [body.bounds[0][1], body.bounds[1][1]] {
                for z in [body.bounds[0][2], body.bounds[1][2]] {
                    include(
                        setup
                            .frame
                            .to_setup_vector(instance.placement.transform_point([x, y, z])),
                    );
                }
            }
        }
    }
    for p in geometry
        .drawing_regions
        .iter()
        .flat_map(|r| r.outer().points())
        .chain(geometry.drawing_paths.iter().flat_map(|p| p.points()))
    {
        include(setup.frame.to_setup_vector([p.x, p.y, 0.]));
    }
    if min[0].is_infinite() {
        ([0.; 3], [0.; 3])
    } else {
        (min, max)
    }
}

fn stock_top(document: &Document) -> f64 {
    let (min, max) = model_bounds(&document.geometry, document.setup());
    document.setup().stock.resolve_bounds(min, max).1[2]
}
/// Validate the complete selection before strategy extraction. A mixed geometry
/// field cannot become a valid operation by dropping its incompatible members.
pub(crate) fn validate_selection(
    geometry: &GeometryState,
    selected: &[EntityRef],
) -> Result<(), String> {
    let Some(first) = selected.first() else {
        return Ok(());
    };
    if selected.iter().any(|r| !geometry.entity_exists(r)) {
        return Err("Needs reselection: missing entity or stale asset revision".into());
    }
    if selected.iter().any(|r| r.kind != first.kind) {
        return Err("Select features of the same kind; faces, loops and edges cannot be mixed in one geometry field".into());
    }
    if selected.iter().any(|r| {
        r.asset_id != first.asset_id
            || r.instance_id != first.instance_id
            || r.body_id != first.body_id
    }) {
        return Err("Machining requires features from one body instance".into());
    }
    if selected
        .iter()
        .enumerate()
        .any(|(i, r)| selected[..i].contains(r))
    {
        return Err("The same geometry feature was selected more than once".into());
    }
    Ok(())
}

pub(crate) fn create_operation(
    document: &Document,
    kind: OpKind,
    selected: Vec<EntityRef>,
    id: u32,
    tool: u32,
    work_offset: u32,
    defaults: JobParams,
) -> Result<(Operation, GeometryBinding), String> {
    let geometry = geometry_for_setup(document)?;
    validate_selection(&geometry, &selected)?;
    let binding = cam_geometry3d::resolve_selection(
        &geometry,
        &selected,
        recipe(kind, &geometry, &selected)?,
        stock_top(document),
    )?;
    let resolved = cam_geometry3d::resolve_binding(&geometry, &binding)?;
    let depth = resolved.heights.depth();
    let boundary = || {
        resolved
            .regions
            .first()
            .map(|r| r.outer().clone())
            .ok_or_else(|| "No resolved boundary".to_string())
    };
    let op = match kind {
        OpKind::Pocket => Operation::Pocket(PocketOp {
            id,
            tool,
            work_offset,
            spindle_rpm: defaults.spindle_rpm,
            boundary: boundary()?,
            islands: resolved.regions[0].holes().to_vec(),
            depth,
            start: None,
            clear: ClearParams {
                stepdown: defaults.stepdown,
                overlap: 0.5,
                offset: 0.0,
                feed: defaults.feed,
                plunge_feed: defaults.plunge_feed,
                plunge: Plunge::Straight,
                lead_overlap: 0.0,
                lead_in: Lead::None,
                lead_out: Lead::None,
                clearing: Clearing::default(),
            },
        }),
        OpKind::Profile => Operation::Profile(ProfileOp {
            id,
            tool,
            work_offset,
            spindle_rpm: defaults.spindle_rpm,
            chain: boundary()?,
            side: match binding.recipe {
                ExtractionRecipe::ProfileLoop { side } => side,
                _ => unreachable!("Profile uses a profile extraction recipe"),
            },
            comp: Comp::Computed,
            offset: 0.0,
            depth,
            stepdown: defaults.stepdown,
            stepover: 0.0,
            feed: defaults.feed,
            plunge_feed: defaults.plunge_feed,
            start: None,
            lead_in: Lead::None,
            lead_out: Lead::None,
            lead_overlap: 0.0,
            plunge: Plunge::Straight,
            clearing: Clearing::default(),
        }),
        OpKind::Drill => Operation::Drill(DrillOp {
            id,
            tool,
            work_offset,
            spindle_rpm: defaults.spindle_rpm,
            points: resolved
                .holes
                .iter()
                .map(|h| [h.center.x, h.center.y])
                .collect(),
            depth,
            start_offset: resolved.heights.top - document.setup().heights.top_of_stock,
            peck: None,
            dwell: None,
            feed: defaults.plunge_feed,
        }),
        OpKind::Face => Operation::Face(FaceOp {
            id,
            tool,
            work_offset,
            spindle_rpm: defaults.spindle_rpm,
            boundary: boundary()?,
            start_offset: resolved.heights.top,
            depth,
            stepdown: defaults.stepdown,
            overlap: 0.5,
            overshoot: 0.0,
            direction: Axis::X,
            feed: defaults.feed,
            plunge_feed: defaults.plunge_feed,
        }),
        _ => unreachable!(),
    };
    Ok((op, binding))
}

/// A fresh topology resolution replaces every linked strategy input. Stale
/// embedded contours are never a fallback after asset replacement or removal.
pub(crate) fn resolve_document(document: &Document) -> Result<Document, String> {
    resolve_document_checked(document, &|| false).map_err(|diagnostic| diagnostic.message)
}

/// Typed source ownership is preserved when resolution runs in a worker.
pub(crate) fn resolve_document_checked(
    document: &Document,
    cancelled: &dyn Fn() -> bool,
) -> Result<Document, cam_toolpath::Diagnostic> {
    let error = |message: String| {
        cam_toolpath::Diagnostic::error(message)
            .with_code(cam_toolpath::DiagnosticCode::GeometryUnresolved)
            .for_setup(document.setup().id)
    };
    if cancelled() {
        return Err(error("Geometry resolution cancelled".into())
            .with_code(cam_toolpath::DiagnosticCode::Cancelled));
    }
    document.geometry.validate().map_err(error)?;
    let mut resolved = document.clone();
    // Resolve all reference geometry in its source placement, before rotating
    // instances for the strategy adapters. Keep a worker-only absolute snapshot.
    for operation in &document.setup().operations {
        if document.excluded_operations.contains(&operation.id()) || document.disabled_origins.get(&document.setup().id).is_some_and(|set|set.contains(&operation.work_offset())) { continue; }
        if let Some(settings) = document.setup().height_settings.get(&operation.id()) {
            let heights = settings.resolve(document,operation).map_err(|reason| error(format!("Operation {}: {reason}",operation.id())).for_op(operation.id()))?;
            resolved.setup_mut().height_settings.get_mut(&operation.id()).unwrap().resolved=Some(heights);
        }
    }
    resolved.geometry = geometry_for_setup(document).map_err(error)?;
    let mut operations = std::mem::take(&mut resolved.setup_mut().operations);
    for operation in &mut operations {
        if cancelled() {
            return Err(error("Geometry resolution cancelled".into())
                .with_code(cam_toolpath::DiagnosticCode::Cancelled));
        }
        let id = operation.id();
        let resolved_operation: Result<(), String> = (|| {
            let Some(binding) = document.geometry.bindings.get(&id) else {
                if document.cad_linked_operations.contains(&id) {
                    return Err(format!(
                        "Operation {id}: linked CAD geometry is missing; reselect its geometry"
                    ));
                }
                return Ok(());
            };
            if let ResolutionStatus::NeedsReselection { reason } = &binding.status {
                return Err(format!("Operation {id}: Needs reselection — {reason}"));
            }
            validate_selection(&resolved.geometry, &binding.selected)
                .map_err(|e| format!("Operation {id}: {e}"))?;
            let mut current_binding = binding.clone();
            if matches!(current_binding.recipe, ExtractionRecipe::FaceRegion) {
                current_binding.heights.top = stock_top(document);
            }
            let input = cam_geometry3d::resolve_binding(&resolved.geometry, &current_binding)
                .map_err(|e| format!("Operation {id}: {e}"))?;
            let depth = input.heights.depth();
            let region = || input.regions.first().ok_or("No resolved machining region");
            match (operation, &binding.recipe) {
                (Operation::Pocket(p), ExtractionRecipe::PocketFloor) => {
                    p.boundary = region()?.outer().clone();
                    p.islands = region()?.holes().to_vec();
                    p.depth = depth;
                }
                (Operation::Profile(p), ExtractionRecipe::ProfileLoop { side }) => {
                    if p.side != *side
                        || p.comp != Comp::Computed
                        || p.offset < 0.0
                        || p.stepover > 0.0
                    {
                        return Err(format!("Operation {id}: profile side/compensation/roughing is outside the certified CAD adapter"));
                    }
                    p.chain = region()?.outer().clone();
                    p.depth = depth;
                }
                (Operation::Drill(p), ExtractionRecipe::DrillHoles) => {
                    let tool = document
                        .setup()
                        .tools
                        .iter()
                        .find(|t| t.number == p.tool)
                        .ok_or("Missing drill tool")?;
                    if input
                        .holes
                        .iter()
                        .any(|h| (h.diameter - tool.diameter).abs() > 1e-3)
                    {
                        return Err(format!(
                            "Operation {id}: drill diameter must match the selected bore"
                        ));
                    }
                    p.points = input
                        .holes
                        .iter()
                        .map(|h| [h.center.x, h.center.y])
                        .collect();
                    p.depth = depth;
                    p.start_offset = input.heights.top - document.setup().heights_for(id).top_of_stock;
                }
                (Operation::Face(p), ExtractionRecipe::FaceRegion) => {
                    if p.overshoot < 0.0 {
                        return Err(format!(
                            "Operation {id}: negative overshoot is not certified for CAD facing"
                        ));
                    }
                    p.boundary = region()?.outer().clone();
                    p.depth = depth;
                    p.start_offset = input.heights.top;
                }
                _ => {
                    return Err(format!(
                        "Operation {id}: geometry recipe does not match the operation"
                    ))
                }
            }
            // Planner consumes absolute heights. The extraction service already applied
            // and certified manual overrides; do not apply them a second time.
            let cache = resolved.geometry.bindings.get_mut(&id).unwrap();
            cache.heights = input.heights;
            cache.overrides = GeometryOverrides::default();
            let fingerprint = resolved
                .geometry
                .binding_fingerprint(&resolved.geometry.bindings[&id]);
            resolved
                .geometry
                .bindings
                .get_mut(&id)
                .unwrap()
                .resolved_fingerprint = Some(fingerprint);
            Ok(())
        })();
        resolved_operation.map_err(|message| error(message).for_op(id))?;
    }
    resolved.setup_mut().operations = operations;
    Ok(resolved)
}

pub(crate) fn mark_missing_bindings(state: &mut GeometryState) {
    let stale: Vec<_> = state
        .bindings
        .iter()
        .filter(|(_, b)| !b.selected.iter().all(|r| state.entity_exists(r)))
        .map(|(id, _)| *id)
        .collect();
    for id in stale {
        state.bindings.get_mut(&id).unwrap().status = ResolutionStatus::NeedsReselection {
            reason: "Source geometry revision or entity is missing; reselect geometry".into(),
        };
    }
}

/// Inspector depth edits are explicit overrides while tool/feed/allowance edits
/// preserve the geometry link. Depth values remain physical magnitudes.
pub(crate) fn capture_height_override(
    binding: Option<&mut GeometryBinding>,
    before: Option<CuttingHeights>,
    after: &Operation,
    heights: &Heights,
) {
    let Some(binding) = binding else {
        return;
    };
    let Some(before) = before else {
        return;
    };
    let operation = after;
    let Some(after) = operation.legacy_cutting_heights(heights) else {
        return;
    };
    let effective = binding.effective_heights();
    match after.top - before.top {
        delta if delta != 0.0 => {
            binding.overrides.top = Some(effective.top + delta);
            binding.overrides.bottom = Some(effective.bottom + delta);
        }
        _ => {}
    }
    let (before_depth, after_depth) = match operation {
        Operation::Pocket(p) => (-before.bottom, p.depth),
        Operation::Profile(p) => (-before.bottom, p.depth),
        Operation::Drill(p) => (before.depth(), p.depth),
        Operation::Face(p) => (before.depth(), p.depth),
        _ => return,
    };
    if after_depth != before_depth {
        binding.overrides.bottom = Some(binding.effective_heights().top - after_depth);
    }
}

/// Validate an inspector edit without discarding its explicit overrides or
/// requiring unrelated unresolved operations to be repaired first.
pub(crate) fn validate_binding_edit(document: &mut Document, id: u32) -> Result<(), String> {
    let mut preview = document.clone();
    preview.setup_mut().operations.retain(|op| op.id() == id);
    // Geometry repair is independent of the height editor's pending references.
    preview.setup_mut().height_settings.clear();
    let resolved = resolve_document(&preview)?;
    let operation = resolved
        .setup()
        .operations
        .first()
        .cloned()
        .ok_or("Missing operation")?;
    let slot = document
        .setup_mut()
        .operations
        .iter_mut()
        .find(|op| op.id() == id)
        .ok_or("Missing operation")?;
    *slot = operation;
    let mut base = document.geometry.bindings[&id].clone();
    base.overrides = GeometryOverrides::default();
    if matches!(base.recipe, ExtractionRecipe::FaceRegion) {
        base.heights.top = stock_top(document);
    }
    let input = cam_geometry3d::resolve_binding(&geometry_for_setup(document)?, &base)?;
    let binding = document.geometry.bindings.get_mut(&id).unwrap();
    binding.heights = input.heights;
    binding.resolved_fingerprint = None;
    Ok(())
}

/// Keep displayed linked heights synchronized with placement and stock edits.
/// Replacement/removal requests remain unresolved until explicit reselection.
pub(crate) fn refresh_binding_heights(document: &mut Document) {
    let updates: Vec<_> = document.geometry.bindings.iter().filter_map(|(&id,binding)| {
        if matches!(&binding.status,ResolutionStatus::NeedsReselection{reason} if !reason.starts_with("Geometry changed:")) {return None;}
        let mut base=binding.clone();base.overrides=GeometryOverrides::default();
        let mut context=document.clone();
        let owner=document.setups.iter().find(|s|s.operations.iter().any(|o|o.id()==id))?;
        context.active_setup_id=owner.id;
        if matches!(base.recipe,ExtractionRecipe::FaceRegion) {base.heights.top=stock_top(&context);}
        Some((id,geometry_for_setup(&context).and_then(|geometry|cam_geometry3d::resolve_binding(&geometry,&base)).map(|input|input.heights)))
    }).collect();
    for (id, result) in updates {
        let binding = document.geometry.bindings.get_mut(&id).unwrap();
        binding.resolved_fingerprint = None;
        match result {
            Ok(heights) => {
                binding.heights = heights;
                binding.status = ResolutionStatus::Resolved;
            }
            Err(reason) => {
                binding.status = ResolutionStatus::NeedsReselection {
                    reason: format!("Geometry changed: {reason}"),
                }
            }
        }
    }
}
