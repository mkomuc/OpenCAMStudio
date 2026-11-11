//! Generate a reviewable wrapped pocket and LinuxCNC program without a GUI.
use cam_app::{default_machine, AppController, OpKind};
use cam_geo::{Contour, Point};
use cam_model::{Heights, Operation, RotaryWrap, Stock, StockAxis, StockOriginAxis};
use cam_post::PostKind;
use cam_toolpath::{CancelToken, JobCompletion};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("rotary-wrap.ocam"));
    let mut app = AppController::new(default_machine());
    app.set_post_kind(PostKind::LinuxCnc);
    app.edit_setup(|s| {
        s.name = "Fourth-axis wrapped pocket".into();
        s.heights = Heights::new(8., 3., 0.);
        s.origin = [0.; 3];
        s.stock = Stock::FixedRound {
            diameter: 50.,
            inner_diameter: 0.,
            length: 80.,
            position: 0.5,
            tube: false,
            axis: Some(StockAxis::Origin {
                origin_axis: StockOriginAxis::X,
            }),
        };
    });
    app.edit_tool(0, |t| {
        t.diameter = 4.;
        t.flute_length = 15.;
    });
    let mut operation = app.operation_template(OpKind::Pocket, 1);
    let Operation::Pocket(p) = &mut operation else {
        unreachable!()
    };
    p.boundary = Contour::new(vec![
        Point::new(-25., -18.),
        Point::new(25., -18.),
        Point::new(25., 18.),
        Point::new(-25., 18.),
    ]);
    p.depth = 3.;
    p.clear.stepdown = 1.;
    p.clear.feed = 600.;
    p.clear.plunge_feed = 150.;
    p.wrap = Some(RotaryWrap {
        radius: 25.,
        ..Default::default()
    });
    app.add_operation(operation);
    let outcome = app.run(&CancelToken::new());
    if outcome.completion != JobCompletion::Complete {
        return Err(format!("Generation failed: {:?}", outcome.diagnostics).into());
    }
    let stock_triangles = outcome.stock_indices.len() / 3;
    app.save_project(&destination)?;
    let nc = destination.with_extension("nc");
    app.export_nc_to(&nc)?;
    println!("Saved {} and {}", destination.display(), nc.display());
    println!(
        "Verified cylindrical stock simulation: {} triangles.",
        stock_triangles
    );
    Ok(())
}
