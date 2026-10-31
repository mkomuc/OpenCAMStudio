//! Live, nominal cutting loads. Custom engagement is a session-only what-if input.
use super::*;
use cam_model::{CuttingEngagement, CuttingForceEstimate, Tool};
use iced::widget::column;

pub(super) fn parse_material_kc(value: &str) -> Result<Option<f64>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.)
        .map(Some)
        .ok_or_else(|| {
            "Specific cutting force must be finite and positive, or blank (N/mm²).".into()
        })
}

fn parse_engagement(inputs: &[String; 3]) -> Result<CuttingEngagement, String> {
    let mut values = [0.; 3];
    for (index, input) in inputs.iter().enumerate() {
        values[index] = input
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("Enter finite positive values for all three engagement dimensions.")?;
    }
    let engagement = CuttingEngagement {
        axial_depth_mm: values[0],
        radial_width_mm: values[1],
        cutting_diameter_mm: values[2],
    };
    engagement.validate()?;
    Ok(engagement)
}

impl App {
    pub(super) fn cutting_force_key(&self, id: u32) -> (u64, Option<u32>) {
        (
            self.controller.inspection_setup().id,
            (!self.creating_operation()).then_some(id),
        )
    }

    pub(super) fn edit_material_cutting_force(&mut self, id: u64, value: String) {
        let parsed = parse_material_kc(&value);
        self.material_force_inputs.insert(id, value);
        match parsed {
            Ok(kc) => {
                if let Some(material) = self.library.materials.iter_mut().find(|m| m.id == id) {
                    material.specific_cutting_force = kc;
                    self.library.save();
                    if let Some(player) = &mut self.simulation {
                        player.sync_force_material(&self.library);
                    }
                    self.status = "Material cutting-force coefficient saved.".into();
                }
            }
            Err(error) => self.status = error,
        }
    }

    fn force_tool(&self, op: &Operation) -> Option<&Tool> {
        self.operation_tools
            .iter()
            .chain(self.controller.inspection_setup().tools.iter())
            .find(|tool| tool.number == op.tool())
    }

