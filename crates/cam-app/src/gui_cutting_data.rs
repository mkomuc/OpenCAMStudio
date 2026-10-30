//! Linked, editable views of the saved RPM and cutting feed.
use super::*;

impl App {
    pub(super) fn effective_cutting_rpm(&self, rpm: f64) -> f64 {
        if rpm == 0. {
            self.controller.defaults().spindle_rpm
        } else {
            rpm
        }
    }

    pub(super) fn operation_cutting_value(&self, op: &Operation, field: Field) -> Option<f64> {
        let tool = self.operation_preset_tool()?;
        let rpm = self.effective_cutting_rpm(op.spindle_rpm());
        match field {
            Field::CuttingSpeed => tool.cutting_speed(rpm),
            Field::FeedPerTooth => tool.feed_per_tooth(rpm, op_field(op, Field::Feed)?),
            _ => None,
        }
    }

    /// Only add derived fields when their cutter data is available. This also
    /// keeps legacy cutters without a flute count editable.
    pub(super) fn add_cutting_fields(&self, op: &Operation, fields: &mut Vec<Field>) {
        for (source, derived) in [
            (Field::SpindleRpm, Field::CuttingSpeed),
            (Field::Feed, Field::FeedPerTooth),
        ] {
            if self.operation_cutting_value(op, derived).is_some() {
                if let Some(index) = fields.iter().position(|f| *f == source) {
                    fields.insert(index + 1, derived);
                }
            }
        }
    }

    pub(super) fn refresh_cutting_fields(&mut self, edited: Option<Field>) {
        let Some(tool) = self.operation_preset_tool().cloned() else {
            return;
        };
        let value = |field| {
            self.fields
                .get(&field)?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
        };
        let rpm = value(Field::SpindleRpm).map(|rpm| self.effective_cutting_rpm(rpm));
        let speed = rpm.and_then(|rpm| tool.cutting_speed(rpm));
        let tooth = rpm
            .zip(value(Field::Feed))
            .and_then(|(rpm, feed)| tool.feed_per_tooth(rpm, feed));
        for (field, value) in [(Field::CuttingSpeed, speed), (Field::FeedPerTooth, tooth)] {
            if field == Field::CuttingSpeed
                && matches!(edited, Some(Field::Feed | Field::FeedPerTooth))
            {
                continue;
            }
            if edited != Some(field) {
                if let Some(value) = value {
                    self.fields.insert(field, fmt_num(value));
                }
            }
        }
    }

    pub(super) fn edit_cutting_field(&mut self, field: Field) {
        if !matches!(
            field,
            Field::SpindleRpm | Field::Feed | Field::CuttingSpeed | Field::FeedPerTooth
        ) {
            return;
        }
        let Some(tool) = self.operation_preset_tool() else {
            return;
        };
        let value = self
            .fields
            .get(&field)
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.);
        let linked = value.and_then(|value| match field {
            Field::CuttingSpeed => tool
                .rpm_for_cutting_speed(value)
                .map(|v| (Field::SpindleRpm, v)),
            Field::FeedPerTooth => {
                let rpm = self
                    .fields
                    .get(&Field::SpindleRpm)?
                    .trim()
                    .parse::<f64>()
                    .ok()?;
                tool.feed_for_tooth(self.effective_cutting_rpm(rpm), value)
                    .map(|v| (Field::Feed, v))
            }
            _ => None,
        });
        if let Some((source, value)) = linked.filter(|(_, v)| *v > 0.) {
            self.fields.insert(source, fmt_num(value));
            if self.creating_operation() {
                self.creation_edited_fields.insert(source);
            }
        }
        self.refresh_cutting_fields(Some(field));
    }
}
