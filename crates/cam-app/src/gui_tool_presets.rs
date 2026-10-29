//! Shared material management and transactional cutting preset controls.
use super::*;
use cam_model::{Material, ToolPreset};
use iced::widget::column;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MaterialChoice {
    pub id: Option<u64>,
    pub name: String,
}
impl std::fmt::Display for MaterialChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PresetChoice {
    pub id: Option<u64>,
    pub name: String,
}
impl std::fmt::Display for PresetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PresetField {
    Rpm,
    Feed,
    Plunge,
    Ramp,
    LeadIn,
    LeadOut,
    Stepdown,
    Overlap,
}
impl PresetField {
    const ALL: [Self; 8] = [
        Self::Rpm,
        Self::Feed,
        Self::Plunge,
        Self::Ramp,
        Self::LeadIn,
        Self::LeadOut,
        Self::Stepdown,
        Self::Overlap,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Rpm => "Spindle (rpm)",
            Self::Feed => "Cutting feed (mm/min)",
            Self::Plunge => "Plunge feed (mm/min)",
            Self::Ramp => "Ramp feed (mm/min, optional)",
            Self::LeadIn => "Lead-in feed (mm/min, optional)",
            Self::LeadOut => "Lead-out feed (mm/min, optional)",
            Self::Stepdown => "Stepdown (mm, optional)",
            Self::Overlap => "Overlap (%, optional)",
        }
    }
    fn valid(self, value: &str) -> bool {
        if value.trim().is_empty() {
            return matches!(
                self,
                Self::Stepdown | Self::Overlap | Self::Ramp | Self::LeadIn | Self::LeadOut
            );
        }
        value.trim().parse::<f64>().ok().is_some_and(|v| {
            v.is_finite()
                && match self {
                    Self::Rpm => v >= 0.,
                    Self::Overlap => (0. ..=99.).contains(&v),
                    _ => v > 0.,
                }
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum ToolDetail {
    Description,
    Manufacturer,
    ProductId,
    ProductLink,
}

/// Preserve each cutter's pending edits while browsing another cutter.
#[derive(Clone, Debug)]
pub(super) struct LibraryToolDraft {
    pub tool: cam_model::Tool,
    fields: BTreeMap<Field, String>,
    preset: Option<u64>,
    preset_values: BTreeMap<PresetField, String>,
}

impl App {
    pub(super) fn select_library_tool(&mut self, index: usize) {
        if index == self.lib_sel || index >= self.library.tools.len() {
            return;
        }
        if self.library_mode() && (self.inspector_dirty() || self.any_field_invalid()) {
            if let Some(tool) = &self.tool_edit {
                self.tool_drafts.insert(
                    self.lib_sel,
                    LibraryToolDraft {
                        tool: tool.clone(),
                        fields: self.fields.clone(),
                        preset: self.library_preset,
                        preset_values: self.preset_values.clone(),
                    },
                );
            }
        }
        self.lib_sel = index;
        self.reload_tool_edit();
        if let Some(draft) = self.tool_drafts.remove(&index) {
            self.tool_edit = Some(draft.tool);
            self.fields = draft.fields;
            self.library_preset = draft.preset;
            self.preset_values = draft.preset_values;
        }
    }

    fn material_choices(&self, unlinked: &str) -> Vec<MaterialChoice> {
        std::iter::once(MaterialChoice {
            id: None,
            name: unlinked.into(),
        })
        .chain(self.library.materials.iter().map(|m| MaterialChoice {
            id: Some(m.id),
            name: m.name.clone(),
        }))
        .collect()
    }
    pub(super) fn stock_material_picker(&self, setup: &cam_model::Setup) -> Element<'_, Message> {
        let mut choices = self.material_choices("No managed material");
        if let Some(id) = setup.material_id {
            if !choices.iter().any(|m| m.id == Some(id)) {
                choices.push(MaterialChoice {
                    id: Some(id),
                    name: format!("{} (unavailable in library)", setup.material),
                });
            }
        }
        let selected = choices.iter().find(|m| m.id == setup.material_id).cloned();
        column![
            pick_list(choices, selected, Message::SetupMaterialPicked)
                .text_size(12)
                .padding([7, 9])
                .style(inspector_picker)
                .width(Length::Fill),
            text(
                if setup.material_id.is_none() && !setup.material.is_empty() {
                    format!("Legacy material: {}", setup.material)
                } else {
                    "Manage materials in Tools → Materials.".into()
                }
            )
            .size(11)
            .color(INSPECTOR_MUTED),
        ]
        .spacing(6)
        .into()
    }
    pub(super) fn library_preset_mut(&mut self) -> Option<&mut ToolPreset> {
        let id = self.library_preset?;
        self.tool_edit
            .as_mut()?
            .presets
            .iter_mut()
            .find(|p| p.id == id)
    }
    pub(super) fn select_library_preset(&mut self, id: Option<u64>) {
        self.library_preset = id;
        self.preset_values.clear();
        if let Some(p) = self
            .tool_edit
            .as_ref()
            .and_then(|t| t.presets.iter().find(|p| Some(p.id) == id))
        {
            self.preset_values = [
                (PresetField::Rpm, fmt_num(p.spindle_rpm)),
                (PresetField::Feed, fmt_num(p.feed)),
                (PresetField::Plunge, fmt_num(p.plunge_feed)),
                (
                    PresetField::Ramp,
                    p.entry_feeds.ramp.map(fmt_num).unwrap_or_default(),
                ),
                (
                    PresetField::LeadIn,
                    p.entry_feeds.lead_in.map(fmt_num).unwrap_or_default(),
                ),
                (
                    PresetField::LeadOut,
                    p.entry_feeds.lead_out.map(fmt_num).unwrap_or_default(),
                ),
                (
                    PresetField::Stepdown,
                    p.stepdown.map(fmt_num).unwrap_or_default(),
                ),
                (
                    PresetField::Overlap,
                    p.overlap.map(|v| fmt_num(v * 100.)).unwrap_or_default(),
                ),
            ]
            .into_iter()
            .collect();
        }
    }
    pub(super) fn add_library_preset(&mut self) {
        if self.preset_edit_invalid() {
            self.status = "Correct the current preset before adding another.".into();
            return;
        }
        let defaults = *self.controller.defaults();
        let Some(tool) = &mut self.tool_edit else {
            return;
        };
        let id = tool.next_preset_id();
        let source = tool
            .presets
            .iter()
            .find(|p| Some(p.id) == self.library_preset)
            .cloned();
        let preset = ToolPreset {
            id,
            name: format!("Preset {id}"),
            ..source.unwrap_or(ToolPreset {
                id,
                name: String::new(),
                material_id: None,
                spindle_rpm: tool.nominal_rpm,
                feed: if tool.nominal_feed > 0. {
                    tool.nominal_feed
                } else {
                    defaults.feed
                },
                plunge_feed: if tool.nominal_plunge_feed > 0. {
                    tool.nominal_plunge_feed
                } else {
                    defaults.plunge_feed
                },
                stepdown: None,
                overlap: None,
                cooling: tool.cooling,
                entry_feeds: Default::default(),
            })
        };
        tool.presets.push(preset);
        self.select_library_preset(Some(id));
    }
    pub(super) fn delete_library_preset(&mut self) {
        if let Some(tool) = &mut self.tool_edit {
            tool.presets.retain(|p| Some(p.id) != self.library_preset);
            let next = tool.presets.first().map(|p| p.id);
            self.select_library_preset(next);
        }
    }
    pub(super) fn edit_preset_value(&mut self, field: PresetField, value: String) {
        if field.valid(&value) {
            let parsed = value.trim().parse::<f64>().ok();
            if let Some(p) = self.library_preset_mut() {
                match field {
                    PresetField::Rpm => p.spindle_rpm = parsed.unwrap(),
                    PresetField::Feed => p.feed = parsed.unwrap(),
                    PresetField::Plunge => p.plunge_feed = parsed.unwrap(),
                    PresetField::Ramp => p.entry_feeds.ramp = parsed,
                    PresetField::LeadIn => p.entry_feeds.lead_in = parsed,
                    PresetField::LeadOut => p.entry_feeds.lead_out = parsed,
                    PresetField::Stepdown => p.stepdown = parsed,
                    PresetField::Overlap => p.overlap = parsed.map(|v| v / 100.),
                }
            }
        }
        self.preset_values.insert(field, value);
    }
    pub(super) fn preset_edit_invalid(&self) -> bool {
        self.tool_edit
            .as_ref()
            .is_some_and(|t| t.presets.iter().any(|p| p.validate().is_err()))
            || self
                .preset_values
                .iter()
                .any(|(field, value)| !field.valid(value))
    }
    pub(super) fn library_preset_editor(&self) -> Element<'_, Message> {
        let Some(tool) = &self.tool_edit else {
            return column![].into();
        };
        let choices: Vec<_> = tool
            .presets
            .iter()
            .map(|p| PresetChoice {
                id: Some(p.id),
                name: p.name.clone(),
            })
            .collect();
        let selected = choices
            .iter()
            .find(|c| c.id == self.library_preset)
            .cloned();
        let mut list = column![
            inspector_section("Cutting presets"),
            pick_list(choices, selected, |c| Message::LibraryPresetSelected(
                c.id.unwrap()
            ))
            .placeholder("No presets")
            .text_size(12)
            .padding([7, 9])
            .style(inspector_picker)
            .width(Length::Fill),
            row![
                inspector_button("Add / duplicate").on_press(Message::AddPreset),
                inspector_button("Delete")
                    .on_press_maybe(self.library_preset.map(|_| Message::DeletePreset))
            ]
            .spacing(6),
        ]
        .spacing(8);
        if let Some(preset) = tool
            .presets
            .iter()
            .find(|p| Some(p.id) == self.library_preset)
        {
            let mut materials = self.material_choices("Any material");
            if let Some(id) = preset.material_id {
                if !materials.iter().any(|m| m.id == Some(id)) {
                    materials.push(MaterialChoice {
                        id: Some(id),
                        name: format!("Unavailable material #{id}"),
                    });
                }
            }
            let selected = materials
                .iter()
                .find(|m| m.id == preset.material_id)
                .cloned();
            list = list.push(
                inspector_input("Preset name", &preset.name).on_input(Message::PresetNameChanged),
            );
            list = list.push(
                text(format!(
                    "Material ID: {}",
                    preset
                        .material_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "Any material".into())
                ))
                .size(12),
            );
            list = list.push(
                pick_list(materials, selected, Message::PresetMaterialChanged)
                    .text_size(12)
                    .padding([7, 9])
                    .style(inspector_picker)
                    .width(Length::Fill),
            );
            list = list.push(
                column![
                    text("Cooling").size(12),
                    pick_list(
                        &cam_model::Cooling::ALL[..],
                        Some(preset.cooling),
                        Message::PresetCoolingChanged
                    )
                    .text_size(12)
                    .padding([7, 9])
                    .style(inspector_picker)
                    .width(Length::Fill)
                ]
                .spacing(4),
            );
            for field in PresetField::ALL {
                let value = self
                    .preset_values
                    .get(&field)
                    .map(String::as_str)
                    .unwrap_or("");
                list = list.push(
                    column![
                        text(field.label()).size(12),
                        inspector_input("", value)
                            .on_input(move |v| Message::PresetValueChanged(field, v))
                    ]
                    .spacing(4),
                );
                if !field.valid(value) {
                    list = list.push(text("Enter a valid value").size(11).color(palette::ERROR));
                }
            }
            if preset.name.trim().is_empty() {
                list = list.push(text("Enter a preset name").size(11).color(palette::ERROR));
            }
            list = list.push(text("Apply tool changes to save presets. The first preset for a material is its automatic default.").size(11).color(INSPECTOR_MUTED));
        }
        list.into()
    }
    /// Material IDs are persisted on presets; names are resolved from the shop library.
    pub(super) fn cutting_presets_table(&self) -> Element<'_, Message> {
        let Some(tool) = &self.tool_edit else {
            return column![].into();
        };
        let selected = self.library_preset;
        let toolbar = row![
            text("Cutting parameters").size(14),
            Space::new().width(Length::Fill),
            inspector_button("Add preset").on_press(Message::AddPreset),
            inspector_button("Edit").on_press_maybe(selected.map(|_| Message::EditPreset)),
            inspector_button("Remove").on_press_maybe(selected.map(|_| Message::DeletePreset)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let widths = [
            150., 150., 80., 75., 85., 115., 115., 105., 105., 105., 95., 90.,
        ];
        let cells = |values: Vec<String>| {
            let mut cells = row![].spacing(4);
            for (value, width) in values.into_iter().zip(widths) {
                cells = cells.push(text(value).size(12).width(Length::Fixed(width)));
            }
            cells
        };
        let mut rows = column![container(cells(
            [
                "Preset",
                "Material",
                "Material ID",
                "Cooling",
                "RPM",
                "Feed mm/min",
                "Plunge mm/min",
                "Ramp mm/min",
                "Lead-in mm/min",
                "Lead-out mm/min",
                "Stepdown mm",
                "Overlap %",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        ))
        .padding(7)]
        .spacing(2);
        // Show the cutter's fallback cutting data even before a named preset is added.
        let nominal = |value| {
            if value > 0. {
                fmt_num(value)
            } else {
                "Job default".into()
            }
        };
        rows = rows.push(
            container(cells(vec![
                "Tool defaults".into(),
                "Any material".into(),
                "—".into(),
                tool.cooling.to_string(),
                nominal(tool.nominal_rpm),
                nominal(tool.nominal_feed),
                nominal(tool.nominal_plunge_feed),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
                "—".into(),
            ]))
            .padding(7),
        );
        let optional = |v: Option<f64>| v.map(fmt_num).unwrap_or_else(|| "—".into());
        for preset in &tool.presets {
            let material = preset
                .material_id
                .map(|id| {
                    self.library
                        .materials
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| format!("Unavailable #{id}"))
                })
                .unwrap_or_else(|| "Any material".into());
            let active = selected == Some(preset.id);
            let item = container(cells(vec![
                preset.name.clone(),
                material,
                preset
                    .material_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "—".into()),
                preset.cooling.to_string(),
                fmt_num(preset.spindle_rpm),
                fmt_num(preset.feed),
                fmt_num(preset.plunge_feed),
                optional(preset.entry_feeds.ramp),
                optional(preset.entry_feeds.lead_in),
                optional(preset.entry_feeds.lead_out),
                optional(preset.stepdown),
                optional(preset.overlap.map(|v| v * 100.)),
            ]))
            .padding(7)
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: active.then_some(Background::Color(palette::SELECT_BG)),
                ..Default::default()
            });
            rows = rows.push(mouse_area(item).on_press(Message::LibraryPresetSelected(preset.id)));
        }
        if tool.presets.is_empty() {
            rows = rows.push(text("No cutting presets for this cutter. Add a preset to define material-specific parameters.").size(12));
        }
        container(
            column![
                toolbar,
                scrollable(rows.width(1445))
                    .direction(scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::default(),
                        horizontal: scrollable::Scrollbar::default(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                text("Edit tool defaults in the inspector. Apply changes saves cutter details and presets.")
                    .size(11)
                    .color(INSPECTOR_MUTED),
            ]
            .spacing(8),
        )
        .padding(16)
        .height(Length::FillPortion(2))
        .width(Length::Fill)
        .into()
    }

    pub(super) fn tool_management_overlay(&self) -> Option<Element<'_, Message>> {
        let (title, content, close) = if self.show_materials {
            (
                "Materials",
                self.managed_material_editor(),
                Message::CloseMaterials,
            )
        } else if self.show_preset_editor {
            (
                "Edit cutting preset",
                self.library_preset_editor(),
                Message::ClosePresetEditor,
            )
        } else {
            return None;
        };
        let can_close = self.show_materials || !self.preset_edit_invalid();
        let card = container(
            column![
                text(title).size(18),
                scrollable(content).height(Length::Shrink),
                text(self.status.clone()).size(11).color(INSPECTOR_MUTED),
                row![
                    Space::new().width(Length::Fill),
                    inspector_button("Done").on_press_maybe(can_close.then_some(close))
                ],
            ]
            .spacing(12),
        )
        .width(600)
        .max_height((self.window.height - 60.).max(200.))
        .padding(16)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color,
            )),
            border: Border {
                color: palette::SEPARATOR,
                width: 1.,
                radius: 6.0.into(),
            },
            ..Default::default()
        });
        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_| {
                container::Style {
                    background: Some(Background::Color(Color::from_rgba(0., 0., 0., 0.45))),
                    ..Default::default()
                }
            }),
        )
        .on_press(Message::Noop);
        Some(
            iced::widget::stack![
                backdrop,
                container(iced::widget::opaque(card))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
            ]
            .into(),
        )
    }

    pub(super) fn managed_material_editor(&self) -> Element<'_, Message> {
        let mut list = column![
            inspector_section("Materials"),
            text("Shared across projects. IDs stay fixed when names change.")
                .size(11)
                .color(INSPECTOR_MUTED)
        ]
        .spacing(8);
        for material in &self.library.materials {
            let id = material.id;
            list = list.push(
                column![
                    text(format!("ID {id}")).size(10).color(INSPECTOR_MUTED),
                    row![
                        inspector_input("Material name", &material.name)
                            .on_input(move |v| Message::MaterialRenamed(id, v)),
                        inspector_button("Delete").on_press(Message::DeleteMaterial(id))
                    ]
                    .spacing(6),
                ]
                .spacing(3),
            );
        }
        list.push(
            row![
                inspector_input("New material", &self.new_material_name)
                    .on_input(Message::NewMaterialNameChanged),
                inspector_button("Add").on_press_maybe(
                    (!self.new_material_name.trim().is_empty()).then_some(Message::AddMaterial)
                )
            ]
            .spacing(6),
        )
        .into()
    }
    pub(super) fn add_managed_material(&mut self) {
        let name = self.new_material_name.trim();
        if name.is_empty()
            || self
                .library
                .materials
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(name))
        {
            self.status = "Use a unique material name.".into();
            return;
        }
        self.library.material_id_floor = self.library.material_id_floor.max(
            self.library
                .materials
                .iter()
                .map(|m| m.id)
                .max()
                .unwrap_or(0),
        ) + 1;
        self.library.materials.push(Material {
            id: self.library.material_id_floor,
            name: name.into(),
        });
        self.new_material_name.clear();
        self.library.save();
    }
    pub(super) fn rename_managed_material(&mut self, id: u64, name: String) {
        if name.trim().is_empty()
            || self
                .library
                .materials
                .iter()
                .any(|m| m.id != id && m.name.eq_ignore_ascii_case(name.trim()))
        {
            return;
        }
        if let Some(material) = self.library.materials.iter_mut().find(|m| m.id == id) {
            material.name = name;
        }
        self.library.save();
    }
    pub(super) fn delete_managed_material(&mut self, id: u64) {
        let in_use = self
            .library
            .tools
            .iter()
            .chain(self.tool_edit.iter())
            .any(|t| t.presets.iter().any(|p| p.material_id == Some(id)))
            || self
                .controller
                .document()
                .setups
                .iter()
                .any(|s| s.material_id == Some(id))
            || self
                .setup_draft
                .as_ref()
                .is_some_and(|s| s.material_id == Some(id));
        if in_use {
            self.status =
                "This material is used by a stock or preset. Unlink it before deleting.".into();
            return;
        }
        self.library.materials.retain(|m| m.id != id);
        self.library.save();
    }
    fn operation_preset_tool(&self) -> Option<&cam_model::Tool> {
        let op = self.operation_draft.as_ref()?;
        if self.cad_pending_kind.is_some() {
            return self
                .library
                .tools
                .iter()
                .find(|t| Some(t.number) == self.cad_tool_pick);
        }
        self.operation_tools
            .iter()
            .chain(self.controller.inspection_setup().tools.iter())
            .find(|t| t.number == op.tool())
    }
    pub(super) fn auto_operation_preset(&mut self) {
        if let Some(cooling) = self.operation_preset_tool().map(|t| t.cooling) {
            self.edit_operation_draft(|op| op.set_cooling(cooling));
        }
        let preset = self
            .operation_preset_tool()
            .and_then(|t| t.preset_for_material(self.controller.inspection_setup().material_id))
            .cloned();
        if let Some(preset) = preset {
            self.apply_operation_preset(&preset);
        }
    }
    fn apply_operation_preset(&mut self, preset: &ToolPreset) {
        let Some(op) = &mut self.operation_draft else {
            return;
        };
        if let Err(error) = op.apply_tool_preset(preset) {
            self.status = error;
            return;
        }
        // Refresh only the cutting values, preserving unrelated in-progress text.
        for field in [
            Field::SpindleRpm,
            Field::Feed,
            Field::PlungeFeed,
            Field::RampFeed,
            Field::LeadInFeed,
            Field::LeadOutFeed,
            Field::Stepdown,
            Field::Overlap,
            Field::Peck,
        ] {
            if matches!(field, Field::Stepdown | Field::Peck) && preset.stepdown.is_none() {
                continue;
            }
            if field == Field::Overlap && preset.overlap.is_none() {
                continue;
            }
            if (field == Field::RampFeed && preset.entry_feeds.ramp.is_none())
                || (field == Field::LeadInFeed && preset.entry_feeds.lead_in.is_none())
                || (field == Field::LeadOutFeed && preset.entry_feeds.lead_out.is_none())
            {
                continue;
            }
            if let Some(value) = op_field(op, field) {
                self.fields.insert(field, fmt_num(value));
            }
        }
    }
    pub(super) fn pick_operation_preset(&mut self, id: Option<u64>) {
        let Some(id) = id else {
            self.edit_operation_draft(Operation::clear_tool_preset);
            return;
        };
        let preset = self
            .operation_preset_tool()
            .and_then(|t| t.presets.iter().find(|p| p.id == id))
            .or_else(|| {
                self.operation_draft
                    .as_ref()?
                    .tool_preset()
                    .filter(|p| p.id == id)
            })
            .cloned();
        if let Some(preset) = preset {
            self.apply_operation_preset(&preset);
        }
    }
    pub(super) fn operation_preset_picker(&self, op: &Operation) -> Element<'_, Message> {
        let mut choices = vec![PresetChoice {
            id: None,
            name: "Manual cutting data".into(),
        }];
        if let Some(tool) = self.operation_preset_tool() {
            choices.extend(tool.presets.iter().map(|p| PresetChoice {
                id: Some(p.id),
                name: p.name.clone(),
            }));
        }
        let selected = if let Some(preset) = op.tool_preset() {
            if !choices.iter().any(|p| p.id == Some(preset.id)) {
                choices.push(PresetChoice {
                    id: Some(preset.id),
                    name: preset.name.clone(),
                });
            }
            choices.iter().find(|p| p.id == Some(preset.id)).cloned()
        } else {
            choices.first().cloned()
        };
        let mut list = column![
            text("Cutting preset").size(12),
            pick_list(choices, selected, Message::OperationPresetPicked)
                .text_size(12)
                .padding([7, 9])
                .style(inspector_picker)
                .width(Length::Fill),
            text("Cooling").size(12),
            pick_list(
                &cam_model::Cooling::ALL[..],
                Some(op.cooling()),
                Message::OperationCoolingChanged
            )
            .text_size(12)
            .padding([7, 9])
            .style(inspector_picker)
            .width(Length::Fill),
        ]
        .spacing(6);
        if let Some(preset) = op.tool_preset() {
            let mut baseline = op.clone();
            if baseline.apply_tool_preset(preset).is_ok() && !baseline.same_work(op) {
                list = list.push(
                    text("Preset values overridden for this operation.")
                        .size(11)
                        .color(INSPECTOR_MUTED),
                );
            }
        }
        list.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preset(id: u64, material: u64, feed: f64) -> ToolPreset {
        ToolPreset {
            id,
            name: format!("Preset {id}"),
            material_id: Some(material),
            spindle_rpm: 5000.,
            feed,
            plunge_feed: 100.,
            stepdown: Some(0.5),
            overlap: Some(0.25),
            cooling: Default::default(),
            entry_feeds: Default::default(),
        }
    }
    fn app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.setup_draft = None;
        app.controller.edit_setup(|s| {
            s.material_id = Some(7);
            s.material = "Aluminum".into();
        });
        let mut tool = crate::tool_library::default_tool(1, ToolKind::EndMill);
        tool.presets = vec![preset(1, 7, 500.), preset(2, 8, 200.)];
        app.library = ToolLibrary {
            tools: vec![tool],
            ..Default::default()
        };
        app.controller.select(Selection::Setup);
        app.refresh_fields();
        app
    }
    fn send(app: &mut App, message: Message) {
        let _ = app.update(message);
    }
    #[test]
    fn creating_switching_overriding_and_undoing_presets_is_transactional() {
        let mut app = app();
        let before = app.controller.document().clone();
        send(&mut app, Message::BeginOp(OpKind::Pocket));
        assert_eq!(
            app.operation_draft
                .as_ref()
                .unwrap()
                .tool_preset()
                .unwrap()
                .id,
            1
        );
        assert_eq!(app.fields[&Field::Feed], "500");
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        send(&mut app, Message::FieldChanged(Field::Depth, "2.5".into()));
        send(
            &mut app,
            Message::OperationPresetPicked(PresetChoice {
                id: Some(2),
                name: "Preset 2".into(),
            }),
        );
        assert_eq!(app.fields[&Field::Feed], "200");
        assert_eq!(app.fields[&Field::Depth], "2.5");
        send(&mut app, Message::FieldChanged(Field::Feed, "321".into()));
        send(
            &mut app,
            Message::FieldChanged(Field::Stepdown, "0.2".into()),
        );
        send(&mut app, Message::OperationTabChanged(OperationTab::Passes));
        send(&mut app, Message::OperationTabChanged(OperationTab::Tool));
        assert_eq!(app.fields[&Field::Feed], "321");
        assert!(app.creation_ready(), "{}", app.status);
        send(&mut app, Message::ConfirmOp);
        let operation = app.controller.inspection_setup().operations.last().unwrap();
        let Operation::Pocket(o) = operation else {
            panic!()
        };
        assert_eq!(o.clear.feed, 321.);
        assert_eq!(o.clear.stepdown, 0.2);
        assert_eq!(o.tool_preset.as_ref().unwrap().id, 2);
        let after = app.controller.document().clone();
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
        assert!(app.controller.redo());
        assert_eq!(app.controller.document(), &after);
    }
    #[test]
    fn choosing_a_cutter_uses_current_library_presets_and_cancel_keeps_saved_values() {
        let mut app = app();
        send(&mut app, Message::BeginOp(OpKind::Profile));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        send(&mut app, Message::ConfirmOp);
        let id = app
            .controller.inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        let before = app.controller.document().clone();
        app.library.tools[0].presets[0].feed = 650.;
        send(&mut app, Message::OperationToolPicked(0));
        assert_eq!(app.fields[&Field::Feed], "650");
        assert_eq!(app.controller.document(), &before);
        send(&mut app, Message::CancelOperationDraft);
        assert_eq!(app.controller.document(), &before);
        app.begin_operation_draft(id);
        assert_eq!(app.fields[&Field::Feed], "500");
        send(&mut app, Message::OperationToolPicked(0));
        send(&mut app, Message::Apply);
        assert_eq!(
            app.controller
                .operation(id)
                .unwrap()
                .tool_preset()
                .unwrap()
                .feed,
            650.
        );
        assert_eq!(
            app.controller.inspection_setup()
                .tools
                .iter()
                .find(|t| t.number == app.controller.operation(id).unwrap().tool())
                .unwrap()
                .presets[0]
                .feed,
            650.
        );
    }
    #[test]
    fn invalid_preset_buffers_block_apply_and_cancel_discards_preset_edits() {
        let mut app = app();
        app.switch_workspace(Workspace::Tools);
        app.lib_sel = 0;
        app.reload_tool_edit();
        let saved = app.library.tools[0].clone();
        send(
            &mut app,
            Message::PresetValueChanged(PresetField::Feed, "NaN".into()),
        );
        assert!(app.any_field_invalid());
        app.apply_inspector();
        assert_eq!(app.library.tools[0], saved);
        send(&mut app, Message::CancelToolDraft);
        assert!(!app.preset_edit_invalid());
        assert_eq!(app.tool_edit.as_ref().unwrap(), &saved);
        send(&mut app, Message::AddPreset);
        assert_eq!(app.tool_edit.as_ref().unwrap().presets.len(), 3);
        assert_eq!(app.library.tools[0].presets.len(), 2);
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref().unwrap(), &saved);
    }
    #[test]
    fn cooling_preset_edits_and_operation_overrides_are_transactional() {
        use cam_model::Cooling;
        let mut app = app();
        app.library.tools[0].cooling = Cooling::Mms;
        app.library.tools[0].presets[0].cooling = Cooling::Mms;
        let before = app.controller.document().clone();
        send(&mut app, Message::BeginOp(OpKind::Pocket));
        assert_eq!(
            app.operation_draft.as_ref().unwrap().cooling(),
            Cooling::Mms
        );
        send(&mut app, Message::OperationCoolingChanged(Cooling::None));
        let op = app.operation_draft.as_ref().unwrap();
        assert_eq!(op.cooling(), Cooling::None);
        assert_eq!(op.tool_preset().unwrap().cooling, Cooling::Mms);
        send(
            &mut app,
            Message::OperationPresetPicked(PresetChoice {
                id: Some(1),
                name: "Preset 1".into(),
            }),
        );
        assert_eq!(
            app.operation_draft.as_ref().unwrap().cooling(),
            Cooling::Mms
        );
        send(&mut app, Message::CancelOp);
        assert_eq!(app.controller.document(), &before);
        app.switch_workspace(Workspace::Tools);
        let saved = app.library.tools[0].clone();
        send(&mut app, Message::PresetCoolingChanged(Cooling::None));
        assert_eq!(
            app.tool_edit.as_ref().unwrap().presets[0].cooling,
            Cooling::None
        );
        assert_eq!(app.library.tools[0], saved);
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref().unwrap(), &saved);
        send(&mut app, Message::ToolCoolingChanged(Cooling::None));
        assert!(app.inspector_dirty());
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref().unwrap().cooling, Cooling::Mms);
    }

    #[test]
    fn selecting_a_preset_does_not_block_selecting_another_cutter() {
        let mut app = app();
        let mut other = crate::tool_library::default_tool(2, ToolKind::BallMill);
        other.presets = vec![preset(3, 7, 900.)];
        app.library.tools.push(other.clone());
        app.switch_workspace(Workspace::Tools);
        send(&mut app, Message::LibraryPresetSelected(2));
        assert!(!app.inspector_dirty());
        send(&mut app, Message::SelectLibraryTool(1));
        assert_eq!(app.lib_sel, 1);
        assert_eq!(app.tool_edit.as_ref(), Some(&other));
        assert_eq!(app.library_preset, Some(3));
        assert!(app.tool_drafts.is_empty());
    }

    #[test]
    fn browsing_cutters_retains_pending_preset_values_and_invalid_buffers() {
        let mut app = app();
        app.library
            .tools
            .push(crate::tool_library::default_tool(2, ToolKind::BallMill));
        app.switch_workspace(Workspace::Tools);
        let saved = app.library.tools.clone();
        send(&mut app, Message::LibraryPresetSelected(2));
        send(
            &mut app,
            Message::PresetValueChanged(PresetField::Feed, "750".into()),
        );
        send(
            &mut app,
            Message::PresetValueChanged(PresetField::Rpm, "invalid".into()),
        );
        send(
            &mut app,
            Message::FieldChanged(Field::ToolDiameter, "unfinished".into()),
        );
        send(&mut app, Message::SelectLibraryTool(1));
        assert_eq!(app.lib_sel, 1);
        assert!(!app.inspector_dirty());
        assert!(!app.any_field_invalid());
        send(
            &mut app,
            Message::ToolDetailChanged(ToolDetail::Description, "Other cutter".into()),
        );
        send(&mut app, Message::LibToolMenu(0));
        assert_eq!(app.lib_sel, 0);
        assert_eq!(app.library_preset, Some(2));
        assert_eq!(app.preset_values[&PresetField::Rpm], "invalid");
        assert_eq!(app.fields[&Field::ToolDiameter], "unfinished");
        assert_eq!(app.tool_edit.as_ref().unwrap().presets[1].feed, 750.);
        assert!(app.any_field_invalid());
        assert_eq!(app.library.tools, saved);
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref(), Some(&saved[0]));
        send(&mut app, Message::SelectLibraryTool(1));
        assert_eq!(app.tool_edit.as_ref().unwrap().description, "Other cutter");
        assert!(app.tool_drafts.is_empty());
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref(), Some(&saved[1]));
    }

    #[test]
    fn catalog_edits_are_transactional_and_survive_kind_changes() {
        let mut app = app();
        app.switch_workspace(Workspace::Tools);
        app.lib_sel = 0;
        app.reload_tool_edit();
        let saved = app.library.tools[0].clone();
        for (field, value) in [
            (ToolDetail::Description, "Finishing cutter"),
            (ToolDetail::Manufacturer, "Example Tools"),
            (ToolDetail::ProductId, "EM-6"),
            (ToolDetail::ProductLink, "https://example.com/EM-6"),
        ] {
            send(&mut app, Message::ToolDetailChanged(field, value.into()));
        }
        assert!(app.inspector_dirty());
        assert_eq!(app.library.tools[0], saved);
        let edited = app.tool_edit.clone().unwrap();
        send(
            &mut app,
            Message::ToolKindChanged(ToolKind::Drill {
                point_angle_deg: 118.,
            }),
        );
        assert_eq!(
            app.tool_edit.as_ref().unwrap().description,
            edited.description
        );
        assert_eq!(
            app.tool_edit.as_ref().unwrap().manufacturer,
            edited.manufacturer
        );
        assert_eq!(
            app.tool_edit.as_ref().unwrap().product_id,
            edited.product_id
        );
        assert_eq!(
            app.tool_edit.as_ref().unwrap().product_link,
            edited.product_link
        );
        send(&mut app, Message::CancelToolDraft);
        assert_eq!(app.tool_edit.as_ref().unwrap(), &saved);
    }

    #[test]
    fn preset_modal_keeps_invalid_edits_open_and_escape_preserves_tool_draft() {
        let mut app = app();
        app.switch_workspace(Workspace::Tools);
        app.lib_sel = 0;
        app.reload_tool_edit();
        let saved = app.library.tools[0].clone();
        send(&mut app, Message::EditPreset);
        assert!(app.show_preset_editor);
        send(
            &mut app,
            Message::PresetValueChanged(PresetField::Feed, "invalid".into()),
        );
        send(&mut app, Message::ClosePresetEditor);
        assert!(app.show_preset_editor);
        send(
            &mut app,
            Message::PresetValueChanged(PresetField::Feed, "750".into()),
        );
        send(&mut app, Message::CadEscape);
        assert!(!app.show_preset_editor);
        assert_eq!(app.tool_edit.as_ref().unwrap().presets[0].feed, 750.);
        assert_eq!(app.library.tools[0], saved);
        send(&mut app, Message::OpenMaterials);
        assert!(app.show_materials);
        send(&mut app, Message::CadEscape);
        assert!(!app.show_materials);
        assert!(app.inspector_dirty());
    }

    #[test]
    fn cad_creation_uses_the_same_material_match() {
        let mut app = app();
        app.cad_pending_kind = Some(OpKind::Profile);
        app.cad_tool_pick = Some(1);
        app.controller.cancel_operation();
        app.seed_creation_editor(OpKind::Profile);
        assert_eq!(
            app.operation_draft
                .as_ref()
                .unwrap()
                .tool_preset()
                .unwrap()
                .id,
            1
        );
        assert_eq!(app.fields[&Field::Feed], "500");
        send(
            &mut app,
            Message::OperationPresetPicked(PresetChoice {
                id: Some(2),
                name: "Preset 2".into(),
            }),
        );
        assert_eq!(app.fields[&Field::Feed], "200");
    }
    #[test]
    fn absent_optional_preset_values_preserve_manual_cutting_buffers() {
        let mut app = app();
        app.library.tools[0].presets[1].stepdown = None;
        app.library.tools[0].presets[1].overlap = None;
        send(&mut app, Message::BeginOp(OpKind::Pocket));
        assert!(app.creating_operation(), "{}", app.status);
        send(
            &mut app,
            Message::FieldChanged(Field::Stepdown, "0.25".into()),
        );
        send(&mut app, Message::FieldChanged(Field::Overlap, "55".into()));
        send(
            &mut app,
            Message::OperationPresetPicked(PresetChoice {
                id: Some(2),
                name: "Preset 2".into(),
            }),
        );
        assert_eq!(app.fields[&Field::Stepdown], "0.25");
        assert_eq!(app.fields[&Field::Overlap], "55");
        assert_eq!(app.fields[&Field::Feed], "200");
    }
    #[test]
    fn managed_materials_keep_ids_on_rename_and_protect_links_on_delete() {
        let mut app = app();
        app.new_material_name = "Wood".into();
        app.add_managed_material();
        let id = app.library.materials[0].id;
        app.rename_managed_material(id, "Hardwood".into());
        assert_eq!(
            app.library.materials[0],
            Material {
                id,
                name: "Hardwood".into()
            }
        );
        app.library.tools[0].presets[0].material_id = Some(id);
        app.delete_managed_material(id);
        assert_eq!(app.library.materials.len(), 1);
        app.library.tools[0].presets[0].material_id = None;
        app.delete_managed_material(id);
        assert!(app.library.materials.is_empty());
        app.new_material_name = "Brass".into();
        app.add_managed_material();
        assert!(app.library.materials[0].id > id);
        assert_eq!(
            serde_json::to_value(&app.library.materials[0])
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn independent_entry_feeds_seed_from_presets_and_remain_editable_per_operation() {
        let mut app = app();
        app.library.tools[0].presets[0].entry_feeds = cam_model::EntryFeeds {
            ramp: Some(170.),
            lead_in: Some(210.),
            lead_out: Some(220.),
        };
        send(&mut app, Message::BeginOp(OpKind::Profile));
        for (field, expected) in [
            (Field::RampFeed, "170"),
            (Field::LeadInFeed, "210"),
            (Field::LeadOutFeed, "220"),
        ] {
            assert!(app.inspector_fields().contains(&field));
            assert_eq!(app.fields[&field], expected);
            assert_eq!(operation_tabs::field_tab(field), OperationTab::Tool);
        }
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0)], None)
            .unwrap();
        send(
            &mut app,
            Message::FieldChanged(Field::RampFeed, "180".into()),
        );
        send(
            &mut app,
            Message::FieldChanged(Field::LeadInFeed, "0".into()),
        );
        send(
            &mut app,
            Message::FieldChanged(Field::LeadOutFeed, "230".into()),
        );
        send(&mut app, Message::ConfirmOp);
        let op = app.controller.inspection_setup().operations.last().unwrap();
        assert_eq!(
            *op.entry_feeds(),
            cam_model::EntryFeeds {
                ramp: Some(180.),
                lead_in: None,
                lead_out: Some(230.)
            }
        );
        assert_eq!(op.tool_preset().unwrap().entry_feeds.ramp, Some(170.));
    }
}
