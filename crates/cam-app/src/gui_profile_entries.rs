//! Named geometry collections and their transactional configuration submenu.
use super::geometry_selection::{
    geometry_collection, selection_hint as hint, selection_icon as icon, GeometryRow,
};
use super::*;
use cam_model::*;
use iced::widget::column;

#[derive(Clone, Debug)]
pub(super) enum EntryMessage {
    Add(ProfileGeometryKind),
    EditSelected,
    RemoveSelected,
    Pick,
    ClearSource,
    Name(String),
    Coplanar(bool),
    Curves(ProfileCurves),
    Side(ProfileSideRule),
    ChainMode(ProfileChainMode),
    Reverse(bool),
    SetupModels(bool),
    Tolerance(String),
    Confirm,
    Cancel,
}
pub(super) struct EntryEditor {
    pub index: usize,
    pub draft: ProfileGeometryEntry,
    pub picking: bool,
    tolerance: String,
    previous: Option<Vec<ProfileGeometryEntry>>,
    previous_row: Option<usize>,
    previous_filter_explicit: bool,
    error: Option<String>,
}

fn kind_name(kind: ProfileGeometryKind) -> &'static str {
    match kind {
        ProfileGeometryKind::FaceContours => "Face Contours",
        ProfileGeometryKind::Chain => "Chain",
        ProfileGeometryKind::Silhouette => "Silhouette",
    }
}
fn kind_icon(kind: ProfileGeometryKind) -> &'static str {
    match kind {
        ProfileGeometryKind::FaceContours => "face",
        ProfileGeometryKind::Chain => "chain",
        ProfileGeometryKind::Silhouette => "silhouette",
    }
}

