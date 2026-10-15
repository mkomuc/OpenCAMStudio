use cam_app::{default_machine, AppController};
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
#[ignore = "requires local testfiles/test_project.ocam"]
fn saved_tigerente_face_12_can_be_planned() {
    let mut app = AppController::new(default_machine());
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/test_project.ocam");
    app.open_project(path).unwrap();
    let outcome = app.run(&CancelToken::new());
    assert_eq!(
        outcome.completion,
        JobCompletion::Complete,
        "{:?}",
        outcome.diagnostics
    );
    assert!(!outcome.program.is_empty());
    assert!(outcome.collisions.is_empty(), "{:?}", outcome.collisions);

    // Recreate the reported setup: distances above zero were entered as though
    // they were offsets above the stock. Keep rejecting this unsafe setup and
    // explain the absolute heights needed to correct it.
    app.edit_heights(|heights| {
        heights.clearance = 5.0;
        heights.retract = 2.0;
        heights.top_of_stock = 0.0;
    });
    let outcome = app.run(&CancelToken::new());
    assert_eq!(outcome.completion, JobCompletion::Failed);
    let error = outcome
        .diagnostics
        .iter()
        .find(|d| d.op == Some(0))
        .unwrap();
    for expected in [
        "clearance Z5.000",
        "retract Z2.000",
        "top Z31.000",
        "absolute Z heights",
    ] {
        assert!(
            error
                .message
                .to_lowercase()
                .contains(&expected.to_lowercase()),
            "{}",
            error.message
        );
    }
}
