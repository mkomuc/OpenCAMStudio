//! Versioned .ocam ZIP container. Editable JSON and small verification reports
//! are separate from independently compressed material-state blocks.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufWriter, Read, Seek, Write};

const VERSION: u32 = 1;
const MAX_ENTRY: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct CacheEntries {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stock: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_mesh: Option<String>,
}

#[derive(serde::Serialize)]
struct Manifest {
    archive_version: u32,
    project: OcamFile,
    stock_caches: BTreeMap<u64, CacheEntries>,
}
#[derive(serde::Deserialize)]
struct ReadManifest {
    archive_version: u32,
    project: serde_json::Value,
    stock_caches: BTreeMap<u64, CacheEntries>,
}

// Existing JSON projects have no dexels. Preserve those caches losslessly until
// a new verification can replace them with actual stock state.
#[derive(serde::Serialize, serde::Deserialize)]
struct LegacyMesh {
    initial_stock: Option<(Vec<cam_render::MeshVertex>, Vec<u32>)>,
    stock_vertices: Vec<cam_render::MeshVertex>,
    stock_indices: Vec<u32>,
    stock_machined: Vec<bool>,
    stock_operations: Vec<Option<u32>>,
    stock_cut_sources: Vec<Option<cam_sim::SurfaceLocation>>,
}

impl LegacyMesh {
    fn validate(&self) -> Result<(), String> {
        let valid = |vertices: &[cam_render::MeshVertex], indices: &[u32]| {
            indices.len().is_multiple_of(3)
                && indices.iter().all(|&i| (i as usize) < vertices.len())
                && vertices.iter().all(|v| {
                    v.position
                        .iter()
                        .chain(&v.normal)
                        .chain(&v.color)
                        .all(|n| n.is_finite())
                })
        };
        let n = self.stock_vertices.len();
        if !valid(&self.stock_vertices, &self.stock_indices)
            || self
                .initial_stock
                .as_ref()
                .is_some_and(|(v, i)| !valid(v, i))
            || self.stock_machined.len() != n
            || self.stock_operations.len() != n
            || (!self.stock_cut_sources.is_empty() && self.stock_cut_sources.len() != n)
        {
            return Err("Invalid legacy stock mesh".into());
        }
        Ok(())
    }
}
#[derive(serde::Serialize)]
struct LegacyMeshRef<'a> {
    initial_stock: Option<&'a (Vec<cam_render::MeshVertex>, Vec<u32>)>,
    stock_vertices: &'a Vec<cam_render::MeshVertex>,
    stock_indices: &'a Vec<u32>,
    stock_machined: &'a Vec<bool>,
    stock_operations: &'a Vec<Option<u32>>,
    stock_cut_sources: &'a Vec<Option<cam_sim::SurfaceLocation>>,
}

fn write_json<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    value: &impl serde::Serialize,
) -> Result<(), String> {
    // Serde emits many tiny writes. Batch these before CRC/compression work.
    let mut buffered = BufWriter::with_capacity(256 * 1024, zip);
    serde_json::to_writer(&mut buffered, value).map_err(|e| e.to_string())?;
    buffered.flush().map_err(|e| e.to_string())
}

pub(super) fn write<W: Write + Seek>(writer: W, project: &Project) -> Result<W, String> {
    project.validate()?;
    project.document.geometry.validate()?;
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));
    let mut verification = BTreeMap::new();
    let mut stock_caches = BTreeMap::new();
    for (id, saved) in &project.toolpaths.verification {
        let inherited = project
            .document
            .setups
            .iter()
            .find(|s| s.id == *id)
            .is_some_and(|s| s.stock == cam_model::Stock::PreviousSetup);
        let mut entries = CacheEntries::default();
        if let Some(snapshot) = &saved.stock_snapshot {
            let path = format!("stock/{id}/final.bin");
            zip.start_file(&path, options).map_err(|e| e.to_string())?;
            zip.write_all(snapshot.as_bytes())
                .map_err(|e| e.to_string())?;
            entries.stock = Some(path);
            if inherited {
                if let Some(initial) = &saved.initial_stock_snapshot {
                    let path = format!("stock/{id}/initial.bin");
                    zip.start_file(&path, options).map_err(|e| e.to_string())?;
                    zip.write_all(initial.as_bytes())
                        .map_err(|e| e.to_string())?;
                    entries.initial = Some(path);
                }
            }
        } else if !saved.stock_vertices.is_empty() || saved.initial_stock.is_some() && inherited {
            let path = format!("stock/{id}/legacy-mesh.json");
            zip.start_file(&path, options).map_err(|e| e.to_string())?;
            write_json(
                &mut zip,
                &LegacyMeshRef {
                    initial_stock: saved.initial_stock.as_ref().filter(|_| inherited),
                    stock_vertices: &saved.stock_vertices,
                    stock_indices: &saved.stock_indices,
                    stock_machined: &saved.stock_machined,
                    stock_operations: &saved.stock_operations,
                    stock_cut_sources: &saved.stock_cut_sources,
                },
            )
            .map_err(|e| e.to_string())?;
            entries.legacy_mesh = Some(path);
        }
        let required = entries.stock.is_some() || entries.legacy_mesh.is_some();
        if saved.requires_stock_cache && !required {
            return Err("Missing stock cache for saved verification".into());
        }
        verification.insert(
            *id,
            StoredVerification {
                fingerprint: saved.fingerprint.clone(),
                program: saved.program.clone(),
                diagnostics: saved.diagnostics.clone(),
                collisions: saved.collisions.clone(),
                requires_stock_cache: required,
                ..Default::default()
            },
        );
        if required {
            stock_caches.insert(*id, entries);
        }
    }
    let core = Project {
        view: project.view.clone(),
        toolpaths: StoredToolpaths {
            cache: project.toolpaths.cache.clone(),
            fingerprints: project.toolpaths.fingerprints.clone(),
            verification,
        },
        schema_version: project.schema_version,
        document: project.document.clone(),
        regions: project.regions.clone(),
        open_paths: project.open_paths.clone(),
        defaults: project.defaults,
        source_name: project.source_name.clone(),
        machine: project.machine.clone(),
        post: project.post,
    };
    zip.start_file("project.json", options)
        .map_err(|e| e.to_string())?;
    write_json(
        &mut zip,
        &Manifest {
            archive_version: VERSION,
            project: OcamFile::Project(core),
            stock_caches,
        },
    )
    .map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())
}

