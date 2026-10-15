//! Native CAD translation is optional; planar/headless builds never need OCCT.
//! The bridge owns every OCCT object for one call, catches C++ exceptions and
//! returns only immutable, serializable millimetre topology. Calls are serialized.
use cam_model::{RigidTransform, SourceUnits, TopologySnapshot};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolidImportOptions {
    pub display_tolerance: f64,
    /// Fallback precision in millimetres when STEP does not specify uncertainty.
    /// Source tolerances needed for consistent topology are preserved.
    pub import_tolerance: f64,
}
impl Default for SolidImportOptions {
    fn default() -> Self {
        Self {
            display_tolerance: 0.1,
            import_tolerance: 1e-6,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportedPlacement {
    pub body_id: u64,
    pub placement: RigidTransform,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolidImport {
    pub topology: TopologySnapshot,
    pub placements: Vec<ImportedPlacement>,
    pub source_units: SourceUnits,
    pub backend: String,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelError {
    BackendUnavailable,
    UnsupportedFormat(String),
    InvalidOptions,
    Io(String),
    Translation(String),
}
impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable => f.write_str("STEP import requires the optional OCCT backend (rebuild with occt and an OCCT 7.9.3 SDK)"),
            Self::UnsupportedFormat(s) => write!(f, "unsupported solid format: {s}"),
            Self::InvalidOptions => f.write_str("CAD import tolerances must be finite and positive"),
            Self::Io(s) => write!(f, "could not read CAD file: {s}"),
            Self::Translation(s) => write!(f, "CAD translation failed: {s}"),
        }
    }
}
impl std::error::Error for KernelError {}

pub fn backend_available() -> bool {
    cfg!(occt_available)
}

pub fn read_step_file(
    path: impl AsRef<Path>,
    options: &SolidImportOptions,
) -> Result<SolidImport, KernelError> {
    if !options.display_tolerance.is_finite()
        || options.display_tolerance <= 0.0
        || !options.import_tolerance.is_finite()
        || options.import_tolerance <= 0.0
    {
        return Err(KernelError::InvalidOptions);
    }
    let path = path.as_ref();
    if !path.is_file() {
        return Err(KernelError::Io(format!(
            "{} is not a readable file",
            path.display()
        )));
    }
    #[cfg(occt_available)]
    {
        native::read(path, options)
    }
    #[cfg(not(occt_available))]
    {
        Err(KernelError::BackendUnavailable)
    }
}

#[cfg(occt_available)]
mod native {
    use super::*;
    use std::{
        ffi::{c_char, CStr, CString},
        sync::Mutex,
    };
    static CAD_SERVICE: Mutex<()> = Mutex::new(());
    extern "C" {
        fn cam_occt_step(
            path: *const c_char,
            display_tolerance: f64,
            import_tolerance: f64,
            error: *mut *mut c_char,
        ) -> *mut c_char;
        fn cam_occt_free(value: *mut c_char);
    }
    struct OwnedString(*mut c_char);
    impl Drop for OwnedString {
        fn drop(&mut self) {
            unsafe { cam_occt_free(self.0) }
        }
    }
    pub(super) fn read(
        path: &Path,
        options: &SolidImportOptions,
    ) -> Result<SolidImport, KernelError> {
        let path = CString::new(
            path.to_str()
                .ok_or_else(|| KernelError::Io("CAD path is not UTF-8".into()))?,
        )
        .map_err(|_| KernelError::Io("CAD path contains NUL".into()))?;
        let _guard = CAD_SERVICE
            .lock()
            .map_err(|_| KernelError::Translation("CAD service lock poisoned".into()))?;
        let mut error = std::ptr::null_mut();
        // SAFETY: C strings live through the call; bridge returns owned malloc
        // strings and never exposes CAD handles or throws over the FFI boundary.
        let value = OwnedString(unsafe {
            cam_occt_step(
                path.as_ptr(),
                options.display_tolerance,
                options.import_tolerance,
                &mut error,
            )
        });
        let error = OwnedString(error);
        if value.0.is_null() {
            let message = if error.0.is_null() {
                "unknown native failure".into()
            } else {
                unsafe { CStr::from_ptr(error.0) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(KernelError::Translation(message));
        }
        let mut import: SolidImport =
            serde_json::from_slice(unsafe { CStr::from_ptr(value.0) }.to_bytes())
                .map_err(|e| KernelError::Translation(format!("invalid bridge result: {e}")))?;
        if import.topology.projection.is_none() {
            import.warnings.push("XY silhouette is unavailable: CAD projection tessellation was incomplete or exceeded its resource budget. Faces remain inspectable.".into());
        }
        Ok(import)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_tolerance_precedes_translation() {
        assert_eq!(
            read_step_file(
                "missing.step",
                &SolidImportOptions {
                    display_tolerance: f64::NAN,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            KernelError::InvalidOptions
        );
    }
    #[test]
    #[cfg(not(occt_available))]
    fn native_free_build_reports_explicit_backend_error() {
        let path = std::env::temp_dir().join(format!(
            "cam-kernel-unavailable-{}.step",
            std::process::id()
        ));
        std::fs::write(&path, "ISO-10303-21;END-ISO-10303-21;").unwrap();
        assert_eq!(
            read_step_file(&path, &Default::default()).unwrap_err(),
            KernelError::BackendUnavailable
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(all(test, occt_available))]
mod multi_body_tests {
    use super::*;
    #[test]
    fn tigerente_multiple_imports_all_bodies() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/Tigerente_multiple.step");
        let import = read_step_file(path, &Default::default()).unwrap();
        eprintln!(
            "Tigerente: {} bodies, {} placements, {} faces; warnings: {:?}",
            import.topology.bodies.len(),
            import.placements.len(),
            import.topology.faces.len(),
            import.warnings
        );
        import.topology.validate().unwrap();
        let projection = import
            .topology
            .projection
            .as_ref()
            .expect("multi-body import must retain machining projection data");
        assert_eq!(projection.chord_tolerance, 0.0025);
        assert!(projection.mesh.triangles.len() <= 1_000_000);
        let covered: std::collections::BTreeSet<_> =
            projection.mesh.triangle_faces.iter().copied().collect();
        assert!(
            import
                .topology
                .faces
                .iter()
                .all(|face| covered.contains(&face.id)),
            "every face of every body must contribute to the projection"
        );
        assert!(
            !import
                .warnings
                .iter()
                .any(|warning| warning.contains("budget") || warning.contains("unavailable")),
            "{:?}",
            import.warnings
        );
        eprintln!(
            "Tigerente projection: {} triangles",
            projection.mesh.triangles.len()
        );
        assert_eq!(import.topology.bodies.len(), 5);
        assert_eq!(import.placements.len(), 5);
        for body in &import.topology.bodies {
            assert!(
                import
                    .topology
                    .faces
                    .iter()
                    .filter(|f| f.body_id == body.id)
                    .any(|face| import.topology.mesh.triangle_faces.contains(&face.id)),
                "body {} must be represented in the display mesh",
                body.id
            );
        }
        assert!(import.topology.bodies.iter().all(|body| body.valid_solid));
        assert!(import.placements.iter().all(|p| import
            .topology
            .bodies
            .iter()
            .any(|b| b.id == p.body_id)));
    }
}
