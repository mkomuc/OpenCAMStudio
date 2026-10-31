#![cfg(feature = "occt")]

use cam_kernel::{read_step_file, SolidImportOptions};
use std::{fs, path::Path};

#[test]
fn literal_backslash_n_separators_are_not_step_line_breaks() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/box-mm.step");
    let source = fs::read_to_string(fixture).unwrap();
    let escaped = source.replace('\n', "\\n");
    let path = std::env::temp_dir().join(format!("cam-escaped-step-{}.step", std::process::id()));
    fs::write(&path, escaped).unwrap();
    let malformed = read_step_file(&path, &SolidImportOptions::default());
    fs::write(&path, source).unwrap();
    let normal = read_step_file(&path, &SolidImportOptions::default());
    fs::remove_file(path).unwrap();

    assert!(
        malformed.is_err(),
        "escaped separators unexpectedly imported"
    );
    let normal = normal.expect("the same STEP data with line breaks must import");
    normal.topology.validate().unwrap();
    assert_eq!(normal.topology.bodies.len(), 1);
    assert!(normal.topology.bodies[0].valid_solid);
}
