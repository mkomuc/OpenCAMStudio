use cam_cldata::{
    ArcDir, Coolant, CutterComp, DrillCycle, MoveKind, Point3, Program, ProgramBuilder, SpindleDir,
    Step, Tag,
};
use cam_model::{Envelope, Machine, Tool, ToolKind};
use cam_post::{EstlcamOptions, EstlcamPost, Post, PostError, PostKind, PostOptions, WorkOffset};

fn machine() -> Machine {
    Machine {
        name: "Estlcam router".into(),
        rapid_rate: 2000.0,
        tool_change_seconds: 0.0,
        max_spindle_rpm: 24000.0,
        max_feed: 2000.0,
        envelope: Envelope::new(
            Point3::new(0.0, 0.0, -50.0),
            Point3::new(300.0, 180.0, 50.0),
        ),
        safe_z: 5.0,
        tool_change_pos: None,
    }
}

fn post(program: &Program, options: &PostOptions) -> String {
    PostKind::Estlcam
        .post(program, &machine(), options)
        .expect("Estlcam program")
}

fn cycle() -> DrillCycle {
    DrillCycle {
        points: vec![[10.0, 20.0], [30.0, 20.0]],
        z_top: 0.0,
        depth: -5.0,
        retract: 3.0,
        peck: Some(2.0),
        dwell: None,
        feed: 100.0,
        tag: Tag::new(1, MoveKind::Plunge),
    }
}