impl App {
    pub(super) fn profile_entries(&self) -> Vec<ProfileGeometryEntry> {
        let Some(Operation::Profile(profile)) = &self.operation_draft else {
            return Vec::new();
        };
        if let Some(entries) = &profile.selections {
            return entries.clone();
        }
        let refs = if self.cad_pending_kind.is_some() || self.cad_editing_op.is_some() {
            self.controller.cad_selection().to_vec()
        } else {
            self.operation_binding_preview
                .as_ref()
                .or_else(|| self.controller.geometry_state().bindings.get(&profile.id))
                .map(|b| b.selected.clone())
                .unwrap_or_default()
        };
        let mut entries = Vec::new();
        for reference in refs {
            let kind = match reference.kind {
                EntityKind::Face => ProfileGeometryKind::FaceContours,
                EntityKind::Silhouette | EntityKind::Body => ProfileGeometryKind::Silhouette,
                _ => ProfileGeometryKind::Chain,
            };
            let mut entry = ProfileGeometryEntry::new(
                kind,
                format!("{} {}", kind_name(kind), entries.len() + 1),
            );
            entry.use_setup_models = false;
            // The older silhouette recipe contained only the body envelope.
            // Configuring it must not silently add through-hole contours.
            if kind == ProfileGeometryKind::Silhouette
                && self
                    .controller
                    .geometry_state()
                    .bindings
                    .get(&profile.id)
                    .is_some_and(|b| matches!(b.recipe, ExtractionRecipe::ProfileLoop { .. }))
            {
                entry.curves = ProfileCurves::Outer;
            }
            entry.source = ProfileGeometrySource::Cad(vec![reference]);
            entries.push(entry);
        }
        if entries.is_empty() && (self.cad_pending_kind.is_some() || self.cad_editing_op.is_some())
        {
            return entries;
        }
        if entries.is_empty() {
            let contours = if self.controller.pending_op().is_some() {
                self.controller
                    .pending_operation_geometry()
                    .into_iter()
                    .filter_map(|r| {
                        self.controller
                            .loop_points(r)
                            .map(|(points, closed)| ProfileContour {
                                chain: cam_geo::Contour::new(points),
                                closed,
                                side: if matches!(r.part, crate::LoopPart::Hole(_)) {
                                    Side::Inside
                                } else {
                                    Side::Outside
                                },
                            })
                    })
                    .collect()
            } else {
                profile.contours().collect::<Vec<_>>()
            };
            for contour in contours {
                let mut entry = ProfileGeometryEntry::new(
                    ProfileGeometryKind::Chain,
                    format!(
                        "{} Chain {}",
                        if contour.closed { "Closed" } else { "Open" },
                        entries.len() + 1
                    ),
                );
                entry.chain_mode = if contour.closed {
                    ProfileChainMode::Closed
                } else {
                    ProfileChainMode::Open
                };
                entry.source = ProfileGeometrySource::Contours(vec![contour]);
                entries.push(entry);
            }
        } else if let Ok(geometry) =
            crate::cad_operations::geometry_for_setup(self.controller.document())
        {
            let saved: Vec<_> = profile.contours().collect();
            for entry in &mut entries {
                if let Ok((contours, _)) = cam_geometry3d::profile_entry_contours(&geometry, entry)
                {
                    for (index, contour) in contours.iter().enumerate() {
                        if let Some(old) = saved
                            .iter()
                            .find(|p| p.chain == contour.chain && p.closed == contour.closed)
                        {
                            if old.side != contour.side {
                                entry.side_overrides.insert(index, old.side);
                            }
                        }
                    }
                }
            }
        }
        entries
    }
    pub(super) fn preview_profile_entries(&self) -> Vec<ProfileGeometryEntry> {
        let mut entries = self.profile_entries();
        if let Some(editor) = &self.profile_entry_editor {
            if let Some(entry) = entries.get_mut(editor.index) {
                *entry = editor.draft.clone();
            }
        }
        entries
    }
    fn setup_profile_bodies(&self) -> Vec<EntityRef> {
        self.controller
            .geometry_state()
            .instances
            .iter()
            .filter(|i| {
                self.controller
                    .document()
                    .setup()
                    .model_instances
                    .contains(&i.id)
            })
            .filter_map(|instance| {
                let asset = self.controller.geometry_state().asset(instance.asset_id)?;
                Some(EntityRef {
                    asset_id: asset.id,
                    revision: asset.revision,
                    instance_id: instance.id,
                    body_id: instance.body_id,
                    kind: EntityKind::Silhouette,
                    id: instance.body_id,
                })
            })
            .collect()
    }
    pub(super) fn profile_entry_message(&mut self, message: EntryMessage) {
        match message {
            EntryMessage::Add(kind) => {
                if self.profile_entry_editor.is_some() {
                    return;
                }
                let previous = match &self.operation_draft {
                    Some(Operation::Profile(p)) => p.selections.clone(),
                    _ => None,
                };
                let previous_row = self.geometry_row_selection;
                let mut entries = self.profile_entries();
                let index = entries.len();
                let mut entry =
                    ProfileGeometryEntry::new(kind, format!("{} {}", kind_name(kind), index + 1));
                if kind == ProfileGeometryKind::Silhouette {
                    entry.source = ProfileGeometrySource::Cad(self.setup_profile_bodies());
                }
                entries.push(entry.clone());
                self.edit_operation_draft(|op| {
                    if let Operation::Profile(p) = op {
                        p.selections = Some(entries);
                    }
                });
                self.geometry_row_selection = Some(index);
                self.profile_entry_editor = Some(EntryEditor {
                    index,
                    draft: entry,
                    picking: false,
                    tolerance: "0.010".into(),
                    previous,
                    previous_row,
                    previous_filter_explicit: self.cad_filter_explicit,
                    error: None,
                });
                self.operation_geometry = None;
                self.operation_binding_preview = None;
                self.profile_entry_start_pick();
            }
            EntryMessage::EditSelected => {
                if self.profile_entry_editor.is_some() {
                    return;
                }
                let previous = match &self.operation_draft {
                    Some(Operation::Profile(p)) => p.selections.clone(),
                    _ => None,
                };
                let entries = self.profile_entries();
                let index = self
                    .geometry_row_selection
                    .filter(|i| *i < entries.len())
                    .unwrap_or(0);
                let previous_row = self.geometry_row_selection;
                if let Some(entry) = entries.get(index).cloned() {
                    let tolerance = fmt_num(entry.tolerance);
                    self.edit_operation_draft(|op| {
                        if let Operation::Profile(p) = op {
                            p.selections = Some(entries);
                        }
                    });
                    self.profile_entry_editor = Some(EntryEditor {
                        index,
                        draft: entry,
                        picking: false,
                        tolerance,
                        previous,
                        previous_row,
                        previous_filter_explicit: self.cad_filter_explicit,
                        error: None,
                    });
                    self.operation_geometry = None;
                    self.operation_binding_preview = None;
                }
            }
            EntryMessage::RemoveSelected => {
                if self.profile_entry_editor.is_some() {
                    return;
                }
                let mut entries = self.profile_entries();
                let index = self.geometry_row_selection.unwrap_or(0);
                if index < entries.len() {
                    entries.remove(index);
                    self.geometry_row_selection = if entries.is_empty() {
                        None
                    } else {
                        Some(index.min(entries.len() - 1))
                    };
                }
                self.edit_operation_draft(|op| {
                    if let Operation::Profile(p) = op {
                        p.selections = Some(entries);
                    }
                });
                self.operation_geometry = None;
                self.operation_binding_preview = None;
                self.profile_side_overrides.clear();
                self.sync_profile_entry_cache();
            }
            EntryMessage::Pick => self.profile_entry_start_pick(),
            EntryMessage::Confirm => {
                let Some(mut editor) = self.profile_entry_editor.take() else {
                    return;
                };
                match editor.tolerance.trim().parse::<f64>() {
                    Ok(value) if value.is_finite() && value > 0. => editor.draft.tolerance = value,
                    _ => {
                        editor.error =
                            Some("Enter a positive, finite tolerance in millimetres.".into());
                        self.profile_entry_editor = Some(editor);
                        return;
                    }
                }
                if editor.draft.name.trim().is_empty() {
                    editor.draft.name =
                        format!("{} {}", kind_name(editor.draft.kind), editor.index + 1);
                }
                let result = crate::cad_operations::geometry_for_setup(self.controller.document())
                    .and_then(|g| {
                        cam_geometry3d::profile_entry_contours(&g, &editor.draft).map(|_| ())
                    });
                if let Err(error) = result {
                    self.report_error(format!(
                        "Profile geometry entry '{}' rejected: {error}",
                        editor.draft.name
                    ));
                    editor.error = Some(error);
                    self.profile_entry_editor = Some(editor);
                    return;
                }
                self.edit_operation_draft(|op| {
                    if let Operation::Profile(p) = op {
                        if let Some(entry) = p
                            .selections
                            .as_mut()
                            .and_then(|entries| entries.get_mut(editor.index))
                        {
                            *entry = editor.draft;
                        }
                    }
                });
                self.cad_filter_explicit = editor.previous_filter_explicit;
                self.geometry_additive = false;
                self.profile_side_overrides.clear();
                self.sync_profile_entry_cache();
                self.refresh_operation_fields();
            }
            EntryMessage::Cancel => {
                if let Some(editor) = self.profile_entry_editor.take() {
                    self.geometry_row_selection = editor.previous_row;
                    self.edit_operation_draft(|op| {
                        if let Operation::Profile(p) = op {
                            p.selections = editor.previous;
                        }
                    });
                    self.cad_filter_explicit = editor.previous_filter_explicit;
                }
                self.geometry_additive = false;
            }
            EntryMessage::SetupModels(enabled) => {
                let bodies = enabled.then(|| self.setup_profile_bodies());
                if let Some(editor) = &mut self.profile_entry_editor {
                    editor.draft.use_setup_models = enabled;
                    if let Some(bodies) = bodies {
                        editor.draft.source = ProfileGeometrySource::Cad(bodies);
                    }
                    editor.error = None;
                }
            }
            message => {
                if let Some(editor) = &mut self.profile_entry_editor {
                    editor.error = None;
                    match message {
                        EntryMessage::Name(name) => editor.draft.name = name,
                        EntryMessage::ClearSource => {
                            editor.draft.source = ProfileGeometrySource::Cad(Vec::new())
                        }
                        EntryMessage::Coplanar(value) => editor.draft.coplanar = value,
                        EntryMessage::Curves(value) => {
                            editor.draft.curves = value;
                            editor.draft.side_overrides.clear();
                        }
                        EntryMessage::Side(value) => {
                            editor.draft.side_rule = value;
                            editor.draft.side_overrides.clear();
                        }
                        EntryMessage::ChainMode(value) => {
                            editor.draft.chain_mode = value;
                            self.profile_chain_edges = value == ProfileChainMode::Open;
                        }
                        EntryMessage::Reverse(value) => editor.draft.reversed = value,
                        EntryMessage::Tolerance(value) => editor.tolerance = value,
                        _ => {}
                    }
                }
            }
        }
    }
    fn profile_entry_start_pick(&mut self) {
        if let Some(editor) = &mut self.profile_entry_editor {
            editor.picking = true;
            self.profile_selection_mode = match editor.draft.kind {
                ProfileGeometryKind::FaceContours => ProfileSelectionMode::FaceContours,
                ProfileGeometryKind::Chain => ProfileSelectionMode::Chain,
                ProfileGeometryKind::Silhouette => ProfileSelectionMode::Silhouette,
            };
            self.profile_chain_edges = editor.draft.kind == ProfileGeometryKind::Chain
                && editor.draft.chain_mode == ProfileChainMode::Open;
            self.cad_filter_explicit = true;
            self.profile_tab_picking = false;
            self.geometry_additive = true;
            self.status = format!(
                "{}: select geometry in the viewport, then OK to keep this entry.",
                kind_name(editor.draft.kind)
            );
        }
    }
    pub(super) fn profile_entry_cad_pick(&mut self, entity: EntityRef) -> bool {
        let Some(editor) = self.profile_entry_editor.as_mut().filter(|e| e.picking) else {
            return false;
        };
        let expected = match editor.draft.kind {
            ProfileGeometryKind::FaceContours => EntityKind::Face,
            ProfileGeometryKind::Silhouette => EntityKind::Silhouette,
            ProfileGeometryKind::Chain => entity.kind,
        };
        if entity.kind != expected {
            self.status = format!(
                "Select {} geometry for this entry.",
                kind_name(editor.draft.kind)
            );
            return true;
        }
        if !self.controller.geometry_state().entity_exists(&entity) {
            self.status = "Source geometry is missing or stale.".into();
            return true;
        }
        if editor.draft.kind == ProfileGeometryKind::Silhouette && editor.draft.use_setup_models {
            editor.draft.source = ProfileGeometrySource::Cad(Vec::new());
        }
        let refs = match &mut editor.draft.source {
            ProfileGeometrySource::Cad(refs) => refs,
            _ => {
                editor.draft.source = ProfileGeometrySource::Cad(Vec::new());
                let ProfileGeometrySource::Cad(refs) = &mut editor.draft.source else {
                    unreachable!()
                };
                refs
            }
        };
        if editor.draft.kind == ProfileGeometryKind::Chain
            && refs.first().is_some_and(|old| old.kind != entity.kind)
        {
            refs.clear();
        }
        if let Some(index) = refs.iter().position(|old| *old == entity) {
            refs.remove(index);
        } else {
            refs.push(entity);
        }
        editor.draft.use_setup_models = false;
        editor.draft.side_overrides.clear();
        editor.error = None;
        true
    }
    pub(super) fn profile_entry_drawing_pick(&mut self, world: [f32; 2], aperture: f32) -> bool {
        if !self
            .profile_entry_editor
            .as_ref()
            .is_some_and(|e| e.picking)
        {
            return false;
        }
        let hit = self
            .controller
            .nearest_drawing_feature(world.map(f64::from), aperture as f64)
            .and_then(|(r, _)| {
                self.controller
                    .loop_points(r)
                    .map(|(points, closed)| (r, points, closed))
            });
        let Some((reference, points, closed)) = hit else {
            return true;
        };
        let editor = self.profile_entry_editor.as_mut().unwrap();
        if editor.draft.kind != ProfileGeometryKind::Chain {
            self.status = "Select a CAD face or body for this entry.".into();
            return true;
        }
        let contour = ProfileContour {
            chain: cam_geo::Contour::new(points),
            closed,
            side: if matches!(reference.part, crate::LoopPart::Hole(_)) {
                Side::Inside
            } else {
                Side::Outside
            },
        };
        editor.draft.source = ProfileGeometrySource::Contours(vec![contour]);
        editor.draft.side_overrides.clear();
        editor.error = None;
        true
    }

