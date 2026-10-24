use cam_cldata::Point3;
use cam_model::{Envelope, Machine};

fn machine() -> Machine {
    Machine {
        name: "test".into(),
        rapid_rate: 1000.0,
        tool_change_seconds: 0.0,
        max_spindle_rpm: 10000.0,
        max_feed: 500.0,
        envelope: Envelope::new(
            Point3::new(-100.0, -100.0, -100.0),
            Point3::new(100.0, 100.0, 100.0),
        ),
        safe_z: 5.0,
        tool_change_pos: Some(Point3::new(0.0, 0.0, 90.0)),
    }
}

#[test]
fn machine_numeric_limits_are_finite_and_positive() {
    machine().validate().unwrap();
    let cases: [fn(&mut Machine); 11] = [
        |m| m.tool_change_seconds = -0.01,
        |m| m.tool_change_seconds = f64::INFINITY,
        |m| m.rapid_rate = 0.0,
        |m| m.max_feed = -1.0,
        |m| m.max_spindle_rpm = f64::NAN,
        |m| m.envelope.min.x = f64::INFINITY,
        |m| m.envelope.max.y = m.envelope.min.y,
        |m| m.envelope.max.z = m.envelope.min.z - 1.0,
        |m| m.safe_z = f64::NAN,
        |m| m.tool_change_pos.as_mut().unwrap().z = f64::INFINITY,
        |m| {
            m.envelope.min.x = -f64::MAX;
            m.envelope.max.x = f64::MAX;
        },
    ];
    for mutate in cases {
        let mut m = machine();
        mutate(&mut m);
        assert!(m.validate().is_err());
    }
}
