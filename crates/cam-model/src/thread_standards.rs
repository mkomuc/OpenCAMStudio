//! Cylindrical 60-degree thread presets, independent of tools and GUI state.
//! Series/pitches: Guhring thread-milling technical chart and OSG's cut-tap
//! chart; provenance and supported range are recorded in docs/thread-presets.md.
//! Preparation diameters are matching hints, not tolerance-class certification.
use std::{fmt, sync::OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSeries {
    MetricCoarse,
    MetricFine,
    Unc,
    Unf,
}
impl fmt::Display for ThreadSeries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MetricCoarse => "Metric coarse",
            Self::MetricFine => "Metric fine",
            Self::Unc => "UNC",
            Self::Unf => "UNF",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadStandard {
    pub series: ThreadSeries,
    pub major_mm: f64,
    pub pitch_mm: f64,
    /// Inch size label; empty for metric.
    pub inch_size: &'static str,
}
impl ThreadStandard {
    pub fn basic_minor_mm(self) -> f64 {
        self.major_mm - 1.082_53 * self.pitch_mm
    }
    /// Approximate cut-thread preparation: metric D-P, Unified 75% thread.
    pub fn preparation_mm(self) -> f64 {
        // Common published drill sizes differ slightly from D-P. Keep these
        // coarse-series entries exact so a 6.8 mm M8 bore is not outranked by
        // an unrelated inch thread just because of that approximation.
        if self.series == ThreadSeries::MetricCoarse {
            for (major, drill) in [(4.5, 3.7), (8., 6.8), (9., 7.8), (12., 10.2)] {
                if self.major_mm == major {
                    return drill;
                }
            }
        }
        if self.series == ThreadSeries::MetricFine {
            for (major, pitch, drill) in [
                (6., 0.75, 5.2),
                (7., 0.75, 6.2),
                (8., 0.75, 7.2),
                (9., 0.75, 8.2),
                (10., 0.75, 9.2),
                (11., 0.75, 10.2),
                (10., 1.25, 8.8),
                (12., 1.25, 10.8),
                (14., 1.25, 12.8),
                (16., 1.25, 14.8),
            ] {
                if self.major_mm == major && self.pitch_mm == pitch {
                    return drill;
                }
            }
        }
        self.major_mm
            - self.pitch_mm
                * if self.inch_size.is_empty() {
                    1.
                } else {
                    0.974_25
                }
    }
    pub fn matches_parameters(self, major: f64, pitch: f64) -> bool {
        (self.major_mm - major).abs() < 1e-6 && (self.pitch_mm - pitch).abs() < 1e-6
    }
    pub fn apply(self, operation: &mut crate::ThreadOp) {
        operation.major_dia = self.major_mm;
        operation.pitch = self.pitch_mm;
    }
}
impl fmt::Display for ThreadStandard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inch_size.is_empty() {
            write!(
                f,
                "M{} × {} · {}",
                self.major_mm, self.pitch_mm, self.series
            )
        } else {
            write!(
                f,
                "{}–{} {}",
                self.inch_size,
                (25.4 / self.pitch_mm).round(),
                self.series
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThreadDiameterBasis {
    #[default]
    PreparedBore,
    Nominal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadDiameterMatch {
    pub standard: ThreadStandard,
    pub error_mm: f64,
}

/// Close candidates only. Prepared bores may follow the basic minor diameter or
/// customary cut-thread preparation. External bosses always match major diameter.
/// The proximity budget scales with pitch (0.025–0.15 mm); it is deliberately
/// independent of thread fit classes and cannot validate a machined thread.
pub fn matching_thread_standards(
    diameter_mm: f64,
    internal: bool,
    basis: ThreadDiameterBasis,
    series: Option<ThreadSeries>,
) -> Vec<ThreadDiameterMatch> {
    if !diameter_mm.is_finite() || diameter_mm <= 0. {
        return Vec::new();
    }
    let mut matches: Vec<_> = thread_standards()
        .iter()
        .copied()
        .filter(|s| series.is_none_or(|series| s.series == series))
        .filter_map(|standard| {
            let error = if internal && basis == ThreadDiameterBasis::PreparedBore {
                if diameter_mm >= standard.major_mm {
                    return None;
                }
                (diameter_mm - standard.preparation_mm())
                    .abs()
                    .min((diameter_mm - standard.basic_minor_mm()).abs())
            } else {
                (diameter_mm - standard.major_mm).abs()
            };
            (error <= (standard.pitch_mm * 0.15).clamp(0.025, 0.15) + 1e-9).then_some(
                ThreadDiameterMatch {
                    standard,
                    error_mm: error,
                },
            )
        })
        .collect();
    matches.sort_by(|a, b| {
        a.error_mm
            .total_cmp(&b.error_mm)
            .then_with(|| {
                let priority = |series| match series {
                    ThreadSeries::MetricCoarse => 0,
                    ThreadSeries::MetricFine => 1,
                    ThreadSeries::Unc => 2,
                    ThreadSeries::Unf => 3,
                };
                priority(a.standard.series).cmp(&priority(b.standard.series))
            })
            .then_with(|| a.standard.major_mm.total_cmp(&b.standard.major_mm))
            .then_with(|| b.standard.pitch_mm.total_cmp(&a.standard.pitch_mm))
    });
    matches
}

pub fn thread_standards() -> &'static [ThreadStandard] {
    static CATALOG: OnceLock<Vec<ThreadStandard>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut catalog = Vec::new();
        for &(series, rows) in &[
            (ThreadSeries::MetricCoarse, METRIC_COARSE),
            (ThreadSeries::MetricFine, METRIC_FINE),
        ] {
            catalog.extend(rows.iter().map(|&(major_mm, pitch_mm)| ThreadStandard {
                series,
                major_mm,
                pitch_mm,
                inch_size: "",
            }));
        }
        for &(inch_size, diameter_inches, coarse, fine) in UNIFIED {
            for (series, tpi) in [(ThreadSeries::Unc, coarse), (ThreadSeries::Unf, fine)] {
                if tpi > 0. {
                    catalog.push(ThreadStandard {
                        series,
                        // These catalog inch diameters have exact decimal-mm
                        // values. Avoid exposing 11.112499999999999 for 7/16.
                        major_mm: (diameter_inches * 25_400_000.).round() / 1_000_000.,
                        pitch_mm: 25.4 / tpi,
                        inch_size,
                    });
                }
            }
        }
        catalog
    })
}

const METRIC_COARSE: &[(f64, f64)] = &[
    (1., 0.25),
    (1.2, 0.25),
    (1.4, 0.3),
    (1.6, 0.35),
    (1.8, 0.35),
    (2., 0.4),
    (2.5, 0.45),
    (3., 0.5),
    (3.5, 0.6),
    (4., 0.7),
    (4.5, 0.75),
    (5., 0.8),
    (6., 1.),
    (7., 1.),
    (8., 1.25),
    (9., 1.25),
    (10., 1.5),
    (11., 1.5),
    (12., 1.75),
    (14., 2.),
    (16., 2.),
    (18., 2.5),
    (20., 2.5),
    (22., 2.5),
    (24., 3.),
    (27., 3.),
    (30., 3.5),
    (33., 3.5),
    (36., 4.),
    (39., 4.),
    (42., 4.5),
    (45., 4.5),
    (48., 5.),
    (52., 5.),
    (56., 5.5),
    (60., 5.5),
    (64., 6.),
];
const METRIC_FINE: &[(f64, f64)] = &[
    (2., 0.25),
    (2.5, 0.35),
    (3., 0.35),
    (3.5, 0.35),
    (4., 0.5),
    (4.5, 0.5),
    (5., 0.5),
    (5.5, 0.5),
    (6., 0.75),
    (7., 0.75),
    (8., 0.5),
    (8., 0.75),
    (8., 1.),
    (9., 0.75),
    (9., 1.),
    (10., 0.75),
    (10., 1.),
    (10., 1.25),
    (11., 1.),
    (11., 0.75),
    (12., 1.),
    (12., 1.25),
    (12., 1.5),
    (14., 1.),
    (14., 1.25),
    (14., 1.5),
    (15., 1.),
    (15., 1.5),
    (16., 1.),
    (16., 1.25),
    (16., 1.5),
    (17., 1.),
    (17., 1.5),
    (18., 1.),
    (18., 1.5),
    (20., 1.),
    (20., 1.5),
    (20., 2.),
    (22., 1.),
    (22., 1.5),
    (22., 2.),
    (24., 1.),
    (24., 1.5),
    (24., 2.),
    (25., 1.),
    (25., 1.5),
    (25., 2.),
    (27., 1.),
    (27., 1.5),
    (27., 2.),
    (28., 1.5),
    (28., 2.),
    (30., 1.5),
    (30., 2.),
    (30., 3.),
    (32., 1.5),
    (32., 2.),
    (33., 1.5),
    (33., 2.),
    (33., 3.),
    (34., 2.),
    (35., 1.5),
    (36., 1.5),
    (36., 2.),
    (36., 3.),
    (39., 1.5),
    (39., 2.),
    (39., 3.),
    (42., 1.5),
    (42., 2.),
    (42., 3.),
    (45., 1.5),
    (45., 2.),
    (45., 3.),
    (48., 1.5),
    (48., 2.),
    (48., 3.),
    (52., 1.5),
    (52., 2.),
    (52., 3.),
    (52., 4.),
    (56., 2.),
    (56., 3.),
    (56., 4.),
    (60., 2.),
    (60., 3.),
    (60., 4.),
    (64., 2.),
    (64., 3.),
    (64., 4.),
];
/// (designation, major inches, UNC TPI, UNF TPI); zero means no entry.
const UNIFIED: &[(&str, f64, f64, f64)] = &[
    ("#0", 0.060, 0., 80.),
    ("#1", 0.073, 64., 72.),
    ("#2", 0.086, 56., 64.),
    ("#3", 0.099, 48., 56.),
    ("#4", 0.112, 40., 48.),
    ("#5", 0.125, 40., 44.),
    ("#6", 0.138, 32., 40.),
    ("#8", 0.164, 32., 36.),
    ("#10", 0.190, 24., 32.),
    ("#12", 0.216, 24., 28.),
    ("1/4", 0.25, 20., 28.),
    ("5/16", 0.3125, 18., 24.),
    ("3/8", 0.375, 16., 24.),
    ("7/16", 0.4375, 14., 20.),
    ("1/2", 0.5, 13., 20.),
    ("9/16", 0.5625, 12., 18.),
    ("5/8", 0.625, 11., 18.),
    ("3/4", 0.75, 10., 16.),
    ("7/8", 0.875, 9., 14.),
    ("1", 1., 8., 12.),
    ("1-1/8", 1.125, 7., 12.),
    ("1-1/4", 1.25, 7., 12.),
    ("1-3/8", 1.375, 6., 12.),
    ("1-1/2", 1.5, 6., 12.),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_bores_match_coarse_fine_and_unified_threads() {
        for (bore, series, major, pitch) in [
            (5., ThreadSeries::MetricCoarse, 6., 1.),
            (6.8, ThreadSeries::MetricCoarse, 8., 1.25),
            (9., ThreadSeries::MetricFine, 10., 1.),
            (30., ThreadSeries::MetricFine, 32., 2.),
            (0.2013 * 25.4, ThreadSeries::Unc, 6.35, 25.4 / 20.),
            (0.2152 * 25.4, ThreadSeries::Unf, 6.35, 25.4 / 28.),
        ] {
            let matches = matching_thread_standards(
                bore,
                true,
                ThreadDiameterBasis::PreparedBore,
                Some(series),
            );
            assert!(
                matches[0].standard.matches_parameters(major, pitch),
                "{bore}: {matches:?}"
            );
        }
    }
    #[test]
    fn nominal_bores_and_bosses_use_major_diameter_and_keep_alternatives() {
        for internal in [false, true] {
            let candidates =
                matching_thread_standards(10.02, internal, ThreadDiameterBasis::Nominal, None);
            assert!(candidates
                .iter()
                .any(|m| m.standard.matches_parameters(10., 1.5)));
            assert!(candidates
                .iter()
                .any(|m| m.standard.matches_parameters(10., 1.)));
        }
        assert!(
            matching_thread_standards(123., true, ThreadDiameterBasis::PreparedBore, None)
                .is_empty()
        );
        assert!(
            matching_thread_standards(f64::NAN, true, ThreadDiameterBasis::Nominal, None)
                .is_empty()
        );
    }

    #[test]
    fn common_metric_bores_win_the_all_series_suggestion() {
        for (bore, major, pitch) in [(5., 6., 1.), (6.8, 8., 1.25), (30., 32., 2.)] {
            let candidates =
                matching_thread_standards(bore, true, ThreadDiameterBasis::PreparedBore, None);
            assert!(
                candidates[0].standard.matches_parameters(major, pitch),
                "{candidates:?}"
            );
        }
    }
    #[test]
    fn catalogue_is_unique_and_presets_only_set_thread_size() {
        for (index, s) in thread_standards().iter().enumerate() {
            assert!(s.basic_minor_mm() > 0. && s.preparation_mm() < s.major_mm);
            assert!(!thread_standards()[..index].contains(s));
        }
        let mut op = crate::ThreadOp {
            id: 1,
            tool: 1,
            points: vec![[0., 0.]],
            internal: true,
            hand: crate::Hand::Left,
            major_dia: 6.,
            pitch: 1.,
            z_top: 0.,
            z_bottom: -5.,
            climb: true,
            passes: 3,
            gradual: false,
            spring_passes: 0,
            drill_clearance: 0.,
            blind_allowance: 0.,
            spindle_rpm: 0.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            feed: 231.,
            plunge_feed: 80.,
        };
        let standard = matching_thread_standards(
            5.,
            true,
            ThreadDiameterBasis::PreparedBore,
            Some(ThreadSeries::MetricCoarse),
        )[0]
        .standard;
        standard.apply(&mut op);
        assert!(standard.matches_parameters(op.major_dia, op.pitch));
        assert_eq!((op.hand, op.feed, op.passes), (crate::Hand::Left, 231., 3));
    }
}
