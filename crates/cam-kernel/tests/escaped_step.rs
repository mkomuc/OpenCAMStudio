#![cfg(feature = "occt")]

use cam_kernel::{read_step_file, SolidImportOptions};
use std::{fs, path::Path};

#[test]
fn escaped_step_line_breaks_are_repaired_without_changing_source() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/box-mm.step");
    let source = fs::read_to_string(fixture).unwrap();
    let escaped = source.replace('\n', "\\n");
    let path = std::env::temp_dir().join(format!("cam-escaped-step-{}.step", std::process::id()));
    fs::write(&path, &escaped).unwrap();
    let repaired = read_step_file(&path, &SolidImportOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), escaped);
    fs::write(&path, source).unwrap();
    let normal = read_step_file(&path, &SolidImportOptions::default());
    fs::remove_file(path).unwrap();

    assert!(repaired
        .warnings
        .iter()
        .any(|w| w.contains("Repaired escaped STEP")));
    let normal = normal.expect("the same STEP data with line breaks must import");
    assert_eq!(repaired.topology, normal.topology);
    assert!(!normal
        .warnings
        .iter()
        .any(|w| w.contains("Repaired escaped STEP")));
    normal.topology.validate().unwrap();
    assert_eq!(normal.topology.bodies.len(), 1);
    assert!(normal.topology.bodies[0].valid_solid);
}