    fn force_operation(&self, op: &Operation) -> Operation {
        let mut draft = op.clone();
        let fields = self
            .fields
            .iter()
            .filter_map(|(field, text)| {
                text.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| (*field, v))
            })
            .collect();
        apply_op_fields(&mut draft, &fields);
        draft
    }

    fn nominal_force_engagement(
        &self,
        op: &Operation,
        tool: &Tool,
    ) -> Result<(CuttingEngagement, &'static str), String> {
        let heights = self.resolve_height_editor(op.id())?;
        op.nominal_cutting_engagement(tool, heights.top - heights.bottom)
    }

    pub(super) fn toggle_custom_cutting_force(&mut self, id: u32, enabled: bool) {
        let key = self.cutting_force_key(id);
        if !enabled {
            self.cutting_force_inputs.remove(&key);
            return;
        }
        let creating = self
            .creating_operation()
            .then(|| self.creation_operation().ok())
            .flatten();
        let Some(op) = creating
            .as_ref()
            .map(|(op, _)| op)
            .or_else(|| self.current_operation(id))
        else {
            return;
        };
        let op = self.force_operation(op);
        let tool = self.force_tool(&op);
        let nominal = tool.and_then(|tool| self.nominal_force_engagement(&op, tool).ok());
        let inputs = if let Some((engagement, _)) = nominal {
            [
                engagement.axial_depth_mm.to_string(),
                engagement.radial_width_mm.to_string(),
                engagement.cutting_diameter_mm.to_string(),
            ]
        } else {
            [
                String::new(),
                String::new(),
                tool.map(|t| t.diameter.to_string()).unwrap_or_default(),
            ]
        };
        self.cutting_force_inputs.insert(key, inputs);
    }

    fn force_estimate(
        &self,
        op: &Operation,
    ) -> Result<(CuttingForceEstimate, &'static str), String> {
        if matches!(op, Operation::Drill(_)) {
            return Err(
                "Drilling thrust requires a drill-specific model; milling force is unavailable."
                    .into(),
            );
        }
        if self.any_field_invalid() {
            return Err("Correct invalid operation parameters to calculate cutting forces.".into());
        }
        let setup = self.controller.inspection_setup();
        let material = setup
            .material_id
            .and_then(|id| self.library.materials.iter().find(|m| m.id == id))
            .ok_or(
                "Choose a managed material in Setup → Stock, then set its kc in Tools → Materials.",
            )?;
        let kc = if let Some(input) = self.material_force_inputs.get(&material.id) {
            parse_material_kc(input)?
        } else {
            material.specific_cutting_force
        }
        .ok_or("Set the material's specific cutting force kc in Tools → Materials.")?;
        let op = self.force_operation(op);
        let tool = self
            .force_tool(&op)
            .ok_or("Choose a cutter to calculate cutting forces.")?;
        let key = self.cutting_force_key(op.id());
        let (engagement, basis) = if let Some(inputs) = self.cutting_force_inputs.get(&key) {
            (parse_engagement(inputs)?, "Custom assumed engagement; verify the effective diameter and contact section for this cut.")
        } else {
            self.nominal_force_engagement(&op, tool)?
        };
        engagement.validate_for_tool(tool)?;
        let estimate = engagement.estimate(
            kc,
            op.cutting_feed_mm_min(),
            self.effective_cutting_rpm(op.spindle_rpm()),
        )?;
        Ok((estimate, basis))
    }

    pub(super) fn cutting_force_panel(&self, op: &Operation) -> Element<'_, Message> {
        let id = op.id();
        let setup = self.controller.inspection_setup();
        let custom = self.cutting_force_inputs.get(&self.cutting_force_key(id));
        let mut panel =
            column![text("Nominal milling load during a steady cut").size(12)].spacing(7);
        if !matches!(op, Operation::Drill(_)) {
            panel = panel.push(
                row![
                    text("Custom engagement").size(12),
                    checkbox(custom.is_some())
                        .size(15)
                        .on_toggle(move |enabled| Message::CuttingForceCustomToggled(id, enabled)),
                ]
                .spacing(8),
            );
            if let Some(inputs) = custom {
                for (index, label) in [
                    "Axial engagement ap (mm)",
                    "Radial engagement ae (mm)",
                    "Effective cutting diameter (mm)",
                ]
                .into_iter()
                .enumerate()
                {
                    panel = panel.push(
                        column![
                            text(label).size(11),
                            inspector_input("Required", &inputs[index]).on_input(move |value| {
                                Message::CuttingForceInputChanged(id, index, value)
                            })
                        ]
                        .spacing(3),
                    );
                }
                panel = panel.push(text("What-if inputs for this session; these do not change the toolpath or project.").size(11).color(INSPECTOR_MUTED));
            }
        }
        if let Some(material) = setup
            .material_id
            .and_then(|id| self.library.materials.iter().find(|m| m.id == id))
        {
            let coefficient = material
                .specific_cutting_force
                .map(|v| format!(" · kc {v} N/mm²"))
                .unwrap_or_default();
            panel = panel.push(text(format!("Material: {}{coefficient}", material.name)).size(11));
        }
        match self.force_estimate(op) {
            Ok((estimate, basis)) => {
                let line = |name: &str, value: String| {
                    row![
                        text(name.to_owned()).size(12).width(Length::Fill),
                        text(value).size(12)
                    ]
                    .spacing(8)
                };
                if custom.is_none() {
                    panel = panel.push(
                        text(format!(
                            "ap {:.3} mm · ae {:.3} mm · effective ⌀ {:.3} mm",
                            estimate.engagement.axial_depth_mm,
                            estimate.engagement.radial_width_mm,
                            estimate.engagement.cutting_diameter_mm
                        ))
                        .size(11),
                    );
                }
                panel = panel
                    .push(line(
                        "Mean tangential force",
                        format!("{:.3} N", estimate.tangential_force_n),
                    ))
                    .push(line(
                        "Net cutting power",
                        format!("{:.3} W", estimate.cutting_power_w),
                    ))
                    .push(line(
                        "Spindle torque",
                        format!("{:.3} N·m", estimate.spindle_torque_nm),
                    ))
                    .push(line(
                        "Removal rate",
                        format!("{:.3} cm³/min", estimate.removal_rate_mm3_min / 1000.),
                    ))
                    .push(
                        text(format!(
                            "{:.0} rpm · {:.1} mm/min",
                            estimate.spindle_rpm, estimate.feed_mm_min
                        ))
                        .size(11),
                    )
                    .push(text(basis).size(11).color(INSPECTOR_MUTED));
            }
            Err(error) => panel = panel.push(text(error).size(11).color(INSPECTOR_MUTED)),
        }
        panel = panel.push(text("Estimates exclude transient peaks, radial/axial forces, deflection and machine losses. Contact with remaining stock is not simulated.").size(11).color(INSPECTOR_MUTED));
        panel
            .push(inspector_button("Materials").on_press(Message::OpenMaterials))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficient_and_custom_inputs_distinguish_missing_from_invalid() {
        assert_eq!(parse_material_kc(" ").unwrap(), None);
        assert_eq!(parse_material_kc(" 1600 ").unwrap(), Some(1600.));
        for input in ["0", "-1", "NaN", "inf", "text"] {
            assert!(parse_material_kc(input).is_err());
        }
        assert!(parse_engagement(&["2".into(), "3".into(), "10".into()]).is_ok());
        assert!(parse_engagement(&["".into(), "3".into(), "10".into()]).is_err());
        assert!(parse_engagement(&["2".into(), "11".into(), "10".into()]).is_err());
    }

    fn app() -> (App, u32) {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        let tool = Tool {
            number: 1,
            diameter: 10.,
            ..Default::default()
        };
        app.library = ToolLibrary {
            tools: vec![tool.clone()],
            materials: vec![cam_model::Material {
                id: 7,
                name: "Test material".into(),
                specific_cutting_force: Some(1500.),
            }],
            ..Default::default()
        };
        app.controller.edit_setup(|s| {
            s.tools = vec![tool];
            s.material_id = Some(7);
        });
        let mut op = app.controller.operation_template(OpKind::Face, 1);
        if let Operation::Face(o) = &mut op {
            o.depth = 2.;
            o.stepdown = 2.;
            o.overlap = 0.7;
            o.feed = 1000.;
            o.spindle_rpm = 10000.;
        }
        app.controller.add_operation(op);
        let op = app.controller.selected_operation().unwrap().clone();
        let id = op.id();
        let heights = cam_model::OperationHeightSettings::legacy(app.controller.document(), &op);
        app.controller.edit_setup(|s| {
            s.height_settings.insert(id, heights);
        });
        app.begin_operation_draft(id);
        (app, id)
    }

    #[test]
    fn preview_uses_draft_feed_speed_and_height_without_changing_the_project() {
        let (mut app, id) = app();
        let op = app.current_operation(id).unwrap().clone();
        let saved = app.controller.document().clone();
        let base = app.force_estimate(&op).unwrap().0;
        assert!((base.cutting_power_w - 150.).abs() < 1e-10);
        let _ = app.update(Message::FieldChanged(Field::Feed, "2000".into()));
        let draft = app.current_operation(id).unwrap().clone();
        let faster = app.force_estimate(&draft).unwrap().0;
        assert_eq!(faster.tangential_force_n, base.tangential_force_n * 2.);
        let _ = app.update(Message::FieldChanged(Field::SpindleRpm, "20000".into()));
        let draft = app.current_operation(id).unwrap().clone();
        assert_eq!(
            app.force_estimate(&draft).unwrap().0.tangential_force_n,
            base.tangential_force_n
        );
        // A shallow height-reference draft must reduce engagement before Apply.
        let bottom = cam_model::HeightPlaneKind::Bottom;
        app.height_draft
            .as_mut()
            .unwrap()
            .definition_mut(bottom)
            .reference = cam_model::HeightReference::Origin;
        app.height_offsets.insert(bottom, "-0.25".into());
        assert_eq!(
            app.force_estimate(&draft)
                .unwrap()
                .0
                .engagement
                .axial_depth_mm,
            0.25
        );
        assert_eq!(app.controller.document(), &saved);
        let _panel = app.cutting_force_panel(&draft);
    }

    #[test]
    fn custom_engagement_is_read_only_invalid_input_hides_results_and_reset_clears_it() {
        let (mut app, id) = app();
        let saved = app.controller.document().clone();
        let op = app.current_operation(id).unwrap().clone();
        app.toggle_custom_cutting_force(id, true);
        let key = app.cutting_force_key(id);
        assert!(app.force_estimate(&op).is_ok());
        assert!(!app.inspector_dirty());
        let _ = app.update(Message::CuttingForceInputChanged(id, 1, "".into()));
        assert!(app.force_estimate(&op).is_err());
        let _ = app.update(Message::CuttingForceInputChanged(id, 1, "1".into()));
        assert!(app.force_estimate(&op).is_ok());
        assert_eq!(app.controller.document(), &saved);
        app.toggle_custom_cutting_force(id, false);
        assert!(!app.cutting_force_inputs.contains_key(&key));
        app.toggle_custom_cutting_force(id, true);
        app.reset_model_browser();
        assert!(app.cutting_force_inputs.is_empty());
    }

    #[test]
    fn creation_placeholder_does_not_reuse_an_existing_operations_assumptions() {
        let (mut app, id) = app();
        app.toggle_custom_cutting_force(id, true);
        let saved_key = app.cutting_force_key(id);
        app.cad_pending_kind = Some(OpKind::Face);
        let creation_key = app.cutting_force_key(0);
        assert_ne!(saved_key, creation_key);
        app.cutting_force_inputs
            .insert(creation_key, ["1".into(), "1".into(), "1".into()]);
        app.seed_creation_editor(OpKind::Face);
        assert!(!app.cutting_force_inputs.contains_key(&creation_key));
        assert!(app.cutting_force_inputs.contains_key(&saved_key));
    }

    #[test]
    fn material_coefficient_edits_validate_save_and_clear_without_changing_operations() {
        let (mut app, id) = app();
        let op = app.current_operation(id).unwrap().clone();
        let saved = app.controller.document().clone();
        app.edit_material_cutting_force(7, "2000".into());
        assert_eq!(app.library.materials[0].specific_cutting_force, Some(2000.));
        assert!((app.force_estimate(&op).unwrap().0.cutting_power_w - 200.).abs() < 1e-10);
        app.edit_material_cutting_force(7, "NaN".into());
        assert_eq!(app.library.materials[0].specific_cutting_force, Some(2000.));
        assert!(app.force_estimate(&op).is_err());
        let _ = app.update(Message::CloseMaterials);
        assert!(app.material_force_inputs.is_empty());
        assert!(app.force_estimate(&op).is_ok());
        app.edit_material_cutting_force(7, "".into());
        assert_eq!(app.library.materials[0].specific_cutting_force, None);
        assert!(app.force_estimate(&op).is_err());
        assert_eq!(app.controller.document(), &saved);
    }
}
