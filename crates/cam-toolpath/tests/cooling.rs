//! Cutting lubrication through planning and Estlcam output wiring.
use cam_cldata::{SpindleDir, Step};
use cam_geo::{Contour, Point};
use cam_model::{
    Comp, Document, Heights, Lead, Operation, Plunge, ProfileOp, Setup, Side, Stock, Tool, ToolKind,
};
use cam_toolpath::{build_job, CancelToken};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Contour {
    Contour::new(vec![
        Point::new(x0, y0),
        Point::new(x1, y0),
        Point::new(x1, y1),
        Point::new(x0, y1),
    ])
}

fn profile(id: u32, chain: Contour, side: Side, spindle_rpm: f64) -> Operation {
    Operation::Profile(ProfileOp {
        passes: Default::default(),
        selections: None,
        closed: true,
        additional_chains: Vec::new(),
        tabs: Default::default(),
        spindle_rpm,
        tool_preset: None,
        cooling: Default::default(),
        entry_feeds: Default::default(),
        work_offset: 1,
        clearing: cam_model::Clearing::default(),
        id,
        tool: 1,
        chain,
        side,
        comp: Comp::Computed,
        offset: 0.0,
        depth: 4.0,
        stepdown: 2.0,
        stepover: 0.0,
        feed: 300.0,
        plunge_feed: 100.0,
        start: None,
        lead_in: Lead::None,
        lead_out: Lead::None,
        lead_overlap: 0.0,
        plunge: Plunge::Straight,
    })
}

fn doc(ops: Vec<Operation>) -> Document {
    Document::new(Setup {
        id: 0,
        frame: Default::default(),
        material: String::new(),
        material_id: None,
        model_instances: Vec::new(),
        program_name: String::new(),
        name: "spindle".into(),
        heights: Heights::new(5.0, 2.0, 0.0),
        stock: Stock::BoundingBox {
            x_offset: 0.0,
            y_offset: 0.0,
            top: 0.0,
            thickness: 10.0,
        },
        tools: vec![Tool {
            number: 1,
            diameter: 6.0,
            length: 30.0,
            flutes: 2,
            kind: ToolKind::EndMill,
            ..Default::default()
        }],
        operations: ops,
        patterns: Vec::new(),
        origin: [0.0, 0.0, 0.0],
        extra_origins: vec![],
        origin_index: 1,
        operation_heights: Default::default(),
        height_settings: Default::default(),
        tool_change_height: None,
    })
}

fn plan(operations: Vec<Operation>) -> cam_cldata::Program {
    let result = build_job(
        &doc(operations),
        1000.,
        SpindleDir::Cw,
        None,
        50.,
        &CancelToken::new(),
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == cam_toolpath::Severity::Error),
        "{:?}",
        result.diagnostics
    );
    assert!(!result.program.is_empty());
    result.program
}
fn cut(id: u32, cooling: cam_model::Cooling) -> Operation {
    let mut op = profile(id, rect(10., 10., 70., 50.), Side::Outside, 3000.);
    op.set_cooling(cooling);
    op
}

#[test]
fn mms_is_active_during_each_cut_and_off_between_operations() {
    use cam_cldata::Coolant;
    use cam_model::Cooling;
    let program = plan(vec![
        cut(1, Cooling::Mms),
        cut(2, Cooling::None),
        cut(3, Cooling::Mms),
    ]);
    let mut coolant = Coolant::Off;
    let mut seen = [false; 3];
    let mut on_count = 0;
    for step in program.steps() {
        match step {
            Step::Coolant(value) => {
                coolant = *value;
                if coolant == Coolant::Mist {
                    on_count += 1;
                }
            }
            Step::ToolChange { .. } | Step::SpindleOff | Step::Stop => {
                assert_eq!(coolant, Coolant::Off)
            }
            Step::Linear { tag, .. } | Step::Arc { tag, .. } => {
                let index = tag.op_id as usize - 1;
                seen[index] = true;
                assert_eq!(
                    coolant,
                    if index == 1 {
                        Coolant::Off
                    } else {
                        Coolant::Mist
                    }
                );
            }
            _ => {}
        }
    }
    assert_eq!(seen, [true; 3]);
    assert_eq!(on_count, 2);
    assert_eq!(coolant, Coolant::Off);
}

#[test]
fn estlcam_mms_uses_selected_outputs_and_none_never_turns_them_on() {
    use cam_model::{Cooling, Envelope, Machine, Point3};
    use cam_post::{EstlcamOptions, PostKind, PostOptions};
    let machine = Machine {
        name: "Cooling test".into(),
        rapid_rate: 2000.,
        max_spindle_rpm: 10000.,
        max_feed: 1000.,
        envelope: Envelope::new(Point3::new(0., 0., -50.), Point3::new(200., 200., 100.)),
        safe_z: 5.,
        tool_change_pos: None,
    };
    for cooling in [Cooling::None, Cooling::Mms] {
        let program = plan(vec![cut(1, cooling)]);
        for (output_1, output_2) in [(true, false), (false, true), (true, true), (false, false)] {
            let options = PostOptions {
                estlcam: EstlcamOptions { output_1, output_2 },
                ..Default::default()
            };
            let nc = PostKind::Estlcam
                .post(&program, &machine, &options)
                .unwrap();
            for (on, off, enabled) in [("M8", "M9", output_1), ("M10", "M11", output_2)] {
                let lines: Vec<_> = nc.lines().collect();
                assert_eq!(
                    lines.contains(&on),
                    enabled && cooling == Cooling::Mms,
                    "{nc}"
                );
                assert_eq!(lines.contains(&off), enabled, "{nc}");
                if enabled && cooling == Cooling::Mms {
                    let on_index = lines.iter().position(|line| *line == on).unwrap();
                    let off_index = lines
                        .iter()
                        .enumerate()
                        .skip(on_index + 1)
                        .find(|(_, line)| **line == off)
                        .unwrap()
                        .0;
                    assert!(lines[on_index + 1..off_index]
                        .iter()
                        .any(|line| line.starts_with("G1")
                            || line.starts_with("G2")
                            || line.starts_with("G3")));
                }
            }
        }
    }
}
