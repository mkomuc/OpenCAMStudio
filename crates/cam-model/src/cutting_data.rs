//! Metric cutting-data conversions. RPM and feed remain the saved source values.
use crate::Tool;

fn nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.).then_some(value)
}

impl Tool {
    /// Surface cutting speed in m/min, using the cutter's nominal diameter in mm.
    pub fn cutting_speed(&self, rpm: f64) -> Option<f64> {
        if !self.diameter.is_finite() || self.diameter <= 0. {
            return None;
        }
        nonnegative(nonnegative(rpm)? * std::f64::consts::PI * self.diameter / 1000.)
    }

    pub fn rpm_for_cutting_speed(&self, speed: f64) -> Option<f64> {
        if !self.diameter.is_finite() || self.diameter <= 0. {
            return None;
        }
        nonnegative(nonnegative(speed)? / (std::f64::consts::PI * self.diameter / 1000.))
    }

    /// Chip load in mm/tooth, from cutting feed in mm/min.
    pub fn feed_per_tooth(&self, rpm: f64, feed: f64) -> Option<f64> {
        if !rpm.is_finite() || rpm <= 0. || self.flutes == 0 {
            return None;
        }
        nonnegative(nonnegative(feed)? / rpm / f64::from(self.flutes))
    }

    pub fn feed_for_tooth(&self, rpm: f64, chip_load: f64) -> Option<f64> {
        if !rpm.is_finite() || rpm <= 0. || self.flutes == 0 {
            return None;
        }
        nonnegative(nonnegative(chip_load)? * f64::from(self.flutes) * rpm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_cutting_data_round_trips() {
        let tool = Tool {
            diameter: 6.,
            flutes: 2,
            ..Default::default()
        };
        let speed = tool.cutting_speed(24000.).unwrap();
        assert!((speed - 452.3893421169302).abs() < 1e-10);
        assert!((tool.rpm_for_cutting_speed(speed).unwrap() - 24000.).abs() < 1e-9);
        assert_eq!(tool.feed_per_tooth(24000., 3000.), Some(0.0625));
        assert_eq!(tool.feed_for_tooth(24000., 0.0625), Some(3000.));
    }

    #[test]
    fn unusable_geometry_and_nonfinite_values_do_not_produce_cutting_data() {
        let mut tool = Tool {
            diameter: 6.,
            flutes: 2,
            ..Default::default()
        };
        for value in [f64::NAN, f64::INFINITY, -1.] {
            assert_eq!(tool.cutting_speed(value), None);
            assert_eq!(tool.rpm_for_cutting_speed(value), None);
            assert_eq!(tool.feed_per_tooth(value, 3000.), None);
            assert_eq!(tool.feed_for_tooth(24000., value), None);
        }
        assert_eq!(tool.feed_per_tooth(0., 3000.), None);
        tool.flutes = 0;
        assert_eq!(tool.feed_per_tooth(24000., 3000.), None);
        assert_eq!(tool.feed_for_tooth(24000., 0.1), None);
        tool.diameter = 0.;
        assert_eq!(tool.cutting_speed(24000.), None);
        assert_eq!(tool.rpm_for_cutting_speed(100.), None);
    }
}