fn entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = zip.by_name(name).map_err(|e| e.to_string())?;
    if file.size() > MAX_ENTRY {
        return Err("Project cache entry is too large".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_ENTRY + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ENTRY {
        return Err("Project cache entry is too large".into());
    }
    Ok(bytes)
}

pub(super) fn read<R: Read + Seek>(reader: R, strict: bool) -> Result<OcamFile, LoadError> {
    let error = |e: String| LoadError::Json(format!("Invalid project archive: {e}"));
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| error(e.to_string()))?;
    let mut names = BTreeSet::new();
    if zip.len() > 100_000 || !zip.file_names().all(|name| names.insert(name.to_owned())) {
        return Err(error("Duplicate or excessive archive entries".into()));
    }
    let metadata = entry(&mut zip, "project.json").map_err(error)?;
    let manifest: ReadManifest = serde_json::from_slice(&metadata)?;
    if manifest.archive_version != VERSION {
        return Err(error(format!(
            "Unsupported archive version {}",
            manifest.archive_version
        )));
    }
    let OcamFile::Project(mut project) = OcamFile::from_value(manifest.project)? else {
        return Err(error("Archive does not contain a project".into()));
    };
    let mut invalid = Vec::new();
    for (id, saved) in &mut project.toolpaths.verification {
        // Cache failures are isolated from the editable document.
        let result = (|| -> Result<(), String> {
            if let Some(entries) = manifest.stock_caches.get(id) {
                if entries.stock.is_some() == entries.legacy_mesh.is_some() {
                    return Err("Ambiguous or missing stock cache".into());
                }
                if let Some(path) = &entries.stock {
                    if project
                        .document
                        .setups
                        .iter()
                        .any(|s| s.id == *id && s.stock == cam_model::Stock::PreviousSetup)
                        && entries.initial.is_none()
                    {
                        return Err("Missing inherited initial stock".into());
                    }
                    saved.stock_snapshot =
                        Some(cam_sim::StockSnapshot::from_bytes(entry(&mut zip, path)?)?);
                }
                if let Some(path) = &entries.initial {
                    saved.initial_stock_snapshot =
                        Some(cam_sim::StockSnapshot::from_bytes(entry(&mut zip, path)?)?);
                }
                if let Some(path) = &entries.legacy_mesh {
                    let legacy: LegacyMesh = serde_json::from_slice(&entry(&mut zip, path)?)
                        .map_err(|e| e.to_string())?;
                    legacy.validate()?;
                    saved.initial_stock = legacy.initial_stock;
                    saved.stock_vertices = legacy.stock_vertices;
                    saved.stock_indices = legacy.stock_indices;
                    saved.stock_machined = legacy.stock_machined;
                    saved.stock_operations = legacy.stock_operations;
                    saved.stock_cut_sources = legacy.stock_cut_sources;
                }
                saved.requires_stock_cache = false;
            } else if saved.requires_stock_cache {
                return Err("Missing stock cache".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            if strict {
                return Err(error(e));
            }
            invalid.push(*id);
        }
    }
    for id in invalid {
        project.toolpaths.verification.remove(&id);
    }
    project.validate().map_err(LoadError::Json)?;
    Ok(OcamFile::Project(project))
}


impl OcamFile {
    /// Read either a versioned project archive or legacy tagged/untagged JSON.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LoadError> {
        if bytes.starts_with(b"PK") {
            read(std::io::Cursor::new(bytes), false)
        } else {
            Self::from_json(std::str::from_utf8(bytes).map_err(|e| LoadError::Json(e.to_string()))?)
        }
    }

    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Self, LoadError> {
        let bytes = std::fs::read(path).map_err(|e| LoadError::Json(e.to_string()))?;
        Self::from_bytes(&bytes)
    }
}

impl Project {
    /// Encode the optimized project container. Library files remain JSON.
    pub fn to_archive(&self) -> Result<Vec<u8>, String> {
        write(std::io::Cursor::new(Vec::new()), self).map(|c| c.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn project() -> Project {
        let OcamFile::Project(project) =
            OcamFile::from_json(&super::super::tests::v9_project_json()).unwrap()
        else {
            panic!()
        };
        project
    }

    fn unpack(bytes: &[u8]) -> Project {
        let OcamFile::Project(project) = OcamFile::from_bytes(bytes).unwrap() else {
            panic!()
        };
        project
    }

    // Rewrite through ZIP to exercise valid CRCs with malformed derived/core data.
    fn rewrite(bytes: &[u8], name: &str, replacement: Option<&[u8]>) -> Vec<u8> {
        let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = input.file_names().map(str::to_owned).collect();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for path in names {
            let original = entry(&mut input, &path).unwrap();
            let data = if path == name {
                replacement
            } else {
                Some(original.as_slice())
            };
            if let Some(data) = data {
                output
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                output.write_all(data).unwrap();
            }
        }
        output.finish().unwrap().into_inner()
    }

    #[test]
    fn editable_document_and_library_roundtrip_with_legacy_migrations() {
        let project = project();
        assert_eq!(unpack(&project.to_archive().unwrap()), project);
        let json = project.to_json().unwrap();
        assert_eq!(unpack(json.as_bytes()), project);
        let library = OcamFile::Library(crate::ToolLibrary::defaults());
        assert_eq!(
            OcamFile::from_bytes(library.to_json().unwrap().as_bytes()).unwrap(),
            library
        );
    }

    #[test]
    fn stock_is_separate_from_manifest_and_bad_cache_does_not_lose_document() {
        let mut project = project();
        let id = project.document.setups[0].id;
        let field = cam_sim::Heightfield::new([0.; 2], [2.; 2], 0.25, 3.);
        project.toolpaths.verification.insert(
            id,
            StoredVerification {
                fingerprint: "test fingerprint".into(),
                stock_snapshot: Some(field.snapshot(-1.).unwrap()),
                ..Default::default()
            },
        );
        let bytes = project.to_archive().unwrap();
        assert_eq!(unpack(&bytes), project);
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let manifest = entry(&mut archive, "project.json").unwrap();
        let text = std::str::from_utf8(&manifest).unwrap();
        for omitted in [
            "stock_snapshot",
            "stock_vertices",
            "stock_indices",
            "initial_stock",
        ] {
            assert!(!text.contains(omitted));
        }
        let path = format!("stock/{id}/final.bin");
        for replacement in [None, Some(b"corrupt material state".as_slice())] {
            let damaged = rewrite(&bytes, &path, replacement);
            let recovered = unpack(&damaged);
            assert_eq!(recovered.document, project.document);
            assert!(recovered.toolpaths.verification.is_empty());
            assert!(read(Cursor::new(&damaged), true).is_err());
        }
        let mut manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
        manifest["archive_version"] = 999.into();
        assert!(OcamFile::from_bytes(&rewrite(
            &bytes,
            "project.json",
            Some(&serde_json::to_vec(&manifest).unwrap())
        ))
        .is_err());
        assert!(OcamFile::from_bytes(&rewrite(&bytes, "project.json", Some(b"{"))).is_err());
        assert!(OcamFile::from_bytes(&bytes[..bytes.len() / 2]).is_err());
    }

    #[test]
    fn legacy_mesh_is_preserved_but_defined_initial_blank_is_omitted() {
        let mut project = project();
        let id = project.document.setups[0].id;
        let vertex = cam_render::MeshVertex {
            position: [0.12345679, 2., -3.],
            normal: [0., 0., 1.],
            color: [0.2, 0.3, 0.4, 1.],
        };
        let legacy = StoredVerification {
            stock_vertices: vec![vertex; 3],
            stock_indices: vec![0, 1, 2],
            stock_machined: vec![true; 3],
            stock_operations: vec![Some(42); 3],
            initial_stock: Some((vec![vertex; 3], vec![0, 1, 2])),
            ..Default::default()
        };
        project.toolpaths.verification.insert(id, legacy.clone());
        let loaded = unpack(&project.to_archive().unwrap());
        let expected = StoredVerification {
            initial_stock: None,
            ..legacy
        };
        assert_eq!(loaded.toolpaths.verification[&id], expected);
        assert_eq!(unpack(&loaded.to_archive().unwrap()), loaded);
    }

    #[test]
    fn failed_cache_readback_preserves_previous_file_and_cleans_temp() {
        let dir = std::env::temp_dir().join(format!("ocam-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.ocam");
        let mut project = project();
        atomic_save(&path, &project).unwrap();
        let previous = std::fs::read(&path).unwrap();
        let id = project.document.setups[0].id;
        // Public JSON snapshot import must also be checked by strict save readback.
        let invalid = serde_json::from_value(serde_json::json!({"data": [1, 2, 3]})).unwrap();
        project.toolpaths.verification.insert(
            id,
            StoredVerification {
                stock_snapshot: Some(invalid),
                ..Default::default()
            },
        );
        assert!(atomic_save(&path, &project).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
