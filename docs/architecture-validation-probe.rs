// Public-API counterexamples for the 2026-10-13 architecture assessment.
// Build/run instructions are in architecture-validation.md.
// This intentionally prints observed behavior rather than changing application code.

use cam_app::{default_machine, AppController};
use cam_cldata::{ArcDir, DrillCycle, MoveKind, Point3, Program, Step, Tag};
use cam_model::{Envelope, Machine};
use cam_post::{PostKind, PostOptions};
use cam_sim::{simulate, SimOptions};
use cam_toolpath::CancelToken;

fn main() {
    let machine = Machine {
        name: "architecture probe".into(),
        rapid_rate: 2000.0,
        max_spindle_rpm: 10000.0,
        max_feed: 1000.0,
        envelope: Envelope::new(
            Point3::new(0.0, 0.0, -50.0),
            Point3::new(10.0, 10.0, 50.0),
        ),
        safe_z: 5.0,
        tool_change_pos: None,
    };
    let tag = Tag::new(0, MoveKind::Cutting);

    // Radius 20, center X21: X1..41, Y-20..20, on a 10x10 machine.
    // The full-circle endpoint equals the start, so endpoint bounds miss the sweep.
    let arc = Program {
        steps: vec![
            Step::Rapid { to: Point3::new(1.0, 0.0, 5.0), tag },
            Step::Arc {
                end: Point3::new(1.0, 0.0, 5.0),
                center: Point3::new(21.0, 0.0, 5.0),
                dir: ArcDir::Ccw,
                feed: 100.0,
                tag,
            },
        ],
    };
    println!(
        "full circle X span 40 vs travel 10: export accepted = {}",
        PostKind::Fanuc.post(&arc, &machine, &PostOptions::default()).is_ok()
    );

    let invalid = Program {
        steps: vec![Step::Linear {
            to: Point3::new(f64::NAN, 1.0, 1.0),
            feed: -10.0,
            tag,
        }],
    };
    println!(
        "NaN position and negative feed: export accepted = {}",
        PostKind::Fanuc.post(&invalid, &machine, &PostOptions::default()).is_ok()
    );

    // After drilling, the actual cutter is at (0,0,2), above stock top Z0.
    // The next rapid crosses into uncut material and ends at (8,0,-1).
    let drill = Program {
        steps: vec![
            Step::Rapid { to: Point3::new(0.0, 0.0, 5.0), tag },
            Step::Drill(DrillCycle {
                points: vec![[0.0, 0.0]],
                z_top: 0.0,
                depth: -1.0,
                retract: 2.0,
                peck: None,
                dwell: None,
                feed: 100.0,
                tag,
            }),
            Step::Rapid { to: Point3::new(8.0, 0.0, -1.0), tag },
        ],
    };
    let sim = simulate(
        &drill,
        [-2.0, -2.0, -5.0],
        [10.0, 2.0, 0.0],
        &SimOptions { resolution: 0.1, tool_radius: 0.5 },
        &[],
    );
    println!(
        "drill then lateral rapid into uncut stock: collisions = {}",
        sim.collisions.len()
    );

    let mut app = AppController::new(default_machine());
    app.open_dxf(
        include_str!("../crates/cam-import/tests/fixtures/part.dxf"),
        "architecture probe",
    ).unwrap();
    let token = CancelToken::new();
    token.cancel();
    let result = app.run(&token);
    println!(
        "pre-cancelled run: steps = {}, errors = {}",
        result.program.len(), result.has_errors()
    );
    println!(
        "pre-cancelled run: export accepted = {}",
        app.export_nc().is_ok()
    );
}
