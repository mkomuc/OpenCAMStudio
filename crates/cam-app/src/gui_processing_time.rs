//! UI for a processing-time estimate from the most recently completed toolpath run.
use super::*;
use iced::widget::{column, row};

#[derive(Clone, Debug)]
pub(super) struct PendingProcessingTime {
    pub(super) stamp: crate::JobStamp,
    pub(super) label: String,
    pub(super) feed_override: String,
}

#[derive(Clone, Debug)]
pub(super) struct ProcessingTimeDialog {
    label: String,
    feed_override: String,
    base_estimate: Result<cam_cldata::ProcessingTime, String>,
    pub(super) estimate: Result<cam_cldata::ProcessingTime, String>,
}

impl App {
    pub(super) fn open_processing_time_dialog(&mut self, pending: PendingProcessingTime) {
        let estimate = self.processing_time_for("100");
        let base_estimate = estimate.clone();
        self.processing_time_dialog = Some(ProcessingTimeDialog {
            label: pending.label,
            feed_override: pending.feed_override,
            base_estimate,
            estimate,
        });
        self.status = "Processing time estimate ready.".into();
    }

    pub(super) fn processing_time_feed_override_changed(&mut self, value: String) {
        if let Some(dialog) = &mut self.processing_time_dialog {
            let estimate = adjusted_estimate(dialog.base_estimate.clone(), &value);
            dialog.feed_override = value;
            dialog.estimate = estimate;
        }
    }

    fn processing_time_for(
        &self,
        override_text: &str,
    ) -> Result<cam_cldata::ProcessingTime, String> {
        let percent = override_text
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter a finite feed override greater than 0%.".to_string())?;
        if !percent.is_finite() || percent <= 0.0 {
            return Err("Enter a finite feed override greater than 0%.".into());
        }
        let outcome = self
            .controller
            .outcome()
            .filter(|outcome| outcome.completion == JobCompletion::Complete)
            .ok_or_else(|| "The completed toolpath is no longer available.".to_string())?;
        outcome.program.processing_time(
            self.controller.machine().rapid_rate,
            self.controller.machine().tool_change_seconds,
            percent,
        )
    }

    pub(super) fn processing_time_overlay(&self) -> Option<Element<'_, Message>> {
        let dialog = self.processing_time_dialog.as_ref()?;
        let machine = self.controller.machine();
        let line = |name: &'static str, value: String| {
            row![
                text(name).size(13).width(Length::Fill),
                text(value).size(13).width(Length::Fixed(155.0))
            ]
            .align_y(Alignment::Center)
            .width(Length::Fill)
        };
        let mut details = column![
            text(format!("Processing time · {}", dialog.label)).size(19),
            row![
                text("Feed override (%)").size(13).width(Length::Fill),
                text_input("100", &dialog.feed_override)
                    .on_input(Message::ProcessingTimeFeedOverride)
                    .width(Length::Fixed(110.0)),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
            line(
                "Machine rapid rate",
                format!("{} mm/min", machine.rapid_rate)
            ),
            line(
                "Tool-change time per change",
                format!("{} s", machine.tool_change_seconds)
            ),
            text("Configure these machine values in Machinery.").size(11),
        ]
        .spacing(8);
        match &dialog.estimate {
            Ok(estimate) => {
                details = details
                    .push(popup_separator())
                    .push(line("Feed length", format_mm(estimate.feed_distance_mm)))
                    .push(line(
                        "Cutting time",
                        format_time(estimate.cutting_seconds()),
                    ))
                    .push(line(
                        "Ramp/helix time",
                        format_time(estimate.ramp_helix_seconds),
                    ))
                    .push(line("Plunge time", format_time(estimate.plunge_seconds)))
                    .push(line("Rapid length", format_mm(estimate.rapid_distance_mm)))
                    .push(line("Rapid time", format_time(estimate.rapid_seconds)))
                    .push(line("Dwell time", format_time(estimate.dwell_seconds)))
                    .push(line("Tool changes", estimate.tool_changes.to_string()))
                    .push(line(
                        "Tool-change time",
                        format_time(estimate.tool_change_seconds),
                    ))
                    .push(popup_separator())
                    .push(line("Total", format_time(estimate.total_seconds())));
            }
            Err(error) => {
                details = details.push(text(error.clone()).size(12).color(palette::ERROR))
            }
        }
        details = details.push(text(
            "Estimate uses nominal feeds. It excludes acceleration, operator stop duration, and unknown initial approach moves.",
        ).size(11));
        let content = column![
            scrollable(details)
                .width(Length::Fill)
                .height(Length::Fixed(
                    (self.window.height - 240.0).clamp(120.0, 440.0)
                )),
            row![
                Space::new().width(Length::Fill),
                inspector_button("Close").on_press(Message::ProcessingTimeClose),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(10);
        let card = container(content)
            .width(520)
            .padding(16)
            .style(|theme: &iced::Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.weak.color,
                )),
                border: Border {
                    color: theme.extended_palette().background.strong.color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });
        Some(
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into(),
        )
    }
}

