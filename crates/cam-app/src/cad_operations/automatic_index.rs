//! Resolve independent 2D machining selections in their rotary frames.
use super::*;
use crate::AppController;
use std::collections::{BTreeMap, BTreeSet};

const ANGLE_TOLERANCE: f64 = 1e-7;

pub(crate) fn resolve(
    document: &Document,
    geometry: &GeometryState,
    operation: &Operation,
    index: &RotaryIndex,
    auto: &AutomaticRotaryIndex,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<ResolvedIndexedCut>, String> {
    index.validate()?;
    if !operation.supports_automatic_index() {
        return Err("Auto indexing requires a 2D operation".into());
    }
    if auto.selected.is_empty() {
        return Err("Select machining geometry for Auto indexing on the Geometry tab".into());
    }

    let mut fixed = index.clone();
    fixed.auto = None;
    fixed.face = None;
    fixed.flip = false;
    let mut source = document.clone();
    let mut resolved = source.clone();
    let mut cuts = Vec::new();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (position, selection) in auto.selected.iter().enumerate() {
        if !seen.insert(selection) {
            let label = describe_entities(geometry, std::slice::from_ref(selection));
            errors.push(format!(
                "Selection {} ({label}): This geometry was selected more than once",
                position + 1
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    for (position, selection) in indexed_features(geometry, operation, &auto.selected)? {
        if cancelled() {
            return Err("Geometry resolution cancelled".into());
        }
        let label = describe_entities(geometry, &selection);
        let cut = (|| {
            let directions =
                cam_geometry3d::automatic_index_directions(geometry, &selection, operation)?;
            let mut angles = Vec::new();
            for direction in directions {
                if let Ok(angle) = fixed.angle_for_direction(direction) {
                    // Use the same signed half-turn convention on both sides of the seam.
                    let angle = if angle.abs() < ANGLE_TOLERANCE {
                        0.
                    } else if (angle.abs() - 180.).abs() < ANGLE_TOLERANCE {
                        -180.
                    } else {
                        angle
                    };
                    if !angles
                        .iter()
                        .any(|a: &f64| (*a - angle).abs() < ANGLE_TOLERANCE)
                    {
                        angles.push(angle);
                    }
                }
            }
            if angles.is_empty() {
                return Err(
                    "The machining orientation is unreachable with the configured rotary axis"
                        .into(),
                );
            }
            let mut valid = None;
            let mut rejection = String::new();
            for angle in angles {
                fixed.angle = angle;
                match selection_in_frame(&source, geometry, operation, &fixed, &selection) {
                    Ok(selection) => {
                        if valid.replace((angle, selection)).is_some() {
                            return Err("The machining orientation is ambiguous; select geometry with a unique tool approach".into());
                        }
                    }
                    Err(reason) => rejection = reason,
                }
            }
            let (angle, selection) = valid.ok_or(rejection)?;
            fixed.angle = angle;
            resolve_indexed_cut(
                &mut source,
                &mut resolved,
                operation,
                &fixed,
                selection,
                &label,
            )
        })();
        match cut {
            Ok(cut) => cuts.push(cut),
            Err(reason) => errors.push(format!("Selection {} ({label}): {reason}", position + 1)),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }

    // Normalize equivalent angles before the stable machining-order sort, so
    // nearly parallel placements cannot reorder ties or cause a second index.
    let mut by_angle: Vec<_> = (0..cuts.len()).collect();
    by_angle.sort_by(|&a, &b| cuts[a].angle.total_cmp(&cuts[b].angle));
    let mut group_angle = f64::NEG_INFINITY;
    for i in by_angle {
        if cuts[i].angle - group_angle >= ANGLE_TOLERANCE {
            group_angle = cuts[i].angle;
        }
        cuts[i].angle = group_angle;
    }
    if auto.order == RotaryMachiningOrder::RotaryAngle {
        cuts.sort_by(|a, b| a.angle.total_cmp(&b.angle));
    }
    Ok(cuts)
}

/// Preserve pick order while treating each connected edge chain as one feature.
/// Named Profile entries keep their own membership and machining settings.
fn indexed_features(
    geometry: &GeometryState,
    operation: &Operation,
    selected: &[EntityRef],
) -> Result<Vec<(usize, Vec<EntityRef>)>, String> {
    let mut remaining: BTreeSet<_> = selected
        .iter()
        .filter(|r| r.kind == EntityKind::Edge)
        .cloned()
        .collect();
    let mut edge_sets = Vec::new();
    if let Operation::Profile(profile) = operation {
        for entry in profile.selections.as_deref().unwrap_or_default() {
            if let ProfileGeometrySource::Cad(refs) = &entry.source {
                let edges: Vec<_> = refs
                    .iter()
                    .filter(|r| remaining.remove(*r))
                    .cloned()
                    .collect();
                if !edges.is_empty() {
                    edge_sets.push(edges);
                }
            }
        }
    }
    edge_sets.push(
        selected
            .iter()
            .filter(|r| remaining.contains(*r))
            .cloned()
            .collect(),
    );
    let positions: BTreeMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(i, reference)| (reference, i))
        .collect();
    let mut features: BTreeMap<_, _> = selected
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| (i, vec![r]))
        .collect();
    for edges in edge_sets.into_iter().filter(|edges| edges.len() > 1) {
        let mut entry = ProfileGeometryEntry::new(ProfileGeometryKind::Chain, String::new());
        entry.source = ProfileGeometrySource::Cad(edges.clone());
        let chains = cam_geometry3d::split_profile_chain_entry(geometry, &entry)
            .map_err(|error| format!("{}: {error}", describe_entities(geometry, &edges)))?;
        let chains = chains.map_or_else(
            || vec![edges],
            |chains| {
                chains
                    .into_iter()
                    .filter_map(|chain| match chain.source {
                        ProfileGeometrySource::Cad(refs) => Some(refs),
                        _ => None,
                    })
                    .collect()
            },
        );
        for chain in chains {
            let position = chain
                .iter()
                .map(|reference| positions[reference])
                .min()
                .unwrap();
            for reference in &chain {
                features.remove(&positions[reference]);
            }
            features.insert(position, chain);
        }
    }
    Ok(features.into_iter().collect())
}

struct SelectionInFrame {
    geometry: GeometryState,
    binding: GeometryBinding,
    stock: ([f64; 3], [f64; 3]),
}

fn selection_in_frame(
    source: &Document,
    geometry: &GeometryState,
    operation: &Operation,
    index: &RotaryIndex,
    selected: &[EntityRef],
) -> Result<SelectionInFrame, String> {
    let id = operation.id();
    let kind = AppController::operation_kind(operation);
    let view = source.view_at_index(id, index)?;
    let stock = view.stock_bounds_for(view.setup());
    let stock_top = stock.1[2];
    let geometry = index.geometry(geometry);
    let mut recipe = recipe(kind, &geometry, selected)?;
    if let Operation::Profile(profile) = operation {
        if let Some(entry) = profile.selections.as_ref().and_then(|entries| {
            entries.iter().find(|entry| {
                matches!(&entry.source,
                ProfileGeometrySource::Cad(refs) if selected.iter().all(|selection| refs.contains(selection)))
            })
        }) {
            let mut entry = entry.clone();
            entry.source = ProfileGeometrySource::Cad(selected.to_vec());
            recipe = ExtractionRecipe::ProfileEntries {
                entries: vec![entry],
            };
        }
    }
    let binding = if matches!(kind, OpKind::Drill | OpKind::Bore | OpKind::Thread) {
        // Cylinders have an undirected axis. Certify the modeled entry instead
        // of using the advisory fallback, which can project either end.
        cam_geometry3d::resolve_selection(&geometry, selected, recipe, stock_top)?
    } else {
        cam_geometry3d::resolve_selection_advisory(&geometry, selected, recipe, stock_top)?
    };
    Ok(SelectionInFrame {
        geometry,
        binding,
        stock,
    })
}

fn resolve_indexed_cut(
    source: &mut Document,
    resolved: &mut Document,
    operation: &Operation,
    index: &RotaryIndex,
    selection: SelectionInFrame,
    label: &str,
) -> Result<ResolvedIndexedCut, String> {
    let id = operation.id();
    let SelectionInFrame {
        geometry,
        binding,
        stock: (stock_min, stock_max),
    } = selection;
    let stock_top = stock_max[2];
    source.rotary_indexes.insert(id, index.clone());
    resolved.geometry = geometry;
    let mut operation = operation.clone();
    if let Operation::Profile(profile) = &mut operation {
        profile.selections = None;
        profile.additional_chains.clear();
    }
    if let Operation::Drill(drill) = &mut operation {
        // Auto's explicit geometry list defines the machining order.
        drill.geometry.select_same_diameter = false;
    }
    resolved.geometry.bindings.insert(id, binding.clone());
    source.geometry.bindings.insert(id, binding);
    let settings = source
        .setup()
        .height_settings
        .get(&id)
        .cloned()
        .map(|mut settings| {
            settings.resolved = None;
            settings
        });
    if let Some(settings) = &settings {
        resolved
            .setup_mut()
            .height_settings
            .insert(id, settings.clone());
    }
    resolve_operation_input(source, resolved, &mut operation, true)?;

    let binding = resolved.geometry.bindings[&id].clone();
    let cutting = binding.effective_heights();
    source.geometry.bindings.insert(id, binding.clone());
    let configured = match settings {
        Some(_) if uses_drill_model_heights(source, &operation, &binding) => {
            resolved.setup().height_settings[&id].resolved
        }
        Some(settings) => Some(settings.resolve(source, &operation)?),
        None => None,
    };
    let safety = source.setup().heights_for(id);
    Ok(ResolvedIndexedCut {
        operation,
        angle: index.angle,
        heights: configured.unwrap_or(ResolvedOperationHeights {
            stock_top,
            clearance: safety.clearance,
            retract: safety.retract,
            feed: None,
            top: cutting.top,
            bottom: cutting.bottom,
        }),
        stock_xy: ([stock_min[0], stock_min[1]], [stock_max[0], stock_max[1]]),
        binding,
        selection_label: label.into(),
    })
}
