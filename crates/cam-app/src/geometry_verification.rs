//! Local, append-only observations for improving automatic feature recognition.
//! Records contain selected geometry and context, never the full imported mesh.
use crate::{AppController, OpKind};
use cam_model::{EntityKind, EntityRef, ProfileGeometryEntry};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub(crate) fn log_path() -> Option<PathBuf> {
    std::env::var_os("OPENCAM_GEOMETRY_VERIFICATION_LOG")
        .map(PathBuf::from)
        .or_else(|| crate::paths::config_file("geometry-verification.jsonl"))
}

pub(crate) fn record(
    controller: &AppController,
    selected: &[EntityRef],
    kind: Option<OpKind>,
    entry: Option<&ProfileGeometryEntry>,
    binding: Option<&cam_model::GeometryBinding>,
    finding: Option<&str>,
    extraction_error: Option<&str>,
) -> Result<bool, String> {
    if selected.is_empty() {
        return Ok(false);
    }
    let state = controller.geometry_state();
    let features: Vec<_> = selected.iter().map(|reference| {
        let Some(asset) = state.asset(reference.asset_id) else { return json!({"entity":reference}); };
        let topology = &asset.topology;
        let loops: Vec<_> = topology.loops.iter().filter(|wire| match reference.kind {
            EntityKind::Loop => wire.id == reference.id,
            EntityKind::Face => wire.face_id == reference.id,
            _ => false,
        }).collect();
        let edge_ids: BTreeSet<_> = loops.iter().flat_map(|wire| wire.edge_uses.iter().map(|usage| usage.edge_id))
            .chain((reference.kind == EntityKind::Edge).then_some(reference.id)).collect();
        let edges: Vec<_> = topology.edges.iter().filter(|edge| edge_ids.contains(&edge.id)).collect();
        let face_ids: BTreeSet<_> = edges.iter().flat_map(|edge| edge.face_ids.iter().copied())
            .chain((reference.kind == EntityKind::Face).then_some(reference.id)).collect();
        let faces: Vec<_> = topology.faces.iter().filter(|face| face_ids.contains(&face.id)).collect();
        json!({"entity":reference, "asset_name":asset.name, "backend":asset.backend,
            "source_format":asset.source_format, "source_fingerprint":asset.source_fingerprint,
            "import_options":asset.import_options,
            "topology_counts":{"faces":topology.faces.len(),"edges":topology.edges.len(),
                "display_triangles":topology.mesh.triangles.len(),
                "projection_triangles":topology.projection.as_ref().map(|projection| projection.mesh.triangles.len())},
            "placement":state.instance(reference.instance_id).map(|instance| instance.placement),
            "body":topology.bodies.iter().find(|body| body.id == reference.body_id),
            "edges":edges,"faces":faces,"loops":loops})
    }).collect();
    let setup = controller.document().setup();
    let mut event = json!({"schema_version":1,"verifier_version":1,"app_version":crate::version_string(),
        "policy":"advisory", "project":controller.current_path(), "source_name":controller.source_name(),
        "setup":{"id":setup.id,"name":setup.name,"frame":setup.frame,"stock":setup.stock,"origin":setup.origin},
        "operation_kind":kind.map(|kind| format!("{kind:?}")), "entry":entry, "binding":binding,
        "selected":selected,"features":features,
        "selection_accepted":selected.iter().all(|reference| state.entity_exists(reference)),
        "geometry_resolved":extraction_error.is_none(), "verification_passed":finding.is_none(),
        "verification_code":verification_code(finding),
        "verification_info":finding,"geometry_error":extraction_error});
    // Large spline control nets stay in the source project; retain identities,
    // source fingerprints, outcome and context without duplicating megabytes.
    if event.to_string().len() > 256 * 1024 {
        event.as_object_mut().unwrap().remove("features");
        event["geometry_details_omitted"] = json!(
            "Selected geometry exceeds the 256 KiB record budget; consult the source project."
        );
    }
    let path = log_path()
        .ok_or("No configuration directory is available for geometry verification records")?;
    append(&path, event)
}

fn verification_code(finding: Option<&str>) -> &'static str {
    let Some(finding) = finding else {
        return "verified";
    };
    let reason = finding.to_lowercase();
    if reason.contains("missing") || reason.contains("stale") {
        "missing_reference"
    } else if reason.contains("repeats") {
        "repeated_selection"
    } else if reason.contains("different planes") {
        "mixed_heights"
    } else if reason.contains("upward") || reason.contains("facing") {
        "face_orientation"
    } else if reason.contains("wall") || reason.contains("overhang") || reason.contains("roof") {
        "wall_access"
    } else if reason.contains("plane") || reason.contains("planar") || reason.contains("aligned") {
        "setup_alignment"
    } else if reason.contains("side") {
        "side_inference"
    } else {
        "feature_recognition"
    }
}

fn append(path: &Path, event: Value) -> Result<bool, String> {
    static SEEN: OnceLock<Mutex<BTreeSet<u64>>> = OnceLock::new();
    let mut seen = SEEN
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    // Keep only small process-local keys; retaining thousands of complete
    // spline records here would make observation logging consume model-sized
    // amounts of memory.
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    serde_json::to_vec(&event)
        .map_err(|error| error.to_string())?
        .hash(&mut hasher);
    let key = hasher.finish();
    if seen.contains(&key) {
        return Ok(false);
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let mut line = serde_json::to_vec(&json!({"timestamp_unix_ms":timestamp,"event":event}))
        .map_err(|error| error.to_string())?;
    line.push(b'\n');
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(&line).map_err(|error| error.to_string())?;
    if seen.len() >= 4096 {
        seen.clear();
    }
    seen.insert(key);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_append_across_projects_and_duplicate_events_are_suppressed() {
        let path = std::env::temp_dir().join(format!(
            "geometry-verification-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = json!({"project":"one.ocam","policy":"advisory","verification_passed":false});
        assert!(append(&path, first.clone()).unwrap());
        assert!(!append(&path, first).unwrap());
        assert!(append(
            &path,
            json!({"project":"two.ocam","verification_passed":true})
        )
        .unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["event"]["project"], "one.ocam");
        assert_eq!(lines[1]["event"]["project"], "two.ocam");
        assert!(lines[0]["timestamp_unix_ms"].as_u64().is_some());
        std::fs::remove_file(path).unwrap();
    }
}
