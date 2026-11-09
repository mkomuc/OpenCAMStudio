use super::*;
use cam_model::RotaryWrap;

pub(super) const INDEX_FACE_COLOR: cam_render::Color = [1.0, 0.48, 0.12, 1.0];

/// Convert a point in the indexed machine frame back to the displayed model frame.
pub(super) fn index_point_to_world(
    frame: &cam_model::SetupFrame,
    index: Option<&cam_model::RotaryIndex>,
    point: [f64; 3],
) -> [f64; 3] {
    let point = if let Some(index) = index {
        let center = cam_cldata::Point3::new(index.center[0], index.center[1], index.center[2]);
        let p = index.axis.material_point(
            cam_cldata::Point3::new(point[0], point[1], point[2]), center, index.angle,
        );
        [p.x, p.y, p.z]
    } else {
        point
    };
    frame.to_world_vector(point)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Number {
    Radius,
    Offset,
    CenterX,
    CenterY,
    CenterZ,
    SurfaceZ,
    Seam,
    Tolerance,
}

#[cfg(test)]
mod index_tests {
    use super::*;
    #[test]
    fn rotary_picks_override_profile_contour_filter() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.cad_pending_kind = Some(OpKind::Profile);
        app.operation_draft = Some(app.controller.operation_template(OpKind::Profile, 1));
        app.rotary_action(Action::Indexed(true));
        app.rotary_action(Action::IndexMode(IndexMode::Face));
        for (pick, expected) in [
            (Pick::IndexFace, EntityKind::Face),
            (Pick::Cylinder, EntityKind::Face),
            (Pick::Faces, EntityKind::Face),
            (Pick::Edges, EntityKind::Edge),
            (Pick::Loops, EntityKind::Loop),
        ] {
            app.rotary_action(Action::Pick(pick));
            let viewport = app.machining_viewport();
            assert_eq!(viewport.cad_filter, expected);
            assert!(viewport.cad_filter_explicit);
            assert!(!viewport.cad_silhouette);
        }
    }
    #[test]
    fn indexed_mode_is_staged_and_committed_with_operation_fields_in_one_undo() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        let op = app.controller.operation_template(OpKind::Drill, 1);
        app.controller.add_operation(op);
        let id = app.controller.inspection_setup().operations[0].id();
        app.begin_operation_draft(id);
        app.rotary_action(Action::Indexed(true));
        app.rotary_action(Action::IndexValue(0, "90".into()));
        assert!(app.controller.document().rotary_indexes.is_empty());
        assert!(app.operation_draft_dirty());
        app.apply_inspector();
        assert_eq!(
            app.controller
                .document()
                .rotary_indexes
                .get(&id)
                .map(|i| i.angle),
            Some(90.)
        );
        app.controller.undo();
        assert!(app.controller.document().rotary_indexes.is_empty());
    }
    #[test]
    fn face_angles_align_normals_and_flip_to_the_other_side() {
        for (axis, normal) in [
            (cam_model::RotaryAxis::A, [0., 0.6, 0.8]),
            (cam_model::RotaryAxis::B, [0.6, 0., 0.8]),
        ] {
            for flip in [false, true] {
                let angle = index_angle_for_normal(normal, axis, flip).unwrap();
                let n = axis.material_point(
                    cam_cldata::Point3::new(normal[0], normal[1], normal[2]),
                    cam_cldata::Point3::new(0., 0., 0.),
                    -angle,
                );
                assert!(n.x.abs() < 1e-9 && n.y.abs() < 1e-9);
                assert!((n.z - if flip { -1. } else { 1. }).abs() < 1e-9);
            }
        }
        assert!(index_angle_for_normal([0.5, 0., 0.866], cam_model::RotaryAxis::A, false).is_err());
    }
    #[test]
    fn invalid_index_input_cannot_commit_a_previous_valid_number() {
        let (mut app, _) = App::new();
        app.rotary_action(Action::Indexed(true));
        app.rotary_action(Action::IndexValue(0, "unfinished".into()));
        assert!(app.validate_rotary_index_draft().is_err());
    }
}
impl Number {
    pub(super) const ALL: [Self; 8] = [
        Self::Radius,
        Self::Offset,
        Self::CenterX,
        Self::CenterY,
        Self::CenterZ,
        Self::SurfaceZ,
        Self::Seam,
        Self::Tolerance,
    ];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Radius => "Cylinder radius (mm)",
            Self::Offset => "Cylinder offset (mm)",
            Self::CenterX => "Axis centre X (mm)",
            Self::CenterY => "Axis centre Y (mm)",
            Self::CenterZ => "Axis centre Z (mm)",
            Self::SurfaceZ => "Unwrapped surface Z (mm)",
            Self::Seam => "Seam angle (degrees)",
            Self::Tolerance => "Wrap tolerance (mm)",
        }
    }
    pub(super) fn get(self, w: &RotaryWrap) -> f64 {
        match self {
            Self::Radius => w.radius,
            Self::Offset => w.offset,
            Self::CenterX => w.center[0],
            Self::CenterY => w.center[1],
            Self::CenterZ => w.center[2],
            Self::SurfaceZ => w.surface_z,
            Self::Seam => w.seam_angle,
            Self::Tolerance => w.tolerance,
        }
    }
    pub(super) fn set(self, w: &mut RotaryWrap, v: f64) {
        match self {
            Self::Radius => w.radius = v,
            Self::Offset => w.offset = v,
            Self::CenterX => w.center[0] = v,
            Self::CenterY => w.center[1] = v,
            Self::CenterZ => w.center[2] = v,
            Self::SurfaceZ => w.surface_z = v,
            Self::Seam => w.seam_angle = v,
            Self::Tolerance => w.tolerance = v,
        }
    }
    pub(super) fn invalid(self, v: f64) -> bool {
        !v.is_finite()
            || match self {
                Self::Radius => v <= 0.,
                Self::Tolerance => v <= 0. || v > 1.,
                _ => false,
            }
    }
    pub(super) fn cylinder_dimension(self) -> bool {
        matches!(
            self,
            Self::Radius | Self::CenterX | Self::CenterY | Self::CenterZ
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    Cylinder,
    Loops,
    Edges,
    Faces,
    IndexFace,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndexMode {
    Manual,
    Face,
}
impl std::fmt::Display for IndexMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Manual => "Manual angle",
            Self::Face => "Select face",
        })
    }
}
#[derive(Clone, Debug)]
pub(super) enum Action {
    Indexed(bool),
    IndexAxis(cam_model::RotaryAxis),
    IndexMode(IndexMode),
    IndexFlip(bool),
    IndexValue(usize, String),
    Enable(bool),
    Unwrap(bool),
    ClearCylinder,
    Pick(Pick),
    Done,
    Clear,
}
fn index_angle_for_normal(
    n: [f64; 3],
    axis: cam_model::RotaryAxis,
    flip: bool,
) -> Result<f64, String> {
    let length = n.iter().map(|x| x * x).sum::<f64>().sqrt();
    if !length.is_finite() || length < 1e-10 {
        return Err("Face normal is invalid".into());
    }
    let (along, angle) = match axis {
        cam_model::RotaryAxis::A => (n[0], n[1].atan2(n[2]).to_degrees()),
        cam_model::RotaryAxis::B => (n[1], (-n[0]).atan2(n[2]).to_degrees()),
    };
    if along.abs() / length > 1e-6 {
        return Err("This face cannot be aligned with Z using the selected rotary axis".into());
    }
    Ok(if flip { angle + 180. } else { angle })
}
impl App {
    /// Draw the orientation reference independently of machining selections.
    pub(super) fn index_face_overlay(&self, cad_scene: &cam_render::CadScene) -> Scene {
        let mut overlay = Scene::new();
        let Some(index) = &self.rotary_index_draft else { return overlay };
        let Some(reference) = &index.face else { return overlay };
        let geometry = self.controller.geometry_state();
        let Some(instance) = geometry.instance(reference.instance_id).filter(|i| i.visible) else {
            return overlay;
        };
        let Some(asset) = geometry.asset(reference.asset_id) else { return overlay };
        if !geometry.entity_exists(reference) {
            return overlay;
        }
        let Some(face) = asset.topology.faces.iter().find(|f| f.id == reference.id) else {
            return overlay;
        };
        let cam_model::CadSurface::Plane { origin, normal } = face.surface else {
            return overlay;
        };
        // CadScene omits hidden instances. Retain only the face's highlighted
        // edges so the overlay does not redraw the rest of the model.
        let outline = cad_scene.edges(std::slice::from_ref(reference), None);
        for strip in outline.strips.into_iter().filter(|s| s.width > 1.0) {
            overlay.add_highlight(strip.points, INDEX_FACE_COLOR);
        }
        if overlay.strips.is_empty() {
            return overlay;
        }
        let norm = normal.iter().map(|x| x * x).sum::<f64>().sqrt();
        if !norm.is_finite() || norm < 1e-10 {
            return overlay;
        }
        let n = normal.map(|v| v / norm);
        let center = std::array::from_fn::<_, 3, _>(|i| {
            (face.bounds[0][i] + face.bounds[1][i]) * 0.5
        });
        let distance: f64 = (0..3).map(|i| (center[i] - origin[i]) * n[i]).sum();
        let center = std::array::from_fn(|i| center[i] - distance * n[i]);
        let tip = instance.placement.transform_point(center);
        let direction = instance.placement.transform_vector(n).map(|v| {
            if index.flip { -v } else { v }
        });
        let extent = (0..3)
            .map(|i| face.bounds[1][i] - face.bounds[0][i])
            .fold(0.0_f64, f64::max);
        let length = (extent * 0.35).clamp(1.0, 15.0);
        let tangent_seed = if direction[0].abs() < 0.8 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        };
        let cross = [
            direction[1] * tangent_seed[2] - direction[2] * tangent_seed[1],
            direction[2] * tangent_seed[0] - direction[0] * tangent_seed[2],
            direction[0] * tangent_seed[1] - direction[1] * tangent_seed[0],
        ];
        let cross_length = cross.iter().map(|v| v * v).sum::<f64>().sqrt();
        let wing = cross.map(|v| v / cross_length);
        let tail = std::array::from_fn::<_, 3, _>(|i| tip[i] + direction[i] * length);
        let head = std::array::from_fn::<_, 3, _>(|i| tip[i] + direction[i] * length * 0.25);
        let left = std::array::from_fn::<_, 3, _>(|i| head[i] + wing[i] * length * 0.13);
        let right = std::array::from_fn::<_, 3, _>(|i| head[i] - wing[i] * length * 0.13);
        let point = |p: [f64; 3]| p.map(|v| v as f32);
        overlay.add_highlight(vec![point(tail), point(tip)], INDEX_FACE_COLOR);
        overlay.add_highlight(vec![point(left), point(tip), point(right)], INDEX_FACE_COLOR);
        overlay
    }

    pub(super) fn rotary_draft_document(&self) -> cam_model::Document {
        let mut document = self.controller.document().clone();
        if let Some(op) = &self.operation_draft {
            if let Some(index) = &self.rotary_index_draft {
                document.rotary_indexes.insert(op.id(), index.clone());
            } else {
                document.rotary_indexes.remove(&op.id());
            }
        }
        document
    }
    pub(super) fn indexed_draft_geometry(&self) -> Result<cam_model::GeometryState, String> {
        crate::cad_operations::geometry_for_operation(
            &self.rotary_draft_document(),
            self.operation_draft.as_ref().map_or(0, Operation::id),
        )
    }
    pub(super) fn seed_rotary_index_values(&mut self) {
        let index = self.rotary_index_draft.clone().unwrap_or_default();
        self.rotary_index_values = [
            fmt_num(index.angle),
            fmt_num(index.center[0]),
            fmt_num(index.center[1]),
            fmt_num(index.center[2]),
        ];
    }
    pub(super) fn validate_rotary_index_draft(&self) -> Result<(), String> {
        if let Some(index) = &self.rotary_index_draft {
            if self.rotary_index_mode == IndexMode::Face && index.face.is_none() {
                return Err("Select a planar index face".into());
            }
            if self
                .rotary_index_values
                .iter()
                .any(|v| v.parse::<f64>().is_err())
            {
                return Err("Enter a finite index angle and axis centre".into());
            }
            index.validate()?;
        }
        Ok(())
    }
    pub(super) fn rotary_controls(&self, op: &Operation) -> Element<'_, Message> {
        use iced::widget::column;
        let enabled = op.rotary_wrap().is_some();
        let indexed = self.rotary_index_draft.is_some();
        let mut modes = row![
            inspector_button(if enabled || indexed {
                "3-axis"
            } else {
                "3-axis ✓"
            })
            .on_press(Message::Rotary(Action::Enable(false))),
            inspector_button(if indexed {
                "3+1 Indexed ✓"
            } else {
                "3+1 Indexed"
            })
            .on_press(Message::Rotary(Action::Indexed(true)))
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if op.supports_rotary_wrap() {
            modes = modes.push(
                inspector_button(if enabled { "Wrap ✓" } else { "Wrap" })
                    .on_press(Message::Rotary(Action::Enable(true))),
            );
        }
        let mut form = column![text("Machining Type").size(12), modes].spacing(12);
        if let Some(index) = &self.rotary_index_draft {
            form = form.push(
                row![
                    text("Rotary axis").size(12),
                    inspector_button(if index.axis == cam_model::RotaryAxis::A {
                        "A (X) ✓"
                    } else {
                        "A (X)"
                    })
                    .on_press(Message::Rotary(Action::IndexAxis(cam_model::RotaryAxis::A))),
                    inspector_button(if index.axis == cam_model::RotaryAxis::B {
                        "B (Y) ✓"
                    } else {
                        "B (Y)"
                    })
                    .on_press(Message::Rotary(Action::IndexAxis(cam_model::RotaryAxis::B)))
                ]
                .spacing(8),
            );
            form = form.push(
                row![
                    text("Angle mode").size(12),
                    pick_list(
                        [IndexMode::Manual, IndexMode::Face],
                        Some(self.rotary_index_mode),
                        |v| Message::Rotary(Action::IndexMode(v))
                    )
                    .text_size(12)
                    .padding([7, 9])
                    .style(inspector_picker)
                ]
                .spacing(8),
            );
            if self.rotary_index_mode == IndexMode::Face {
                form = form.push(
                    row![
                        inspector_button(if index.face.is_some() {
                            "Change face…"
                        } else {
                            "Select face…"
                        })
                        .on_press(Message::Rotary(Action::Pick(Pick::IndexFace))),
                        text(format!("Angle: {}°", fmt_num(index.angle))).size(12),
                        text("Flip tool engagement").size(12),
                        checkbox(index.flip).on_toggle(|v| Message::Rotary(Action::IndexFlip(v)))
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
                form = form.push(text("Flip adds 180° to the index angle, turning the selected face away from the tool. Select geometry accessible from the resulting direction.").size(12));
                form = form.push(text("Orange: orientation face and tool approach. Blue: cutting geometry.").size(12));
            } else {
                form = form.push(
                    row![
                        text("Index angle (degrees)").size(12),
                        iced::widget::text_input("0", &self.rotary_index_values[0])
                            .on_input(|v| Message::Rotary(Action::IndexValue(0, v)))
                    ]
                    .spacing(8),
                );
            }
            for (i, label) in [
                "Axis centre X (mm)",
                "Axis centre Y (mm)",
                "Axis centre Z (mm)",
            ]
            .into_iter()
            .enumerate()
            {
                let i = i + 1;
                form = form.push(
                    row![
                        text(label).size(12),
                        iced::widget::text_input("0", &self.rotary_index_values[i])
                            .on_input(move |v| Message::Rotary(Action::IndexValue(i, v)))
                    ]
                    .spacing(8),
                );
            }
            return form.push(text("Axis centre is a point on the rotary axis in setup coordinates. CAD geometry is machined at this fixed workpiece angle; drawing coordinates and heights use the indexed machine frame.").size(12)).into();
        }
        let Some(wrap) = op.rotary_wrap() else {
            return form
                .push(text("Tool Orientation").size(14))
                .push(text("Uses setup orientation").size(12))
                .into();
        };
        form = form.push(text("Wrap").size(14));
        form = form.push(
            row![
                text("Unwrap Geometry").size(12),
                checkbox(wrap.unwrap_geometry).on_toggle(|v| Message::Rotary(Action::Unwrap(v)))
            ]
            .spacing(8),
        );
        form = form.push(
            row![
                text("Wrap Cylinder").size(12),
                inspector_button(if wrap.cylinder.is_some() {
                    "Face"
                } else {
                    "Select face…"
                })
                .on_press(Message::Rotary(Action::Pick(Pick::Cylinder))),
                inspector_button("×").on_press(Message::Rotary(Action::ClearCylinder))
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        let field = Field::Wrap(Number::Offset);
        let value = self
            .fields
            .get(&field)
            .cloned()
            .unwrap_or_else(|| fmt_num(wrap.offset));
        form = form.push(field_row_labeled(
            field,
            "Offset (mm)",
            field.help(),
            &value,
            self.tooltips,
            self.field_invalid(field),
        ));
        form = form.push(
            row![
                text("Radius").size(12),
                text(format!(
                    "{} mm",
                    fmt_num(wrap.radius + value.parse::<f64>().unwrap_or(wrap.offset))
                ))
                .size(12)
            ]
            .spacing(12),
        );
        form.into()
    }
    pub(super) fn rotary_geometry_controls(&self, op: &Operation) -> Element<'_, Message> {
        use iced::widget::column;
        let count = op.rotary_wrap().map_or(0, |w| w.geometry.len());
        column![
            text(format!("{count} selected machining boundaries")).size(12),
            row![
                inspector_button("Loops").on_press(Message::Rotary(Action::Pick(Pick::Loops))),
                inspector_button("Edges").on_press(Message::Rotary(Action::Pick(Pick::Edges))),
                inspector_button("Faces").on_press(Message::Rotary(Action::Pick(Pick::Faces)))
            ]
            .spacing(4),
            row![
                inspector_button("Done selecting").on_press(Message::Rotary(Action::Done)),
                inspector_button("Use drawing geometry").on_press(Message::Rotary(Action::Clear))
            ]
            .spacing(4)
        ]
        .spacing(8)
        .into()
    }
    pub(super) fn rotary_action(&mut self, action: Action) {
        match action {
            Action::Indexed(enabled) => {
                if enabled {
                    let mut index = self.rotary_index_draft.clone().unwrap_or_default();
                    if let Some(wrap) = self
                        .operation_draft
                        .as_ref()
                        .and_then(Operation::rotary_wrap)
                    {
                        index.axis = wrap.axis;
                        index.center = wrap.center;
                    }
                    self.rotary_index_draft = Some(index);
                    self.edit_operation_draft(|op| op.set_rotary_wrap(None));
                    self.seed_rotary_index_values();
                } else {
                    self.rotary_index_draft = None;
                }
            }
            Action::IndexAxis(axis) => {
                if let Some(index) = &mut self.rotary_index_draft {
                    index.axis = axis;
                }
                self.recalculate_index_face();
            }
            Action::IndexMode(mode) => {
                self.rotary_index_mode = mode;
                self.rotary_pick = None;
                if mode == IndexMode::Manual {
                    if let Some(index) = &mut self.rotary_index_draft {
                        index.face = None;
                    }
                }
            }
            Action::IndexFlip(flip) => {
                if let Some(index) = &mut self.rotary_index_draft {
                    index.flip = flip;
                }
                self.recalculate_index_face();
            }
            Action::IndexValue(i, value) => {
                if i < 4 {
                    if let (Some(index), Ok(v)) =
                        (&mut self.rotary_index_draft, value.parse::<f64>())
                    {
                        if i == 0 {
                            index.angle = v;
                        } else {
                            index.center[i - 1] = v;
                        }
                    }
                    self.rotary_index_values[i] = value;
                }
            }
            Action::Enable(enabled) => {
                self.rotary_index_draft = None;
                self.rotary_index_mode = IndexMode::Manual;
                let current = self
                    .operation_draft
                    .as_ref()
                    .and_then(Operation::rotary_wrap)
                    .cloned();
                if current.is_some() == enabled {
                    return;
                }
                if !enabled {
                    self.rotary_wrap_draft = current.map(|mut wrap| {
                        if let Some(value) = self
                            .fields
                            .get(&Field::Wrap(Number::Offset))
                            .and_then(|s| s.parse().ok())
                        {
                            wrap.offset = value;
                        }
                        wrap
                    });
                }
                let restoring = self.rotary_wrap_draft.is_some();
                let mut wrap = if enabled {
                    self.rotary_wrap_draft.take().unwrap_or_default()
                } else {
                    RotaryWrap::default()
                };
                if let Some(op) = self.operation_draft.as_ref().filter(|_| !restoring) {
                    if let Some(binding) = self.controller.geometry_state().bindings.get(&op.id()) {
                        wrap.geometry = binding.selected.clone();
                    } else if self.cad_pending_kind.is_some() {
                        wrap.geometry = self.controller.cad_selection().to_vec();
                    }
                }
                self.edit_operation_draft(|op| {
                    op.set_rotary_wrap(enabled.then_some(wrap));
                    if enabled {
                        if let Operation::Profile(p) = op {
                            p.comp = cam_model::Comp::Computed;
                        }
                    }
                });
                self.rotary_pick = None;
                self.refresh_operation_fields();
            }
            Action::Unwrap(value) => self.edit_operation_draft(|op| {
                if let Some(mut w) = op.rotary_wrap().cloned() {
                    w.unwrap_geometry = value;
                    op.set_rotary_wrap(Some(w));
                }
            }),
            Action::ClearCylinder => self.edit_operation_draft(|op| {
                if let Some(mut w) = op.rotary_wrap().cloned() {
                    w.cylinder = None;
                    op.set_rotary_wrap(Some(w));
                }
            }),
            Action::Pick(pick) => {
                self.rotary_pick = Some(pick);
                self.cad_filter = match pick {
                    Pick::Cylinder | Pick::Faces | Pick::IndexFace => CadFilter::Face,
                    Pick::Edges => CadFilter::Edge,
                    Pick::Loops => CadFilter::Loop,
                };
                self.cad_filter_explicit = true;
                self.status=if pick==Pick::IndexFace {"Select a planar face to align with Z."} else if pick==Pick::Cylinder {"Select the exterior cylinder face."}else{"Select wrapped boundaries, then Done selecting. Click a selected boundary to remove it."}.into();
            }
            Action::Done => {
                self.rotary_pick = None;
                self.status = "Wrapped geometry selection finished.".into();
            }
            Action::Clear => {
                self.rotary_pick = None;
                self.edit_operation_draft(|op| {
                    if let Some(mut w) = op.rotary_wrap().cloned() {
                        w.geometry.clear();
                        op.set_rotary_wrap(Some(w));
                    }
                });
            }
        }
    }
    fn index_face_angle(&self, entity: &EntityRef) -> Result<f64, String> {
        let geometry = crate::cad_operations::geometry_for_setup(self.controller.document())?;
        if entity.kind != EntityKind::Face || !geometry.entity_exists(entity) {
            return Err("Select a valid planar face".into());
        }
        let instance = geometry
            .instance(entity.instance_id)
            .ok_or("Face instance is missing")?;
        let asset = geometry
            .asset(entity.asset_id)
            .ok_or("Face model is missing")?;
        let face = asset
            .topology
            .faces
            .iter()
            .find(|f| f.id == entity.id)
            .ok_or("Face is missing")?;
        let cam_model::CadSurface::Plane { normal, .. } = face.surface else {
            return Err("Select a planar face for indexing".into());
        };
        let n = instance.placement.transform_vector(normal);
        let index = self
            .rotary_index_draft
            .as_ref()
            .ok_or("Indexing is disabled")?;
        index_angle_for_normal(n, index.axis, index.flip)
    }

    fn recalculate_index_face(&mut self) {
        let Some(face) = self
            .rotary_index_draft
            .as_ref()
            .and_then(|i| i.face.clone())
        else {
            return;
        };
        match self.index_face_angle(&face) {
            Ok(angle) => {
                if let Some(index) = &mut self.rotary_index_draft {
                    index.angle = angle;
                }
                self.rotary_index_values[0] = fmt_num(angle);
            }
            Err(e) => {
                if let Some(index) = &mut self.rotary_index_draft {
                    index.face = None;
                }
                self.report_error(e);
            }
        }
    }
    pub(super) fn rotary_cad_pick(&mut self, entity: &EntityRef) -> bool {
        let Some(pick) = self.rotary_pick else {
            return false;
        };
        if pick == Pick::IndexFace {
            match self.index_face_angle(entity) {
                Ok(angle) => {
                    if let Some(index) = &mut self.rotary_index_draft {
                        index.face = Some(entity.clone());
                        index.angle = angle;
                    }
                    self.rotary_index_values[0] = fmt_num(angle);
                    self.rotary_pick = None;
                    self.status = "Index face selected.".into();
                }
                Err(e) => self.report_error(e),
            }
            return true;
        }
        let Some(mut wrap) = self
            .operation_draft
            .as_ref()
            .and_then(Operation::rotary_wrap)
            .cloned()
        else {
            self.rotary_pick = None;
            return false;
        };
        if pick == Pick::Cylinder {
            let result = crate::cad_operations::geometry_for_setup(self.controller.document())
                .and_then(|g| cam_geometry3d::wrap_cylinder(&g, entity, wrap));
            match result {
                Ok(w) => {
                    self.edit_operation_draft(|op| op.set_rotary_wrap(Some(w)));
                    self.rotary_pick = None;
                    self.fields
                        .retain(|f, _| !matches!(f, Field::Wrap(n) if n.cylinder_dimension()));
                    self.refresh_operation_fields();
                    self.status = "Wrap cylinder selected.".into();
                }
                Err(e) => self.report_error(e),
            }
        } else {
            if let Some(index) = wrap.geometry.iter().position(|r| r == entity) {
                wrap.geometry.remove(index);
            } else {
                wrap.geometry.push(entity.clone());
            }
            let count = wrap.geometry.len();
            self.edit_operation_draft(|op| op.set_rotary_wrap(Some(wrap)));
            self.status = format!(
                "{count} wrapped boundary selections. Select more or click Done selecting."
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrap_inventory_exposes_only_offset_as_an_editable_number() {
        let controller = AppController::new(crate::default_machine());
        let mut operation = controller.operation_template(OpKind::Pocket, 1);
        operation.set_rotary_wrap(Some(RotaryWrap::default()));
        let fields: Vec<_> = operation_fields(&operation)
            .into_iter()
            .filter(|f| matches!(f, Field::Wrap(_)))
            .collect();
        assert_eq!(fields, vec![Field::Wrap(Number::Offset)]);
    }
}
