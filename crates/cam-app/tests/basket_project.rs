use cam_app::{default_machine, AppController};
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
#[ignore = "uses the supplied local basket_1.ocam CAD project"]
fn basket_second_setup_pocket_finishing_verifies() {
    let mut app = AppController::new(default_machine());
    app.open_project(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/basket_1.ocam"),
    )
    .unwrap();
    app.activate_setup(4).unwrap();
    for fresh in [false, true] {
        if fresh {
            // A changed strategy input forces a new fragment as well as testing
            // the original project's persisted cache on the first run.
            app.edit_operation(14, |op| {
                let cam_model::Operation::Pocket(p) = op else {
                    panic!()
                };
                p.clear.feed += 1.;
            });
        }
        let run = app.run(&CancelToken::new());
        assert_eq!(
            run.completion,
            JobCompletion::Complete,
            "{:?}",
            run.diagnostics
        );
        assert!(
            run.collisions.is_empty(),
            "fresh={fresh}: {:?}",
            run.collisions
        );
    }
}
