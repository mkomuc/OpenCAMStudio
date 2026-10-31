//! Optional local project fixtures used by qualification tests.
use std::path::Path;

pub(crate) fn available(name: &str) -> bool {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testfiles")
        .join(name);
    match std::fs::metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Skipping test: optional fixture {} is missing", path.display());
            false
        }
        _ => true,
    }
}