    pub(super) fn profile_geometry_collection(&self) -> Element<'_, Message> {
        let entries = self.profile_entries();
        let selected = self
            .geometry_row_selection
            .filter(|i| *i < entries.len())
            .or_else(|| (!entries.is_empty()).then_some(0));
        let rows = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let source_empty = match &entry.source {
                    ProfileGeometrySource::Cad(refs) => refs.is_empty(),
                    ProfileGeometrySource::Contours(c) => c.is_empty(),
                };
                GeometryRow {
                    icon: kind_icon(entry.kind),
                    label: if source_empty {
                        format!("{} · Select geometry", entry.name)
                    } else {
                        entry.name.clone()
                    },
                    action: self
                        .profile_entry_editor
                        .as_ref()
                        .is_none_or(|e| e.index == index)
                        .then_some(Message::GeometryRowSelected(index)),
                }
            })
            .collect();
        let editing = self.profile_entry_editor.is_some();
        let mut toolbar = row![].spacing(4).align_y(Alignment::Center);
        for kind in [
            ProfileGeometryKind::FaceContours,
            ProfileGeometryKind::Chain,
            ProfileGeometryKind::Silhouette,
        ] {
            let available = !editing
                && (kind == ProfileGeometryKind::Chain
                    || !self.controller.geometry_state().instances.is_empty());
            let description = match kind {
                ProfileGeometryKind::FaceContours => "Select a planar face to include its outer boundary and holes. Optionally include coplanar faces.",
                ProfileGeometryKind::Chain => "Select an open or closed contour and configure its direction and cutter side.",
                ProfileGeometryKind::Silhouette => "Select bodies to use their outlines and through openings viewed along the setup Z axis.",
            };
            toolbar = toolbar.push(hint(
                button(icon(kind_icon(kind), 28.))
                    .padding(4)
                    .style(inspector_secondary)
                    .on_press_maybe(available.then_some(Message::ProfileEntry(EntryMessage::Add(kind)))),
                format!("Add {}. {description}", kind_name(kind)), self.tooltips));
        }
        toolbar = toolbar.push(hint(
            button(icon("remove", 28.))
                .padding(4)
                .style(inspector_secondary)
                .on_press_maybe(
                    (!editing && selected.is_some())
                        .then_some(Message::ProfileEntry(EntryMessage::RemoveSelected)),
                ),
            "Delete the selected geometry entry.", self.tooltips));
        toolbar = toolbar.push(hint(
            button(icon("settings", 28.))
                .padding(4)
                .style(inspector_secondary)
                .on_press_maybe(
                    (!editing && selected.is_some())
                        .then_some(Message::ProfileEntry(EntryMessage::EditSelected)),
                ),
            "Configure the selected geometry entry.", self.tooltips));
        geometry_collection(
            "Contour selection",
            rows,
            selected,
            Some(Message::ProfileEntry(if selected.is_some() {
                EntryMessage::EditSelected
            } else {
                EntryMessage::Add(ProfileGeometryKind::Chain)
            })),
            toolbar,
        )
    }

    pub(super) fn profile_entry_settings<'a>(
        &'a self,
        editor: &'a EntryEditor,
    ) -> Element<'a, Message> {
        let entry = &editor.draft;
        let mut body = column![
            row![
                icon(kind_icon(entry.kind), 24.),
                text(format!("{} settings", kind_name(entry.kind))).size(15)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                text("Name").size(12).width(90),
                text_input("Entry name", &entry.name)
                    .size(12)
                    .padding([7, 8])
                    .on_input(|v| Message::ProfileEntry(EntryMessage::Name(v)))
                    .width(Length::Fill)
            ]
            .spacing(6),
            row![
                text(if entry.kind == ProfileGeometryKind::Silhouette {
                    "Bodies"
                } else {
                    "Selection"
                })
                .size(12)
                .width(90),
                inspector_button(if editor.picking {
                    "Selecting…"
                } else {
                    "Select"
                })
                .on_press(Message::ProfileEntry(EntryMessage::Pick)),
                inspector_button("Clear")
                    .on_press(Message::ProfileEntry(EntryMessage::ClearSource))
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        ]
        .spacing(9);
        let count = match &entry.source {
            ProfileGeometrySource::Cad(refs) => refs.len(),
            ProfileGeometrySource::Contours(contours) => contours.len(),
        };
        body = body.push(
            text(format!("{count} source feature(s) selected"))
                .size(11)
                .color(INSPECTOR_MUTED),
        );
        if entry.kind == ProfileGeometryKind::FaceContours {
            body = body.push(hint(
                checkbox(entry.coplanar)
                    .label("Select coplanar faces")
                    .size(15)
                    .text_size(12)
                    .on_toggle(|v| Message::ProfileEntry(EntryMessage::Coplanar(v))),
                "Include upward planar faces on the same plane in each selected body.", self.tooltips));
        }
        if entry.kind == ProfileGeometryKind::Chain {
            let mut modes = row![text("Mode").size(12).width(90)]
                .spacing(6)
                .align_y(Alignment::Center);
            for (mode, name, label) in [
                (ProfileChainMode::Closed, "closed", "Closed chain"),
                (ProfileChainMode::Open, "open", "Open chain: follow planar edges at the operation's Top and Bottom heights, including edges beside rounded surfaces. The cut extends the selected curve vertically."),
            ] {
                let active = entry.chain_mode == mode
                    || (entry.chain_mode == ProfileChainMode::Auto
                        && mode == ProfileChainMode::Closed);
                modes = modes.push(hint(
                    button(icon(name, 27.))
                        .padding(4)
                        .style(move |theme, status| inspector_tab(theme, status, active))
                        .on_press(Message::ProfileEntry(EntryMessage::ChainMode(mode))),
                    label, self.tooltips));
            }
            body=body.push(modes).push(hint(checkbox(entry.reversed).label("Reverse direction").size(15).text_size(12).on_toggle(|v|Message::ProfileEntry(EntryMessage::Reverse(v))),"Reverse this chain's selected direction. Open-chain left and right follow that direction.", self.tooltips));
        } else {
            if entry.kind == ProfileGeometryKind::Silhouette {
                body=body.push(hint(checkbox(entry.use_setup_models).label("Use setup models").size(15).text_size(12).on_toggle(|v|Message::ProfileEntry(EntryMessage::SetupModels(v))),"Use the bodies currently included in the active setup. Select bodies manually to customize the collection.", self.tooltips));
                body = body.push(
                    row![
                        text("Tolerance (mm)").size(12).width(90),
                        text_input("0.010", &editor.tolerance)
                            .size(12)
                            .padding([7, 8])
                            .on_input(|v| Message::ProfileEntry(EntryMessage::Tolerance(v)))
                            .width(Length::Fill)
                    ]
                    .spacing(6),
                );
            }
            let mut filters = row![text("Curves").size(12).width(90)]
                .spacing(6)
                .align_y(Alignment::Center);
            for (value, name, label) in [
                (ProfileCurves::All, "all", "All contours"),
                (ProfileCurves::Outer, "outer", "Outer contours only"),
                (ProfileCurves::Inner, "inner", "Inner contours only"),
            ] {
                let active = entry.curves == value;
                filters = filters.push(hint(
                    button(icon(name, 27.))
                        .padding(4)
                        .style(move |theme, status| inspector_tab(theme, status, active))
                        .on_press(Message::ProfileEntry(EntryMessage::Curves(value))),
                    label, self.tooltips));
            }
            body = body.push(filters);
        }
        const SIDES: [ProfileSideRule; 5] = [
            ProfileSideRule::StartOutside,
            ProfileSideRule::StartInside,
            ProfileSideRule::AlwaysOutside,
            ProfileSideRule::AlwaysInside,
            ProfileSideRule::On,
        ];
        body=body.push(profile_picker("Side",illustrated_help::PROFILE_ENTRY_SIDES,self.tooltips,entry.side_rule,&SIDES,|v|Message::ProfileEntry(EntryMessage::Side(v))));
        if let Some(error) = &editor.error {
            body = body.push(text(error).size(11).color(palette::WARN));
        }
        let footer = inspector_footer(
            row![
                inspector_button("Cancel").on_press(Message::ProfileEntry(EntryMessage::Cancel)),
                Space::new().width(Length::Fill),
                inspector_button("OK")
                    .style(inspector_primary)
                    .on_press(Message::ProfileEntry(EntryMessage::Confirm))
            ]
            .spacing(8),
        );
        column![
            scrollable(body.padding(14))
                .width(Length::Fill)
                .height(Length::Fill),
            footer
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

impl App {
    fn sync_profile_entry_cache(&mut self) {
        if let Some((resolved, _)) = self.profile_selection_geometry() {
            self.edit_operation_draft(|op| {
                if let Operation::Profile(p) = op {
                    p.chain = resolved.chain;
                    p.closed = resolved.closed;
                    p.side = resolved.side;
                    p.additional_chains = resolved.additional_chains;
                    p.selections = resolved.selections;
                }
            });
        }
    }
    pub(super) fn set_profile_entry_contour_side(&mut self, index: usize, side: Side) {
        let Ok(geometry) = crate::cad_operations::geometry_for_setup(self.controller.document())
        else {
            return;
        };
        let mut offset = 0;
        for (entry_index, entry) in self.preview_profile_entries().iter().enumerate() {
            let Ok((contours, _)) = cam_geometry3d::profile_entry_contours(&geometry, entry) else {
                continue;
            };
            if index < offset + contours.len() {
                let local = index - offset;
                if let Some(editor) = &mut self.profile_entry_editor {
                    if editor.index != entry_index {
                        return;
                    }
                    editor.draft.side_overrides.insert(local, side);
                } else {
                    self.edit_operation_draft(|op| {
                        if let Operation::Profile(p) = op {
                            if let Some(entry) = p
                                .selections
                                .as_mut()
                                .and_then(|entries| entries.get_mut(entry_index))
                            {
                                entry.side_overrides.insert(local, side);
                            }
                        }
                    });
                }
                self.profile_contour_selection = index;
                self.geometry_row_selection = Some(entry_index);
                self.status = format!("{}: contour {} uses {side}.", entry.name, local + 1);
                if self.profile_entry_editor.is_none() {
                    self.sync_profile_entry_cache();
                }
                return;
            }
            offset += contours.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.controller.select(Selection::Setup);
        app.operation_draft = None;
        app.setup_draft = None;
        app.refresh_fields();
        let _ = app.update(Message::BeginOp(OpKind::Profile));
        app.operation_tab = OperationTab::Geometry;
        app
    }
    fn pick(app: &mut App, reference: LoopRef) {
        let (points, _) = app.controller.loop_points(reference).unwrap();
        let p = points[0];
        assert!(app.profile_entry_drawing_pick([p.x as f32, p.y as f32], 0.5));
    }
    #[test]
    fn icon_adds_a_named_entry_settings_cancel_rolls_back_and_delete_uses_selected_row() {
        let mut app = app();
        let before = app.controller.document().clone();
        assert!(app.profile_entries().is_empty());
        app.profile_entry_message(EntryMessage::Add(ProfileGeometryKind::Chain));
        assert_eq!(app.profile_entries().len(), 1);
        assert!(app.profile_entry_editor.is_some());
        pick(&mut app, LoopRef::outer(0));
        app.profile_entry_message(EntryMessage::Name("Outside edge".into()));
        app.profile_entry_message(EntryMessage::Side(ProfileSideRule::On));
        app.profile_entry_message(EntryMessage::Confirm);
        assert!(app.profile_entry_editor.is_none());
        assert_eq!(app.profile_entries()[0].name, "Outside edge");
        app.profile_entry_message(EntryMessage::Add(ProfileGeometryKind::Chain));
        assert_eq!(app.profile_entries().len(), 2);
        app.profile_entry_message(EntryMessage::Cancel);
        assert_eq!(app.profile_entries().len(), 1);
        app.profile_entry_message(EntryMessage::EditSelected);
        assert!(app.profile_entry_editor.is_some());
        app.profile_entry_message(EntryMessage::Name("Discard this".into()));
        app.profile_entry_message(EntryMessage::Reverse(true));
        app.profile_entry_message(EntryMessage::Cancel);
        assert_eq!(app.profile_entries()[0].name, "Outside edge");
        assert!(!app.profile_entries()[0].reversed);
        app.geometry_row_selection = Some(0);
        app.profile_entry_message(EntryMessage::RemoveSelected);
        assert!(app.profile_entries().is_empty());
        assert!(app.creation_operation().is_err());
        assert_eq!(app.controller.document(), &before);
    }
    #[test]
    fn entry_submenu_side_and_direction_drive_preview_and_survive_save_reopen() {
        let mut app = app();
        app.profile_entry_message(EntryMessage::Add(ProfileGeometryKind::Chain));
        pick(&mut app, LoopRef::outer(0));
        app.profile_entry_message(EntryMessage::Name("Open reversed perimeter".into()));
        app.profile_entry_message(EntryMessage::ChainMode(ProfileChainMode::Open));
        app.profile_entry_message(EntryMessage::Reverse(true));
        app.profile_entry_message(EntryMessage::Side(ProfileSideRule::AlwaysInside));
        app.profile_entry_message(EntryMessage::Confirm);
        let (Operation::Profile(profile), binding) = app.creation_operation().unwrap() else {
            panic!("profile")
        };
        assert!(binding.is_none());
        assert!(!profile.closed);
        assert_eq!(profile.side, Side::Inside);
        assert_eq!(
            profile.chain.points().first(),
            app.controller.regions()[0].outer().points().last()
        );
        let _ = app.update(Message::ConfirmOp);
        assert!(!app.creating_operation(), "{}", app.status);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("operation")
        };
        let path = std::env::temp_dir().join(format!("named-profile-{}.ocam", std::process::id()));
        app.controller.save_project(&path).unwrap();
        let mut reopened = AppController::new(crate::default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(reopened.operation(id), app.controller.operation(id));
        let Some(Operation::Profile(profile)) = reopened.operation(id) else {
            panic!("profile")
        };
        assert_eq!(
            profile.selections.as_ref().unwrap()[0].name,
            "Open reversed perimeter"
        );
    }
    #[test]
    fn all_selection_icons_are_valid_svgs() {
        for svg in [
            include_str!("../assets/icons/profile-selection/face.svg"),
            include_str!("../assets/icons/profile-selection/chain.svg"),
            include_str!("../assets/icons/profile-selection/silhouette.svg"),
            include_str!("../assets/icons/profile-selection/settings.svg"),
            include_str!("../assets/icons/profile-selection/closed.svg"),
            include_str!("../assets/icons/profile-selection/open.svg"),
            include_str!("../assets/icons/profile-selection/all.svg"),
            include_str!("../assets/icons/profile-selection/outer.svg"),
            include_str!("../assets/icons/profile-selection/inner.svg"),
        ] {
            let parsed = roxmltree::Document::parse(svg).unwrap();
            assert_eq!(parsed.root_element().tag_name().name(), "svg");
        }
    }
}
