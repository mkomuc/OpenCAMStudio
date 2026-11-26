use cam_app::{default_machine, AppController, OpKind};
use cam_cldata::{MoveKind, Step};
use cam_model::{Heights, Operation, Stock, Tool, ToolKind};
use cam_toolpath::{CancelToken, JobCompletion};

#[test]
fn one_and_three_teeth_generate_verify_and_post_a_long_thread() {
    for (teeth, cutting_length, expected_arcs) in [(1, 2.4, 16), (3, 6.0, 12)] {
        let mut app = AppController::new(default_machine());
        app.edit_setup(|setup| {
            setup.stock = Stock::FixedBox {
                size: [40., 40., 18.],
                model_position: [0.5, 0.5, 1.],
            };
            setup.heights = Heights::new(6., 3., 0.);
        });
        let bore_tool = app.use_tool(Tool {
            diameter: 32. - 2. * 0.541_265 * 2.,
            length: 40.,
            flute_length: 25.,
            ..Default::default()
        });
        let Operation::Drill(mut bore) = app.operation_template(OpKind::Drill, bore_tool) else {
            unreachable!()
        };
        bore.points = vec![[0., 0.]];
        bore.depth = 18.;
        bore.feed = 100.;
        app.add_operation(Operation::Drill(bore));

        let cutter = app.use_tool(Tool {
            diameter: 4.,
            length: 50.,
            flute_length: cutting_length,
            shoulder_length: 18.,
            shank_diameter: 6.,
            thread_teeth: teeth,
            kind: ToolKind::ThreadMill { pitch: Some(2.) },
            ..Default::default()
        });
        let Operation::Thread(mut thread) = app.operation_template(OpKind::Thread, cutter) else {
            unreachable!()
        };
        thread.points = vec![[0., 0.]];
        thread.major_dia = 32.;
        thread.pitch = 2.;
        thread.z_top = 0.;
        thread.z_bottom = -16.;
        thread.drill_clearance = 0.;
        thread.passes = 1;
        thread.spring_passes = 0;
        app.add_operation(Operation::Thread(thread));
        let thread_id = app.document().setup().operations.last().unwrap().id();

        let result = app.run(&CancelToken::new());
        assert_eq!(
            result.completion,
            JobCompletion::Complete,
            "{:?}",
            result.diagnostics
        );
        assert!(result.collisions.is_empty(), "{:?}", result.collisions);
        let arcs = result.program.steps().iter().filter(|step| {
            matches!(step, Step::Arc { tag, .. } if tag.op_id == thread_id && tag.kind == MoveKind::Cutting)
        });
        assert_eq!(arcs.count(), expected_arcs, "{teeth} teeth");
        assert!(result.stock_operations.contains(&Some(thread_id)));
        assert!(app.export_nc().unwrap().contains("Thread mill"));
    }
}
