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
                .chain(self.operation_tools.iter())
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

    pub(super) fn seed_creation_editor(&mut self, kind: OpKind) {
        self.operation_tab = OperationTab::Tool;
        self.creation_edited_fields.clear();
        self.inspector_active = true;
        self.operation_binding_preview = None;
        self.operation_geometry = None;
        self.operation_draft = Some(self.controller.operation_template(kind, 0));
        self.height_draft_id = Some(0);
        let settings = cam_model::OperationHeightSettings::default();
        self.height_offsets = cam_model::HeightPlaneKind::ALL
            .into_iter()
            .map(|p| (p, fmt_num(settings.definition(p).offset)))
            .collect();
        self.height_draft = Some(settings);
        self.sync_creation_tool();
        let work_offset = self.controller.active_origin();
        self.edit_operation_draft(|op| op.set_work_offset(work_offset));
        self.refresh_fields();
        self.seed_wizard_cutting();
    }

    pub(super) fn sync_creation_tool(&mut self) {
        if let Some(tool) = self.wizard_tool() {
            let number = self.stage_operation_tool(tool);
            self.edit_operation_draft(|op| op.set_tool(number));
        }
    }

    pub(super) fn creation_operation(
        &self,
    ) -> Result<(Operation, Option<cam_model::GeometryBinding>), String> {
        let mut draft = self
            .operation_draft
            .clone()
            .ok_or("Operation unavailable")?;
        self.wizard_tool().ok_or("Choose a cutter")?;
        self.wizard_cutting().ok_or("Enter valid cutting data")?;
        let fields = self
            .inspector_fields()
            .into_iter()
            .map(|field| {
                self.fields
                    .get(&field)
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|v| {
                        v.is_finite() && !operation_tabs::operation_field_invalid(&draft, field, *v)
                    })
                    .map(|v| (field, v))
                    .ok_or("Correct invalid operation fields")
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        apply_op_fields(&mut draft, &fields);
        if let Some(kind) = self.cad_pending_kind {
            let mut document = self.controller.document().clone();
            document
                .setup_mut()
                .tools
                .extend(self.operation_tools.iter().copied());
            let (geometry, binding) = crate::cad_operations::create_operation(
                &document,
                kind,
                self.controller.cad_selection().to_vec(),
                0,
                draft.tool(),
                self.controller.active_origin(),
                *self.controller.defaults(),
            )?;
            match (&mut draft, geometry) {
                (Operation::Profile(op), Operation::Profile(new)) => {
                    op.chain = new.chain;
                    op.side = new.side;
                    op.comp = new.comp;
                }
                (Operation::Pocket(op), Operation::Pocket(new)) => {
                    op.boundary = new.boundary;
                    op.islands = new.islands;
                }
                (Operation::Face(op), Operation::Face(new)) => op.boundary = new.boundary,
                (Operation::Drill(op), Operation::Drill(new)) => op.points = new.points,
                _ => return Err("Unsupported CAD operation".into()),
            }
            Ok((draft, Some(binding)))
        } else {
            let pending = self
                .controller
                .pending_op()
                .ok_or("Operation unavailable")?;
            if !self.controller.pending_ready() {
                return Err("Select compatible geometry and a cutter".into());
            }
            let mut operation = self.controller.operation_with_drawing_geometry(
                &draft,
                &self.controller.pending_operation_geometry(),
            )?;
            if self.creation_edited_fields.contains(&Field::MajorDia) {
                if let (Operation::Thread(op), Operation::Thread(edited)) = (&mut operation, &draft)
                {
                    op.major_dia = edited.major_dia;
                }
            }
            if operation_tabs::operation_start(&operation).is_none() {
                if let Some(start) = pending.start {
                    operation_tabs::set_operation_start(&mut operation, true);
                    operation_tabs::set_operation_start_coordinate(&mut operation, 0, start[0]);
                    operation_tabs::set_operation_start_coordinate(&mut operation, 1, start[1]);
                }
            }
            Ok((operation, None))
        }
    }

    pub(super) fn creation_ready(&self) -> bool {
        self.creation_operation()
            .and_then(|(op, binding)| {
                let mut document = self.controller.document().clone();
                document
                    .setup_mut()
                    .tools
                    .extend(self.operation_tools.iter().copied());
                if let Some(binding) = binding {
                    document.geometry.bindings.insert(0, binding);
                }
                self.edited_height_settings()?
                    .resolve(&document, &op)
                    .map(|_| ())
            })
            .is_ok()
    }

    pub(super) fn creation_geometry_controls(&self) -> Element<'_, Message> {
        let kind = self
            .cad_pending_kind
            .or_else(|| self.controller.pending_op().map(|p| p.kind))
            .unwrap();
        let mut col = column![].spacing(10);
        if self.cad_pending_kind.is_some() {
            col =
                col.push(self.geometry_list(cad_geometry_labels(self.controller.cad_selection())));
            col = col.push(text(match kind {
                OpKind::Face => "Select a planar top face, or Ctrl-click for the whole-body XY outline.",
                OpKind::Pocket => "Select a planar pocket floor in the viewport.",
                OpKind::Profile => "Select a closed boundary or Ctrl-click for its XY silhouette. For a blind bore, select its floor loop.",
                OpKind::Drill => "Select cylindrical hole faces. Select adds holes; Remove deletes a list item.",
                _ => "Select geometry in the viewport.",
            }).size(11).color(INSPECTOR_MUTED));
            if let Some(note) = cad_silhouette_note(
                self.controller.geometry_state(),
                self.controller.cad_selection(),
            ) {
                col = col.push(text(note).size(11).color(INSPECTOR_MUTED));
            }
            for candidate in &self.cad_candidates {
                col = col.push(
                    inspector_button(format!(
                        "{:?} {}",
                        candidate.entity.kind, candidate.entity.id
                    ))
                    .on_press(Message::CadSelect(candidate.entity.clone())),
                );
            }
            if let Err(reason) = self
                .controller
                .preview_cad_operation(kind, self.controller.cad_selection())
            {
                if !self.controller.cad_selection().is_empty() {
                    col = col.push(text(reason).size(12).color(palette::WARN));
                }
            }
        } else {
            if op_uses_snaps(kind) {
                col = col.push(self.snap_toolbar());
            }
            col = col.push(self.geometry_list(drawing_geometry_labels(
                &self.controller.pending_operation_geometry(),
            )));
            col = col.push(
                text("Select geometry in the viewport. Ctrl/Command adds compatible features.")
                    .size(11)
                    .color(INSPECTOR_MUTED),
            );
        }
        col.into()
    }
}
