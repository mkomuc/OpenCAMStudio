//! Setup form with shared inspector controls and a persistent action bar.
use super::*;
use iced::widget::column;

impl App {
    fn setup_geometry_list(&self, draft: &cam_model::Setup) -> Element<'_, Message> {
        let geometry = self.controller.geometry_state();
        let included = |id| draft.model_instances.contains(&id);
        let mut entries = column![].spacing(2).width(Length::Fill);
        let mut count = 0;
        for instance in &geometry.instances {
            let label = geometry
                .asset(instance.asset_id)
                .map(|asset| format!("{} · {}", asset.name, instance.name))
                .unwrap_or_else(|| instance.name.clone());
            if included(instance.id) {
                count += 1;
                let selected = self.setup_model_selection == Some(instance.id);
                entries = entries.push(
                    button(text(label).size(12).width(Length::Fill))
                        .width(Length::Fill)
                        .padding([8, 9])
                        .style(move |theme, status| inspector_tab(theme, status, selected))
                        .on_press(Message::SetupModelSelected(instance.id)),
                );
            }
        }
        if count == 0 {
            entries = entries.push(
                container(
                    text(if self.controller.has_geometry() {
                        "No models selected. Click Add, then pick bodies in the view."
                    } else {
                        "Import a model to add setup geometry."
                    })
                    .size(12)
                    .color(INSPECTOR_MUTED),
                )
                .padding(9),
            );
        }
        let mut form = column![
            text(format!("Models · {count}"))
                .size(11)
                .color(INSPECTOR_MUTED),
            container(scrollable(entries).height(120).width(Length::Fill))
                .padding(4)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.12, 0.14, 0.17))),
                    border: Border {
                        color: Color::from_rgb(0.25, 0.28, 0.32),
                        width: 1.0,
                        radius: 5.0.into()
                    },
                    ..container::Style::default()
                }),
        ]
        .spacing(8);
        if !geometry.instances.is_empty() {
            let remove = self
                .setup_model_selection
                .filter(|id| included(*id))
                .map(|id| Message::SetupModelToggled(id, false));
            form = form.push(
                row![
                    inspector_button(if self.setup_model_pick { "Done" } else { "Add" })
                        .on_press(Message::SetupModelPick),
                    inspector_button("Remove").on_press_maybe(remove),
                ]
                .spacing(6),
            );
            if self.setup_model_pick {
                form = form.push(
                    text("Click bodies in the view to add them. Click Done when finished.")
                        .size(11)
                        .color(INSPECTOR_MUTED),
                );
            }
        }
        form.into()
    }

    pub(super) fn setup_editor(&self) -> Element<'_, Message> {
        let Some(draft) = &self.setup_draft else {
            return column![
                inspector_heading("Setup"),
                text("Double-click a setup, or right-click it and choose Edit setup, to edit its settings.").size(12)
            ]
            .spacing(8)
            .padding(14)
            .into();
        };
        let tabs = row(SetupEditorTab::ALL.into_iter().map(|tab| {
            let active = tab == self.setup_tab;
            inspector_button(tab.to_string())
                .width(Length::FillPortion(if tab == SetupEditorTab::Postprocess {
                    3
                } else {
                    2
                }))
                .padding([8, 4])
                .style(move |theme, status| inspector_tab(theme, status, active))
                .on_press(Message::SetupTabChanged(tab))
                .into()
        }))
        .spacing(4);
        let header = container(
            column![
                text(format!("SETUP {}", draft.id))
                    .size(10)
                    .color(INSPECTOR_MUTED),
                inspector_heading(draft.name.clone()),
                tabs,
            ]
            .spacing(10),
        )
        .padding(14)
        .width(Length::Fill);
        // Leave a gutter for the scrollbar so fields never touch the panel edge.
        let mut list = column![]
            .spacing(8)
            .padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 16.0,
                left: 14.0,
            })
            .width(Length::Fill);
        match self.setup_tab {
            SetupEditorTab::Setup => {
                list = list.push(inspector_section("General"));
                list = list.push(text("Setup name").size(12).color(INSPECTOR_TEXT));
                list = list.push(
                    inspector_input("Setup name", &draft.name).on_input(Message::SetupNameChanged),
                );
                list = list.push(inspector_section("Geometry"));
                list = list.push(self.setup_geometry_list(draft));
                list = list.push(self.setup_wcs_editor(draft));
            }
            SetupEditorTab::Stock => {
                list = list.push(inspector_section("Material"));
                list = list.push(self.stock_material_picker(draft));
                list = list.push(inspector_section("Stock definition"));
                let kind = match draft.stock {
                    cam_model::Stock::FixedBox { .. } => StockEditorKind::FixedBox,
                    cam_model::Stock::RelativeBox { .. } => StockEditorKind::RelativeBox,
                    cam_model::Stock::BoundingBox { .. } => StockEditorKind::LegacyBox,
                    cam_model::Stock::PreviousSetup => StockEditorKind::PreviousSetup,
                };
                list = list.push(
                    pick_list(
                        if self
                            .controller
                            .setups()
                            .first()
                            .is_some_and(|s| s.id != draft.id)
                        {
                            &StockEditorKind::ALL[..]
                        } else {
                            &StockEditorKind::ALL[..3]
                        },
                        Some(kind),
                        Message::SetupStockKindChanged,
                    )
                    .text_size(12)
                    .padding([7, 9])
                    .style(inspector_picker)
                    .width(Length::Fill),
                );
                match draft.stock {
                    cam_model::Stock::PreviousSetup => {
                        let previous = self
                            .controller
                            .setups()
                            .iter()
                            .take_while(|s| s.id != draft.id)
                            .last();
                        list = list.push(text(previous.map_or_else(
                            || "The first setup needs box stock.".into(),
                            |s| format!("Uses the remaining material after {}. Included operations are recalculated when generating or simulating.", s.name),
                        )).size(11).color(INSPECTOR_MUTED));
                    }
                    cam_model::Stock::FixedBox {
                        size,
                        model_position,
                    } => {
                        list = list.push(inspector_section("Size and placement"));
                        for i in 0..3 {
                            let value = self.setup_value(20 + i, fmt_num(size[i]));
                            list = list.push(inspector_value(
                                format!("{} size", ["X", "Y", "Z"][i]),
                                &value,
                                "mm",
                                move |v| Message::SetupStockNumberChanged(i, v),
                            ));
                            list = list.push(
                                row![
                                    text("Model position")
                                        .size(11)
                                        .color(INSPECTOR_MUTED)
                                        .width(Length::Fill),
                                    pick_list(
                                        &BoxAnchor::ALL[..],
                                        Some(BoxAnchor::of(model_position[i])),
                                        move |anchor| Message::SetupStockPositionChanged(i, anchor)
                                    )
                                    .text_size(12)
                                    .padding([7, 9])
                                    .style(inspector_picker)
                                    .width(120),
                                ]
                                .spacing(8)
                                .align_y(Alignment::Center),
                            );
                        }
                    }
                    cam_model::Stock::RelativeBox { offsets } => {
                        list = list.push(inspector_section("Stock allowance"));
                        for (side, side_offsets) in offsets.iter().enumerate() {
                            for (axis, offset) in side_offsets.iter().enumerate() {
                                let i = side * 3 + axis;
                                let value = self.setup_value(20 + i, fmt_num(*offset));
                                list = list.push(inspector_value(
                                    format!(
                                        "{} {}",
                                        if side == 0 { "−" } else { "+" },
                                        ["X", "Y", "Z"][axis]
                                    ),
                                    &value,
                                    "mm",
                                    move |v| Message::SetupStockNumberChanged(i, v),
                                ));
                            }
                        }
                    }
                    cam_model::Stock::BoundingBox {
                        x_offset,
                        y_offset,
                        top,
                        thickness,
                    } => {
                        list = list.push(inspector_section("Stock dimensions"));
                        for (label, key, value) in [
                            ("X allowance", 50, x_offset),
                            ("Y allowance", 51, y_offset),
                            ("Top Z", 52, top),
                            ("Thickness", 53, thickness),
                        ] {
                            let value = self.setup_value(20 + key, fmt_num(value));
                            list = list.push(inspector_value(label, &value, "mm", move |v| {
                                Message::SetupStockNumberChanged(key, v)
                            }));
                        }
                    }
                }
                let (model_min, model_max) = self.controller.pending_setup_model_bounds(draft);
                let dimensions = std::array::from_fn::<_, 3, _>(|i| model_max[i] - model_min[i]);
                list = list.push(inspector_section("Model bounds"));
                list = list.push(
                    text(format!(
                        "{:.3} × {:.3} × {:.3} mm",
                        dimensions[0], dimensions[1], dimensions[2]
                    ))
                    .size(11)
                    .color(INSPECTOR_MUTED),
                );
                let (stock_min, stock_max) = self.controller.pending_setup_stock_bounds(draft);
                if draft.stock != cam_model::Stock::PreviousSetup
                    && (0..3).any(|i| {
                        stock_min[i] > model_min[i] + 1e-6 || stock_max[i] < model_max[i] - 1e-6
                    })
                {
                    list = list.push(
                        text("Stock does not contain the selected machining model.")
                            .size(11)
                            .color(palette::ERROR),
                    );
                }
            }
            SetupEditorTab::Postprocess => {
                list = list.push(inspector_section("Program output"));
                let offset = self.setup_value(40, draft.origin_index.to_string());
                list = list.push(inspector_value(
                    "Work offset",
                    &offset,
                    "",
                    Message::SetupWorkOffsetChanged,
                ));
                list = list.push(text("Program name / number").size(12).color(INSPECTOR_TEXT));
                list = list.push(
                    inspector_input("Setup program", &draft.program_name)
                        .on_input(Message::SetupProgramChanged),
                );
                list = list.push(
                    text("Each physical setup exports as a separate NC program.")
                        .size(11)
                        .color(INSPECTOR_MUTED),
                );
            }
        }
        let numbers_valid = self
            .setup_values
            .values()
            .all(|v| v.trim().parse::<f64>().is_ok())
            && self
                .setup_values
                .get(&40)
                .is_none_or(|v| v.trim().parse::<u32>().is_ok());
        let can_commit = numbers_valid && self.setup_frame_issue(draft).is_none();
        let footer = inspector_footer(
            row![
                inspector_button("Cancel").on_press(Message::SetupCancel),
                Space::new().width(Length::Fill),
                inspector_button("Apply setup")
                    .style(inspector_primary)
                    .on_press_maybe(can_commit.then_some(Message::SetupCommit)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        column![
            header,
            scrollable(list).width(Length::Fill).height(Length::Fill),
            footer
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
