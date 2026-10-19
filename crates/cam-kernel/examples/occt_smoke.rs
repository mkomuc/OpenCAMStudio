//! Runs against the packaged runtime, with the build SDK hidden by release CI.
use cam_kernel::{read_iges_file, read_step_file, SolidImportOptions};

fn main() {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(paths.len(), 2, "expected STEP and IGES fixture paths");
    assert!(cam_kernel::backend_available(), "OCCT was not compiled in");
    let options = SolidImportOptions::default();
    for import in [
        read_step_file(&paths[0], &options).expect("packaged STEP import failed"),
        read_iges_file(&paths[1], &options).expect("packaged IGES import failed"),
    ] {
        import.topology.validate().expect("invalid imported topology");
        assert_eq!(import.topology.bodies.len(), 1);
        assert!(import.topology.bodies[0].valid_solid);
        assert_eq!(import.backend, "OCCT 7.9.3");
    }
    println!("Packaged OCCT 7.9.3 STEP/IGES imports passed");
}
