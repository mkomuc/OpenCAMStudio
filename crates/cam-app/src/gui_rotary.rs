use super::*;
use cam_model::{RotaryAxis, RotaryWrap};

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
}
#[derive(Clone, Debug)]
pub(super) enum Action {
    Enable(bool),
    Axis(RotaryAxis),
    Pick(Pick),
    Done,
    Clear,
}
impl std::fmt::Display for AxisChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            RotaryAxis::A => "A — around X",
            RotaryAxis::B => "B — around Y",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AxisChoice(RotaryAxis);
impl App {
    pub(super) fn rotary_controls(&self, op: &Operation) -> Element<'_, Message> {
        use iced::widget::column;
        if !op.supports_rotary_wrap() {
            return text("Wrap Toolpath is available for Profile and Pocket, including 2D adaptive clearing.").size(12).into();
        }
        let mut form = column![row![
            text("Wrap Toolpath (4th axis)").size(12),
            checkbox(op.rotary_wrap().is_some()).on_toggle(|v| Message::Rotary(Action::Enable(v)))
        ]
        .spacing(8)]
        .spacing(8);
        let Some(wrap) = op.rotary_wrap() else {
            return form.into();
        };
        form = form.push(pick_list(
            [AxisChoice(RotaryAxis::A), AxisChoice(RotaryAxis::B)],
            Some(AxisChoice(wrap.axis)),
            |v| Message::Rotary(Action::Axis(v.0)),
        ));
        form = form.push(
            inspector_button(if wrap.cylinder.is_some() {
                "Reselect Wrap Cylinder"
            } else {
                "Select Wrap Cylinder"
            })
            .on_press(Message::Rotary(Action::Pick(Pick::Cylinder))),
        );
        form=form.push(text("Use a cylindrical face, or enter radius and axis centre. A wraps drawing Y around X; B wraps drawing X around Y. Heights remain in the unwrapped plane.").size(12).color(INSPECTOR_MUTED));
        for number in Number::ALL {
            let field = Field::Wrap(number);
            let value = self
                .fields
                .get(&field)
                .cloned()
                .unwrap_or_else(|| fmt_num(number.get(wrap)));
            form = form.push(field_row_labeled(
                field,
                number.label(),
                field.help(),
                &value,
                self.tooltips,
                self.field_invalid(field),
            ));
        }
        form =
            form.push(text(format!("CAD machining boundaries: {}", wrap.geometry.len())).size(12));
        form = form.push(
            row![
                inspector_button("Loops").on_press(Message::Rotary(Action::Pick(Pick::Loops))),
                inspector_button("Edges").on_press(Message::Rotary(Action::Pick(Pick::Edges))),
                inspector_button("Faces").on_press(Message::Rotary(Action::Pick(Pick::Faces)))
            ]
            .spacing(4),
        );
        form = form.push(
            row![
                inspector_button("Done selecting").on_press(Message::Rotary(Action::Done)),
                inspector_button("Use drawing geometry").on_press(Message::Rotary(Action::Clear))
            ]
            .spacing(4),
        );
        if !self.controller.post_kind().supports_rotary_wrap() {
            form = form.push(
                text("For rotary G-code, select LinuxCNC, Fanuc or Haas in the post settings.")
                    .size(12)
                    .color(INSPECTOR_MUTED),
            );
        }
        form.into()
    }
    pub(super) fn rotary_action(&mut self, action: Action) {
        match action {
            Action::Enable(enabled) => {
                if enabled && self.rotary_heights.is_none() {
                    self.rotary_heights = self
                        .height_draft
                        .clone()
                        .map(|settings| (settings, self.height_offsets.clone()));
                }
                let mut wrap = RotaryWrap::default();
                if let Some(op) = &self.operation_draft {
                    if let Some(binding) = self.controller.geometry_state().bindings.get(&op.id()) {
                        wrap.geometry = binding.selected.clone();
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
                if enabled {
                    let settings = cam_model::OperationHeightSettings::default();
                    self.height_offsets = cam_model::HeightPlaneKind::ALL
                        .into_iter()
                        .map(|p| (p, fmt_num(settings.definition(p).offset)))
                        .collect();
                    self.height_draft = Some(settings);
                } else if let Some((settings, offsets)) = self.rotary_heights.take() {
                    self.height_draft = Some(settings);
                    self.height_offsets = offsets;
                }
                self.rotary_pick = None;
                self.refresh_operation_fields();
            }
            Action::Axis(axis) => {
                self.edit_operation_draft(|op| {
                    if let Some(mut w) = op.rotary_wrap().cloned() {
                        w.axis = axis;
                        w.cylinder = None;
                        op.set_rotary_wrap(Some(w));
                    }
                });
            }
            Action::Pick(pick) => {
                self.rotary_pick = Some(pick);
                self.cad_filter = match pick {
                    Pick::Cylinder | Pick::Faces => CadFilter::Face,
                    Pick::Edges => CadFilter::Edge,
                    Pick::Loops => CadFilter::Loop,
                };
                self.cad_filter_explicit = true;
                self.status=if pick==Pick::Cylinder {"Select the exterior cylinder face."}else{"Select wrapped boundaries, then Done selecting. Click a selected boundary to remove it."}.into();
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
    pub(super) fn rotary_cad_pick(&mut self, entity: &EntityRef) -> bool {
        let Some(pick) = self.rotary_pick else {
            return false;
        };
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
                    self.fields.retain(|f, _| !matches!(f, Field::Wrap(_)));
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
