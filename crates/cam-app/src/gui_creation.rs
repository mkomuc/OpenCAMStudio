//! Shared DXF/STEP operation creation controls.
use super::*;
use iced::widget::column;
impl App {
    pub(super) fn creating_operation(&self) -> bool {
        self.cad_pending_kind.is_some()
            || (self.controller.pending_op().is_some() && self.drawing_editing_op.is_none())
    }
    pub(super) fn wizard_tool(&self) -> Option<cam_model::Tool> {
        if self.cad_pending_kind.is_some() {
            self.library
                .tools
                .iter()
                .find(|tool| Some(tool.number) == self.cad_tool_pick)
                .copied()
        } else {
            let number = self.controller.pending_op()?.tool?;
            self.controller
                .document()
                .setup()
                .tools
                .iter()
                .find(|tool| tool.number == number)
                .copied()
        }
    }
    pub(super) fn wizard_seeded_cutting(&self) -> Option<CuttingData> {
        let tool = self.wizard_tool()?;
        let defaults = self.controller.defaults();
        Some(CuttingData {
            rpm: tool.nominal_rpm,
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
        })
    }
    pub(super) fn seed_wizard_cutting(&mut self) {
        if let Some(cutting) = self.wizard_seeded_cutting() {
            self.fields.insert(Field::SpindleRpm, fmt_num(cutting.rpm));
            self.fields.insert(Field::Feed, fmt_num(cutting.feed));
            self.fields
                .insert(Field::PlungeFeed, fmt_num(cutting.plunge_feed));
        }
    }
    pub(super) fn wizard_cutting(&self) -> Option<CuttingData> {
        let seeded = self.wizard_seeded_cutting()?;
        let parse = |field, fallback| {
            self.fields
                .get(&field)
                .map_or(Some(fallback), |text| text.parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        let cutting = CuttingData {
            rpm: parse(Field::SpindleRpm, seeded.rpm)?,
            feed: parse(Field::Feed, seeded.feed)?,
            plunge_feed: parse(Field::PlungeFeed, seeded.plunge_feed)?,
        };
        (cutting.rpm >= 0. && cutting.feed > 0. && cutting.plunge_feed > 0.).then_some(cutting)
    }
    pub(super) fn op_wizard(
        &self,
        operation_kind: OpKind,
        pending: Option<PendingOp>,
    ) -> Element<'_, Message> {
        let cad = self.cad_pending_kind.is_some();
        let geometry_selected = if cad {
            !self.controller.cad_selection().is_empty()
        } else {
            pending.as_ref().is_some_and(|p| p.boundary.is_some())
        };
        let tool = self.wizard_tool();
        let kind = match operation_kind {
            OpKind::Profile => "Profile",
            OpKind::Pocket => "Pocket",
            OpKind::Drill => "Drill",
            OpKind::Face => "Face",
            OpKind::Chamfer => "Chamfer",
            OpKind::Thread => "Thread",
            OpKind::Engrave => "Engrave",
            OpKind::Carve => "Carve",
        };
        // The tool is chosen in two steps — **family**, then a tool within it — so a
        // library of hundreds does not have to be scrolled for every operation. The
        // families offered are bounded by the operation (see `families_for`).
        let families = families_for(operation_kind);
        let family = self.wizard_family.filter(|f| families.contains(f));
        let family_picker = pick_list(families.to_vec(), family, Message::SetPendingFamily)
            .placeholder("Choose a family")
            .text_size(12)
            .padding([7, 9])
            .style(inspector_picker)
            .width(Length::Fill);

        // Tools of the chosen family only. Nothing is listed until a family is picked.
        let tools: Vec<ToolChoice> = match family {
            Some(f) => self
                .library
                .tools
                .iter()
                .enumerate()
                .filter(|(_, t)| ToolKindPick::of(t.kind) == f)
                .map(|(index, t)| ToolChoice {
                    index,
                    number: t.number,
                    diameter: t.diameter,
                    kind: t.kind,
                })
                .collect(),
            None => Vec::new(),
        };
        let selected = tool
            .and_then(|embedded| {
                self.library
                    .tools
                    .iter()
                    .position(|saved| saved.identity() == embedded.identity())
            })
            .and_then(|index| tools.iter().find(|choice| choice.index == index).copied());
        let tool_picker = pick_list(tools.clone(), selected, |c| {
            Message::SetPendingLibraryTool(c.index)
        })
        .placeholder(if family.is_some() {
            "Choose a tool"
        } else {
            "Choose a family first"
        })
        .text_size(12)
        .padding([7, 9])
        .style(inspector_picker)
        .width(Length::Fill);

        let verb = if pending
            .as_ref()
            .is_some_and(|pending| pending.replacing.is_some())
        {
            "Reinitialize"
        } else {
            "New"
        };
        let mut col = column![
            inspector_heading(format!("{verb} {kind} operation")),
            text("Tool family").size(12),
            family_picker,
            text("Tool").size(12),
            tool_picker,
        ]
        .spacing(10)
        .padding(14);

        // A family with nothing in it is a dead end now that the wizard has no
        // "New tool" shortcut — say where to go rather than leaving an empty list.
        if let Some(f) = family {
            if tools.is_empty() {
                col = col.push(
                    text(format!(
                        "No {f} in the library — add one in the Tooling tab."
                    ))
                    .size(12)
                    .color(palette::WARN),
                );
            }
        }

        // Cutting data for the new operation, seeded from the tool's nominals and
        // editable here (or later in the operation's inspector). Shown once a tool is
        // chosen — before then there is nothing to seed from.
        if tool.is_some() {
            col = col.push(inspector_section("Cutting data"));
            for f in [Field::SpindleRpm, Field::Feed, Field::PlungeFeed] {
                let value = self.fields.get(&f).cloned().unwrap_or_default();
                col = col.push(field_row_labeled(
                    f,
                    self.field_label(f),
                    self.field_help(f),
                    &value,
                    self.tooltips,
                    self.field_invalid(f),
                ));
            }
        }

        // Object snaps govern where the start/lead-in lands; show them for the
        // kinds that carry a start, while still awaiting that (boundary) pick.
        if !cad && op_uses_snaps(operation_kind) && !geometry_selected {
            col = col.push(self.snap_toolbar());
        }

        let geometry_ready = if cad {
            col = col.push(inspector_section("Geometry"));
            col =
                col.push(self.geometry_list(cad_geometry_labels(self.controller.cad_selection())));
            let prompt = match operation_kind {
                OpKind::Face => "Select a planar top face. Facing clears the whole stock top.",
                OpKind::Pocket => "Select a planar pocket floor in the viewport.",
                OpKind::Profile => {
                    "Select a closed boundary. For a blind bore, select its floor loop."
                }
                OpKind::Drill => {
                    "Select cylindrical hole faces. Select adds holes; Remove deletes a list item."
                }
                _ => "Select geometry in the viewport. Remove list items to replace them.",
            };
            col = col.push(text(prompt).size(11).color(INSPECTOR_MUTED));
            if self.cad_candidates.len() > 1 {
                col = col.push(
                    text("Overlapping candidates")
                        .size(11)
                        .color(INSPECTOR_MUTED),
                );
                for candidate in &self.cad_candidates {
                    col = col.push(
                        inspector_button(format!(
                            "{:?} {}",
                            candidate.entity.kind, candidate.entity.id
                        ))
                        .on_press(Message::CadSelect(candidate.entity.clone())),
                    );
                }
            }
            match self
                .controller
                .preview_cad_operation(operation_kind, self.controller.cad_selection())
            {
                Ok(input) => {
                    col = col.push(
                        text(format!(
                            "{} feature(s) · top Z {:.3} / bottom Z {:.3} mm",
                            self.controller.cad_selection().len(),
                            input.heights.top,
                            input.heights.bottom
                        ))
                        .size(12),
                    );
                    true
                }
                Err(reason) => {
                    if geometry_selected {
                        col = col.push(text(reason).size(12).color(palette::WARN));
                    }
                    false
                }
            }
        } else {
            col = col.push(inspector_section("Geometry"));
            col = col.push(self.geometry_list(drawing_geometry_labels(
                &self.controller.pending_operation_geometry(),
            )));
            self.controller.pending_ready()
        };
        // Progress, then the one commit point. Tool and geometry may be chosen in
        // either order; Confirm stays disabled until both are settled.
        col = col.push(
            text(match (tool.is_some(), geometry_selected) {
                (false, false) => {
                    "Choose a tool and click the geometry in the viewport.".to_string()
                }
                (true, false) => "Now click the geometry in the viewport.".to_string(),
                (false, true) => "Geometry selected — now choose a tool.".to_string(),
                (true, true) if cad && !geometry_ready => {
                    "Review the selected geometry before confirming.".to_string()
                }
                (true, true) if !cad && op_takes_islands(operation_kind) => format!(
                    "Click enclosed areas to exclude ({} selected), then Confirm.",
                    pending.as_ref().map_or(0, |pending| pending.islands.len())
                ),
                (true, true) => "Ready — Confirm to create the operation.".to_string(),
            })
            .size(12),
        );
        let footer = inspector_footer(
            row![
                inspector_button("Cancel").on_press(Message::CancelOp),
                Space::new().width(Length::Fill),
                inspector_button("Confirm")
                    .style(inspector_primary)
                    .on_press_maybe(
                        (geometry_ready && self.wizard_cutting().is_some())
                            .then_some(Message::ConfirmOp)
                    ),
            ]
            .spacing(8),
        );
        column![
            scrollable(col).width(Length::Fill).height(Length::Fill),
            footer
        ]
        .height(Length::Fill)
        .into()
    }
}
