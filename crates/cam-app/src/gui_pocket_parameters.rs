//! Pocket finishing controls; all cutter compensation is computed by the strategy.
use super::*;
use cam_model::PocketFinishing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Number {
    Count,
    Stepover,
    Feed,
    Overlap,
}
impl Number {
    pub const ALL: [Self; 4] = [Self::Count, Self::Stepover, Self::Feed, Self::Overlap];
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "Number of finishing passes",
            Self::Stepover => "Finishing stepover (mm)",
            Self::Feed => "Finishing feed (mm/min, 0=same)",
            Self::Overlap => "Finishing overlap (mm)",
        }
    }
    pub fn help(self) -> &'static str {
        match self {
            Self::Count => "Radial finishing cuts on the pocket walls and islands after roughing at each depth. Includes the final wall pass.",
            Self::Stepover => "Material reserved for each finishing cut. Roughing leaves the pass count times this stepover in addition to the wall stock to leave.",
            Self::Feed => "Cutting feed for finishing passes and the repeat pass. Zero uses the pocket cutting feed. Entry feed overrides still apply.",
            Self::Overlap => "Extra cutting distance past each finishing loop's start to blend the closure mark.",
        }
    }
    pub fn get(self, p: &PocketFinishing) -> f64 {
        match self {
            Self::Count => p.count as f64,
            Self::Stepover => p.stepover,
            Self::Feed => p.feed,
            Self::Overlap => p.overlap,
        }
    }
    pub fn set(self, p: &mut PocketFinishing, value: f64) {
        match self {
            Self::Count => p.count = value as u32,
            Self::Stepover => p.stepover = value,
            Self::Feed => p.feed = value,
            Self::Overlap => p.overlap = value,
        }
    }
    pub fn invalid(self, value: f64) -> bool {
        !value.is_finite()
            || match self {
                Self::Count => value < 1. || value > 1000. || value.fract() != 0.,
                Self::Stepover => value <= 0.,
                Self::Feed | Self::Overlap => value < 0.,
            }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Toggle {
    Enabled,
    AllLeads,
    Repeat,
}
impl Toggle {
    pub fn set(self, p: &mut PocketFinishing, value: bool) {
        match self {
            Self::Enabled => p.enabled = value,
            Self::AllLeads => p.lead_in_out_all = value,
            Self::Repeat => p.repeat_pass = value,
        }
    }
}

impl App {
    pub(super) fn pocket_finishing_controls(
        &self,
        p: &PocketFinishing,
    ) -> operation_sections::Form<'_> {
        let mut list = operation_sections::Form::new(self, OperationTab::Passes, None)
            .optional_section(
                "Finishing passes",
                p.enabled,
                Some(|value| Message::PocketFinishToggle(Toggle::Enabled, value)),
            );
        if !p.enabled {
            return list;
        }
        let number = |n: Number| {
            let field = Field::PocketFinish(n);
            let value = self
                .fields
                .get(&field)
                .cloned()
                .unwrap_or_else(|| fmt_num(n.get(p)));
            field_row_labeled(
                field,
                n.label(),
                n.help(),
                &value,
                self.tooltips,
                self.field_invalid(field),
            )
        };
        let toggle = |label, help, value, t| -> Element<'_, Message> {
            row![
                label_help(label, help, self.tooltips),
                checkbox(value)
                    .size(15)
                    .on_toggle(move |value| Message::PocketFinishToggle(t, value))
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        };
        list = list
            .push(number(Number::Count))
            .push(number(Number::Stepover));
        list = list.push(toggle("Lead-in/out for all finishing passes",
            "Apply the leads selected in Linking to every finishing pass. Otherwise only the first pass leads in and the last pass leads out. Disconnected loops retract to clearance.",
            p.lead_in_out_all, Toggle::AllLeads));
        list = list.push(number(Number::Feed));
        list = list.push(toggle("Repeat finishing pass",
            "Repeat the final wall pass once at the same size and finishing feed to remove material left by tool deflection.",
            p.repeat_pass, Toggle::Repeat));
        list.push(number(Number::Overlap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> (App, u32) {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "pocket.dxf").unwrap();
        app.controller.new_operation(OpKind::Pocket);
        let id = app
            .controller
            .inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        (app, id)
    }

    #[test]
    fn finishing_draft_keeps_settings_commits_and_undoes_atomically() {
        let (mut app, id) = editor();
        let before = app.controller.operation(id).unwrap().clone();
        assert!(!operation_fields(&before)
            .iter()
            .any(|f| matches!(f, Field::PocketFinish(_))));
        let _ = app.update(Message::PocketFinishToggle(Toggle::Enabled, true));
        for (number, value) in [
            (Number::Count, "3"),
            (Number::Stepover, "0.4"),
            (Number::Feed, "123"),
            (Number::Overlap, "0.2"),
        ] {
            let _ = app.update(Message::FieldChanged(
                Field::PocketFinish(number),
                value.into(),
            ));
        }
        let _ = app.update(Message::PocketFinishToggle(Toggle::AllLeads, true));
        let _ = app.update(Message::PocketFinishToggle(Toggle::Repeat, true));
        let _ = app.update(Message::PocketFinishToggle(Toggle::Enabled, false));
        let _ = app.update(Message::PocketFinishToggle(Toggle::Enabled, true));
        assert_eq!(app.fields[&Field::PocketFinish(Number::Count)], "3");
        assert_eq!(app.fields[&Field::PocketFinish(Number::Feed)], "123");
        let _ = app.update(Message::OperationSectionToggled(
            operation_sections::Section::new(OperationTab::Passes, "Finishing passes"),
        ));
        assert_eq!(app.controller.operation(id), Some(&before));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Linking));
        let _ = app.update(Message::Apply);
        let Some(Operation::Pocket(op)) = app.controller.operation(id) else {
            panic!("expected pocket");
        };
        assert_eq!(
            op.finishing,
            PocketFinishing {
                enabled: true,
                count: 3,
                stepover: 0.4,
                feed: 123.,
                overlap: 0.2,
                repeat_pass: true,
                lead_in_out_all: true,
            }
        );
        assert!(app.controller.undo());
        assert_eq!(app.controller.operation(id), Some(&before));
    }

    #[test]
    fn invalid_finishing_fields_block_apply_and_cancel_preserves_operation() {
        let (mut app, id) = editor();
        let before = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::PocketFinishToggle(Toggle::Enabled, true));
        for (number, value) in [
            (Number::Count, "1.5"),
            (Number::Count, "0"),
            (Number::Stepover, "0"),
            (Number::Feed, "-1"),
            (Number::Overlap, "NaN"),
        ] {
            let _ = app.update(Message::FieldChanged(
                Field::PocketFinish(number),
                value.into(),
            ));
            assert!(app.field_invalid(Field::PocketFinish(number)));
            assert!(app.any_field_invalid());
            let _ = app.update(Message::Apply);
            assert_eq!(app.controller.operation(id), Some(&before));
            let valid = if number == Number::Count { "1" } else { "0.6" };
            let _ = app.update(Message::FieldChanged(
                Field::PocketFinish(number),
                valid.into(),
            ));
        }
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.controller.operation(id), Some(&before));
    }
}