fn format_mm(value: f64) -> String {
    format!("{value:.1} mm")
}

fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".into();
    }
    let total = seconds.round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

fn adjusted_estimate(
    estimate: Result<cam_cldata::ProcessingTime, String>,
    override_text: &str,
) -> Result<cam_cldata::ProcessingTime, String> {
    let mut estimate = estimate?;
    let percent = override_text
        .trim()
        .parse::<f64>()
        .map_err(|_| "Enter a finite feed override greater than 0%.".to_string())?;
    if !percent.is_finite() || percent <= 0.0 {
        return Err("Enter a finite feed override greater than 0%.".into());
    }
    estimate.feed_seconds *= 100.0 / percent;
    estimate.ramp_helix_seconds *= 100.0 / percent;
    estimate.plunge_seconds *= 100.0 / percent;
    if !estimate.feed_seconds.is_finite()
        || !estimate.ramp_helix_seconds.is_finite()
        || !estimate.plunge_seconds.is_finite()
        || !estimate.total_seconds().is_finite()
    {
        return Err("Feed override produces an unrepresentable estimate.".into());
    }
    Ok(estimate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_scales_only_feed_time_and_rejects_nonpositive_or_overflowing_values() {
        let base = cam_cldata::ProcessingTime {
            feed_distance_mm: 20.0,
            rapid_distance_mm: 8.0,
            feed_seconds: 10.0,
            ramp_helix_seconds: 3.0,
            plunge_seconds: 2.0,
            rapid_seconds: 4.0,
            dwell_seconds: 2.0,
            tool_changes: 2,
            tool_change_seconds: 6.0,
        };
        let half_feed = adjusted_estimate(Ok(base), "50").unwrap();
        assert_eq!(half_feed.feed_seconds, 20.0);
        assert_eq!(half_feed.ramp_helix_seconds, 6.0);
        assert_eq!(half_feed.plunge_seconds, 4.0);
        assert_eq!(half_feed.rapid_seconds, base.rapid_seconds);
        assert_eq!(half_feed.dwell_seconds, base.dwell_seconds);
        assert_eq!(half_feed.tool_change_seconds, base.tool_change_seconds);
        assert_eq!(half_feed.total_seconds(), 32.0);
        assert!(adjusted_estimate(Ok(base), "0").is_err());
        assert!(adjusted_estimate(Ok(base), "NaN").is_err());
        assert!(adjusted_estimate(Ok(base), "1e-320").is_err());
        assert_eq!(
            adjusted_estimate(Err("base calculation failed".into()), "50"),
            Err("base calculation failed".into())
        );
    }

    #[test]
    fn duration_format_keeps_hours_unbounded() {
        assert_eq!(format_time(3661.0), "01:01:01");
        assert_eq!(format_time(90_061.0), "25:01:01");
    }
}