#[test]
fn manual_tool_changes_show_cutter_details_and_the_upcoming_operation_rpm() {
    let options = PostOptions {
        tools: vec![
            Tool {
                number: 7,
                description: "6 mm rougher".into(),
                diameter: 6.0,
                length: 40.0,
                flute_length: 18.0,
                flutes: 3,
                nominal_rpm: 8000.0,
                ..Default::default()
            },
            Tool {
                number: 2,
                description: "Ball finisher".into(),
                diameter: 4.0,
                length: 20.0,
                flutes: 2,
                kind: ToolKind::BallMill,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let program = ProgramBuilder::new()
        .tool_change(7)
        .comment("first operation")
        .spindle_on(12000.0, SpindleDir::Cw)
        .spindle_off()
        .tool_change(2)
        .spindle_on(16000.0, SpindleDir::Cw)
        .spindle_off()
        .tool_change(7)
        .spindle_on(9000.0, SpindleDir::Cw)
        .build();
    let nc = post(&program, &options);
    let changes: Vec<_> = nc.lines().filter(|l| l.contains(" M6 ")).collect();
    assert_eq!(changes, [
        "T7 M6 (T7 - 6 mm rougher - Square End Mill - D6.000 - FL18.000 - CR=0.000 - F3 - RPM 12000)",
        "T2 M6 (T2 - Ball finisher - Ball Nose End Mill - D4.000 - FL20.000 - CR=2.000 - F2 - RPM 16000)",
        "T7 M6 (T7 - 6 mm rougher - Square End Mill - D6.000 - FL18.000 - CR=0.000 - F3 - RPM 9000)",
    ]);
    assert!(nc.contains("M3 S12000\n"));
    assert!(!nc.contains("RPM 8000"));
}

#[test]
fn tool_change_names_cannot_break_out_of_the_inline_comment() {
    let options = PostOptions {
        precision: 2,
        tools: vec![Tool {
            number: 1,
            description: "⌀6 (rougher)\n)\rM3 S24000(".into(),
            diameter: 6.0,
            length: 30.0,
            flutes: 2,
            kind: ToolKind::BullNose { corner_radius: 0.5 },
            ..Default::default()
        }],
        ..Default::default()
    };
    let nc = post(&ProgramBuilder::new().tool_change(1).build(), &options);
    let line = nc.lines().find(|l| l.starts_with("T1 M6")).unwrap();
    assert_eq!(line, "T1 M6 (T1 - dia6 rougherM3 S24000 - Rounded-Edge End Mill - D6.00 - FL30.00 - CR=0.50 - F2)");
    assert_eq!(line.matches('(').count(), 1);
    assert_eq!(line.matches(')').count(), 1);
    assert!(nc.is_ascii());
    assert!(!nc.lines().any(|l| l.starts_with("M3")));
}

#[test]
fn unchanged_modal_rpm_is_shown_without_using_a_later_operations_speed() {
    let program = ProgramBuilder::new()
        .tool_change(1)
        .spindle_on(12000.0, SpindleDir::Cw)
        .tool_change(2)
        .rapid(Point3::new(10.0, 10.0, 5.0), MoveKind::Link)
        .spindle_on(9000.0, SpindleDir::Cw)
        .build();
    let nc = post(&program, &PostOptions::default());
    assert!(nc.contains("T2 M6 (T2 - RPM 12000)\n"), "{nc}");
}

#[test]
fn missing_tool_metadata_still_identifies_the_tool_without_borrowing_another_tools_rpm() {
    let program = ProgramBuilder::new()
        .tool_change(9)
        .tool_change(4)
        .spindle_on(6000.0, SpindleDir::Cw)
        .build();
    let nc = post(&program, &PostOptions::default());
    assert!(nc.contains("T9 M6 (T9)\nT4 M6 (T4 - RPM 6000)\nM3 S6000\n"));
}

#[test]
fn selectable_post_declares_its_limits_and_nc_extension() {
    assert!(PostKind::ALL.contains(&PostKind::Estlcam));
    assert_eq!(PostKind::Estlcam.to_string(), "Estlcam");
    assert_eq!(EstlcamPost.name(), "Estlcam");
    assert_eq!(PostKind::Estlcam.file_extensions(), &["nc"]);
    assert_eq!(PostKind::Estlcam.default_file_name(), "program.nc");
    let caps = EstlcamPost.capabilities();
    assert!(caps.arcs && caps.coolant && caps.tool_change);
    assert!(!caps.work_offsets && !caps.canned_drill && !caps.cutter_comp);
    assert!(PostKind::Estlcam.datum_label(1, WorkOffset::G54).is_some());
    assert!(PostKind::Estlcam.datum_label(2, WorkOffset::G54).is_none());
    assert!(PostKind::Estlcam.datum_label(1, WorkOffset::G55).is_none());
}

#[test]
fn every_motion_has_a_g_word_and_only_supported_commands_are_emitted() {
    let program = ProgramBuilder::new()
        .comment("router (job)\nM4")
        .tool_change(1)
        .spindle_on(12000.0, SpindleDir::Cw)
        .coolant(Coolant::Flood)
        .rapid(Point3::new(10.0, 10.0, 5.0), MoveKind::Link)
        .rapid(Point3::new(20.0, 10.0, 5.0), MoveKind::Link)
        .feed(300.0)
        .linear(Point3::new(20.0, 10.0, -1.0), MoveKind::Plunge)
        .linear(Point3::new(30.0, 10.0, -1.0), MoveKind::Cutting)
        .arc(
            Point3::new(40.0, 20.0, -1.0),
            Point3::new(30.0, 20.0, -1.0),
            ArcDir::Ccw,
            MoveKind::Cutting,
        )
        .arc(
            Point3::new(30.0, 30.0, -1.0),
            Point3::new(30.0, 20.0, -1.0),
            ArcDir::Ccw,
            MoveKind::Cutting,
        )
        .drill(cycle())
        .spindle_off()
        .coolant(Coolant::Off)
        .build();
    let options = PostOptions {
        estlcam: EstlcamOptions {
            output_1: true,
            output_2: true,
        },
        ..Default::default()
    };
    let nc = post(&program, &options);
    assert_eq!(
        nc,
        EstlcamPost.post(&program, &machine(), &options).unwrap()
    );
    for line in nc.lines().filter(|line| !line.starts_with('(')) {
        let words: Vec<_> = line.split_whitespace().collect();
        assert!(
            words.iter().filter(|w| w.starts_with('G')).count() <= 1,
            "{line}"
        );
        assert!(
            words.iter().filter(|w| w.starts_with('M')).count() <= 1,
            "{line}"
        );
        for word in &words {
            if word.starts_with('G') {
                assert!(matches!(*word, "G0" | "G1" | "G2" | "G3"), "{line}");
            }
            if word.starts_with('M') {
                assert!(
                    matches!(
                        *word,
                        "M0" | "M3" | "M5" | "M6" | "M8" | "M9" | "M10" | "M11"
                    ),
                    "{line}"
                );
            }
        }
        if words
            .iter()
            .any(|w| w.starts_with(['X', 'Y', 'Z', 'I', 'J']))
        {
            assert!(line.starts_with('G'), "motion missing G word: {line}");
        }
    }
    assert_eq!(nc.lines().filter(|l| l.starts_with("G3")).count(), 2);
    assert!(nc.contains("I0.000 J10.000"), "relative I/J: {nc}");
    assert_eq!(
        nc.lines()
            .filter(|l| l.starts_with("G1") && l.contains("Z-5.000"))
            .count(),
        2
    );
    assert!(nc.ends_with("M5\nM9\nM11\n"));
}

#[test]
fn outputs_can_be_enabled_independently_for_coolant_or_mms() {
    for output_1 in [false, true] {
        for output_2 in [false, true] {
            for coolant in [Coolant::Flood, Coolant::Mist] {
                let program = ProgramBuilder::new()
                    .coolant(coolant)
                    .coolant(Coolant::Off)
                    .build();
                let options = PostOptions {
                    estlcam: EstlcamOptions { output_1, output_2 },
                    ..Default::default()
                };
                let nc = post(&program, &options);
                for (on, off, enabled) in [("M8", "M9", output_1), ("M10", "M11", output_2)] {
                    assert_eq!(nc.lines().any(|l| l == on), enabled, "{nc}");
                    assert_eq!(nc.lines().any(|l| l == off), enabled, "{nc}");
                    if enabled {
                        assert!(
                            nc.find(&format!("{off}\n")).unwrap()
                                < nc.find(&format!("{on}\n")).unwrap()
                        );
                        assert!(
                            nc.rfind(&format!("{off}\n")).unwrap()
                                > nc.find(&format!("{on}\n")).unwrap()
                        );
                    }
                }
            }
        }
    }
    // Even without an explicit coolant-off, owned outputs are shut down at EOF.
    assert!(post(
        &ProgramBuilder::new().coolant(Coolant::Flood).build(),
        &PostOptions::default()
    )
    .ends_with("M5\nM9\n"));
}

#[test]
fn full_circles_and_helices_are_split_in_both_directions() {
    for dir in [ArcDir::Cw, ArcDir::Ccw] {
        for z in [0.0, -4.0] {
            let program = ProgramBuilder::new()
                .rapid(Point3::new(15.0, 20.0, 0.0), MoveKind::Link)
                .feed(300.0)
                .arc(
                    Point3::new(15.0, 20.0, z),
                    Point3::new(10.0, 20.0, 0.0),
                    dir,
                    MoveKind::Cutting,
                )
                .build();
            let nc = post(&program, &PostOptions::default());
            let g = if dir == ArcDir::Cw { "G2" } else { "G3" };
            let arcs: Vec<_> = nc.lines().filter(|l| l.starts_with(g)).collect();
            assert_eq!(arcs.len(), 2, "{nc}");
            assert!(arcs[0].contains("X5.000"), "{nc}");
            assert!(arcs[0].contains("I-5.000 J0.000"), "{nc}");
            assert!(arcs[1].contains("X15.000"), "{nc}");
            assert!(arcs[1].contains("I5.000 J0.000"), "{nc}");
            if z != 0.0 {
                assert!(arcs[0].contains("Z-2.000"), "{nc}");
                assert!(arcs[1].contains("Z-4.000"), "{nc}");
            }
        }
    }
}

#[test]
fn a_near_full_arc_that_rounds_closed_is_also_split() {
    let angle = 0.000001_f64;
    let program = ProgramBuilder::new()
        .rapid(Point3::new(10.0, 0.0, 0.0), MoveKind::Link)
        .feed(100.0)
        .arc(
            Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 0.0),
            Point3::new(0.0, 0.0, 0.0),
            ArcDir::Cw,
            MoveKind::Cutting,
        )
        .build();
    let nc = post(&program, &PostOptions::default());
    let arcs: Vec<_> = nc.lines().filter(|l| l.starts_with("G2")).collect();
    assert_eq!(arcs.len(), 2, "{nc}");
    assert!(arcs[0].contains("X-10.000"), "{nc}");
    assert!(arcs[1].contains("X10.000"), "{nc}");
}

#[test]
fn unsupported_intent_is_refused_instead_of_silently_ignored() {
    let mut dwell_cycle = cycle();
    dwell_cycle.dwell = Some(0.5);
    for step in [
        Step::Dwell { seconds: 1.0 },
        Step::Spindle {
            rpm: 1000.0,
            dir: SpindleDir::Ccw,
        },
        Step::Datum(2),
        Step::CutterComp(CutterComp::Left(1)),
        Step::CutterComp(CutterComp::Right(1)),
        Step::Drill(dwell_cycle),
    ] {
        assert!(matches!(
            PostKind::Estlcam.post(
                &Program { steps: vec![step] },
                &machine(),
                &PostOptions::default()
            ),
            Err(PostError::Unsupported(_))
        ));
    }
    assert!(PostKind::Estlcam
        .post(
            &Program::new(),
            &machine(),
            &PostOptions {
                work_offset: WorkOffset::G55,
                ..Default::default()
            }
        )
        .is_err());
    // Neutral no-op intent needs no unsupported command in the output.
    let nc = post(
        &Program {
            steps: vec![
                Step::Datum(1),
                Step::Dwell { seconds: 0.0 },
                Step::CutterComp(CutterComp::Off),
            ],
        },
        &PostOptions::default(),
    );
    assert!(!nc.contains("G4"));
    assert!(!nc.contains("G54"));
}
