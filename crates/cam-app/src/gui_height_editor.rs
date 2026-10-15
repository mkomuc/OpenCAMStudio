//! Transactional reference/offset controls shared by every operation kind.
use super::*;
use cam_model::{
    HeightPlaneKind as Plane, HeightReference as Reference, OperationHeightSettings,
    ResolvedOperationHeights,
};
use iced::widget::column;

pub(super) fn plane_input_id(plane: Plane) -> String {
    format!("plane-offset-{plane:?}")
}
impl App {
    pub(super) fn seed_height_editor(&mut self, id: u32) {
        self.height_draft_id = Some(id);
        self.height_reference_pick = None;
        let Some(op) = self.controller.operation(id) else {
            return;
        };
        let settings = self
            .controller
            .document()
            .setup()
            .height_settings
            .get(&id)
            .cloned()
            .unwrap_or_else(|| OperationHeightSettings::legacy(self.controller.document(), op));
        self.height_offsets = Plane::ALL
            .into_iter()
            .map(|p| (p, fmt_num(settings.definition(p).offset)))
            .collect();
        self.height_draft = Some(settings);
    }
    pub(super) fn height_rules_dirty(&self) -> bool {
        let (Some(id), Some(draft)) = (self.height_draft_id, self.height_draft.as_ref()) else {
            return false;
        };
        if self.controller.selection() != Selection::Operation(id) {
            return false;
        }
        let Some(op) = self.controller.operation(id) else {
            return false;
        };
        let baseline = self
            .controller
            .document()
            .setup()
            .height_settings
            .get(&id)
            .cloned()
            .unwrap_or_else(|| OperationHeightSettings::legacy(self.controller.document(), op));
        draft != &baseline
            || Plane::ALL.into_iter().any(|p| {
                self.height_offsets.get(&p).is_some_and(|s| {
                    s.trim().parse::<f64>().ok() != Some(baseline.definition(p).offset)
                })
            })
    }
    pub(super) fn edited_height_settings(&self) -> Result<OperationHeightSettings, String> {
        let mut settings = self
            .height_draft
            .clone()
            .ok_or("Open an operation to edit its heights")?;
        settings.resolved = None;
        for plane in Plane::ALL {
            if settings.definition(plane).reference == Reference::Disabled {
                continue;
            }
            let value = self
                .height_offsets
                .get(&plane)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{plane}: offset must be a finite number"))?;
            settings.definition_mut(plane).offset = value;
        }
        settings.validate_structure()?;
        Ok(settings)
    }
    pub(super) fn resolve_height_editor(
        &self,
        id: u32,
    ) -> Result<ResolvedOperationHeights, String> {
        let mut document = self.controller.document().clone();
        for tool in &self.operation_tools {
            if !document
                .setup()
                .tools
                .iter()
                .any(|saved| saved.number == tool.number)
            {
                document.setup_mut().tools.push(*tool);
            }
        }
        let mut op = self
            .current_operation(id)
            .cloned()
            .ok_or("Operation unavailable")?;
        let parsed = self
            .fields
            .iter()
            .filter_map(|(field, value)| {
                value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| (*field, v))
            })
            .collect();
        apply_op_fields(&mut op, &parsed);
        if let Some(binding) = &self.operation_binding_preview {
            document.geometry.bindings.insert(id, binding.clone());
        }
        self.edited_height_settings()?.resolve(&document, &op)
    }
    pub(super) fn height_reference_selected(&mut self, plane: Plane, reference: Reference) {
        if let Some(settings) = &mut self.height_draft {
            let d = settings.definition_mut(plane);
            d.reference = reference;
            if matches!(reference, Reference::Highest | Reference::Lowest) && d.sources.is_empty() {
                d.sources = vec![Reference::StockTop, Reference::ModelTop];
            }
        }
        self.focused_height = Some(plane_input_id(plane));
        self.height_reference_pick = None;
    }
    pub(super) fn height_geometry_selected(&mut self, entity: EntityRef) {
        if let (Some(plane), Some(settings)) = (self.height_reference_pick, &mut self.height_draft)
        {
            let d = settings.definition_mut(plane);
            if let Some(index) = d.entities.iter().position(|r| r == &entity) {
                d.entities.remove(index);
            } else {
                d.entities.push(entity);
            }
            self.focused_height = Some(plane_input_id(plane));
            self.status =
                "Height reference selected. Pick more geometry or choose Done picking.".into();
        }
    }
    pub(super) fn reference_height_controls(&self, id: u32) -> Element<'_, Message> {
        let Some(settings) = self.height_draft.as_ref() else {
            return text("Open the operation editor to set heights").into();
        };
        let resolved = self.resolve_height_editor(id);
        let mut controls = column![
            row![
                text("Reference planes and offsets")
                    .size(12)
                    .width(Length::Fill),
                inspector_button("Use defaults").on_press(Message::HeightDefaults),
            ]
            .spacing(8),
            text("Offsets are in mm along setup Z. Select an offset to preview the planes.")
                .size(11)
                .color(INSPECTOR_MUTED)
        ]
        .spacing(10);
        for plane in Plane::ALL {
            let d = settings.definition(plane);
            let collapsed = self.collapsed_heights.contains(&plane);
            let mut section =
                column![
                    inspector_button(format!("{} {plane}", if collapsed { "▸" } else { "▾" }))
                        .width(Length::Fill)
                        .on_press(Message::HeightSectionToggle(plane))
                ]
                .spacing(7);
            if !collapsed {
                section = section.push(
                    row![
                        text("From").size(12).width(50),
                        pick_list(Reference::options(plane), Some(d.reference), move |r| {
                            Message::HeightReferenceChanged(plane, r)
                        })
                        .text_size(12)
                        .padding([7, 9])
                        .width(Length::Fill)
                        .style(inspector_picker)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
                if d.reference != Reference::Disabled {
                    let value = self
                        .height_offsets
                        .get(&plane)
                        .cloned()
                        .unwrap_or_else(|| fmt_num(d.offset));
                    let invalid = value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .is_none_or(|v| !v.is_finite());
                    section = section.push(
                        row![
                            text("Offset").size(12).width(50),
                            inspector_input("0", &value)
                                .id(plane_input_id(plane))
                                .on_input(move |v| Message::HeightOffsetChanged(plane, v))
                                .on_submit(Message::Apply)
                                .align_x(iced::alignment::Horizontal::Right)
                                .width(Length::Fill)
                                .style(move |theme, status| {
                                    let mut style = inspector_input_style(theme, status);
                                    if invalid {
                                        style.border.color = palette::ERROR;
                                    }
                                    style
                                }),
                            text("mm").size(11)
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                    if matches!(d.reference, Reference::Highest | Reference::Lowest) {
                        section = section.push(
                            text("Compare these references:")
                                .size(11)
                                .color(INSPECTOR_MUTED),
                        );
                        for reference in Reference::options(plane).into_iter().filter(|r| {
                            !matches!(
                                r,
                                Reference::Disabled | Reference::Highest | Reference::Lowest
                            )
                        }) {
                            section = section.push(
                                checkbox(d.sources.contains(&reference))
                                    .label(reference.to_string())
                                    .size(14)
                                    .text_size(11)
                                    .on_toggle(move |on| {
                                        Message::HeightSourceToggled(plane, reference, on)
                                    }),
                            );
                        }
                    }
                    let needs_geometry = matches!(
                        d.reference,
                        Reference::Selection | Reference::FixtureTop | Reference::FixtureBottom
                    ) || d.sources.iter().any(|r| {
                        matches!(
                            r,
                            Reference::Selection | Reference::FixtureTop | Reference::FixtureBottom
                        )
                    }) && matches!(
                        d.reference,
                        Reference::Highest | Reference::Lowest
                    );
                    if needs_geometry {
                        section = section.push(
                            row![
                                inspector_button(if self.height_reference_pick == Some(plane) {
                                    "Done picking"
                                } else {
                                    "Pick reference geometry"
                                })
                                .on_press(Message::HeightPickToggle(plane)),
                                inspector_button("Clear")
                                    .on_press(Message::HeightSelectionClear(plane))
                            ]
                            .spacing(6),
                        );
                        section = section.push(
                            text(format!(
                                "{} selected · {}",
                                d.entities.len() + d.drawings.len(),
                                if matches!(
                                    d.reference,
                                    Reference::FixtureTop | Reference::FixtureBottom
                                ) {
                                    "pick fixture bodies"
                                } else {
                                    "pick faces, edges or vertices"
                                }
                            ))
                            .size(11)
                            .color(INSPECTOR_MUTED),
                        );
                        if self.height_reference_pick == Some(plane) {
                            section = section.push(
                                row![
                                    text("Pick").size(11),
                                    pick_list(
                                        &CadFilter::ALL[..],
                                        Some(self.cad_filter),
                                        Message::CadFilterChanged
                                    )
                                    .text_size(11)
                                    .style(inspector_picker)
                                ]
                                .spacing(8),
                            );
                        }
                        for (index, entity) in d.entities.iter().enumerate() {
                            section = section.push(
                                row![
                                    text(format!(
                                        "{:?} {} · instance {}",
                                        entity.kind, entity.id, entity.instance_id
                                    ))
                                    .size(11)
                                    .width(Length::Fill),
                                    inspector_button("×")
                                        .on_press(Message::HeightEntityRemove(plane, index))
                                ]
                                .spacing(6),
                            );
                        }
                        for (index, drawing) in d.drawings.iter().enumerate() {
                            section = section.push(
                                row![
                                    text(format!(
                                        "Drawing {} · {} points",
                                        if drawing.closed { "contour" } else { "path" },
                                        drawing.points.len()
                                    ))
                                    .size(11)
                                    .width(Length::Fill),
                                    inspector_button("×").on_press(Message::HeightEntityRemove(
                                        plane,
                                        d.entities.len() + index
                                    ))
                                ]
                                .spacing(6),
                            );
                        }
                    }
                    if let Ok(heights) = resolved {
                        if let Some(z) = heights.plane(plane) {
                            section = section.push(
                                text(format!("Z {} mm", fmt_num(z)))
                                    .size(11)
                                    .color(INSPECTOR_MUTED),
                            );
                        }
                    }
                }
            }
            controls = controls.push(container(section).padding([9, 0]).width(Length::Fill));
        }
        if let Err(reason) = resolved {
            controls = controls.push(text(reason).size(11).color(palette::ERROR));
        }
        controls.into()
    }
}
