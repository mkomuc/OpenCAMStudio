//! Manual holding-tab placement on the operation contour in every camera view.
use super::*;
use iced::widget::column;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Positioning {
    ByDistance,
    Manual,
}
impl Positioning {
    pub(super) const ALL: [Self; 2] = [Self::ByDistance, Self::Manual];
}
impl std::fmt::Display for Positioning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ByDistance => "By distance",
            Self::Manual => "Manual",
        })
    }
}

impl App {
    pub(super) fn manual_tab_controls(&self, op: &cam_model::ProfileOp) -> Element<'_, Message> {
        let mut list = column![row![
            text("Tab locations").size(12).width(Length::Fill),
            inspector_button(if self.profile_tab_picking {
                "Done"
            } else {
                "Select"
            })
            .on_press(Message::ProfileTabPickToggle),
            inspector_button("Clear").on_press_maybe(
                (!op.tabs.positions.is_empty()).then_some(Message::ProfileTabsClear)
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center)]
        .spacing(4);
        for (index, point) in op.tabs.positions.iter().enumerate() {
            list = list.push(
                row![
                    text(format!(
                        "{}   X {}   Y {}",
                        index + 1,
                        fmt_num(point[0]),
                        fmt_num(point[1])
                    ))
                    .size(12)
                    .width(Length::Fill),
                    inspector_button("×").on_press(Message::ProfileTabRemove(index)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        list.into()
    }

    pub(super) fn profile_tab_operation(&self) -> Option<cam_model::ProfileOp> {
        if self.cad_editing_op.is_some() || self.drawing_editing_op.is_some() {
            return None;
        }
        let operation = if self.creating_operation() {
            self.creation_operation().ok()?.0
        } else {
            self.operation_draft.clone()?
        };
        match operation {
            Operation::Profile(p) => Some(p),
            _ => None,
        }
    }

    pub(super) fn profile_tab_overlay(
        &self,
        camera: OrbitCamera,
        browser_width: f32,
    ) -> Option<TabOverlay> {
        if self.operation_tab != OperationTab::Geometry
            || !self.inspector_active
            || !self.inspector_enabled
        {
            return None;
        }
        let op = self.profile_tab_operation()?;
        if !op.tabs.enabled || !op.tabs.manual || !op.chain.is_valid() {
            return None;
        }
        let z = self
            .resolve_height_editor(if self.creating_operation() { 0 } else { op.id })
            .map(|h| h.top)
            .unwrap_or(self.controller.document().setup().heights.top_of_stock);
        Some(TabOverlay {
            camera,
            browser_width,
            points: op.chain.points().iter().map(|p| [p.x, p.y]).collect(),
            positions: op.tabs.positions,
            z,
            frame: self.controller.document().setup().frame.clone(),
            picking: self.profile_tab_picking,
            catch_px: self.settings.snap_catch_px().max(8.0),
            gizmo_size: self.show_gizmo.then_some(self.gizmo_size),
        })
    }
}

pub(super) struct TabOverlay {
    camera: OrbitCamera,
    browser_width: f32,
    points: Vec<[f64; 2]>,
    positions: Vec<[f64; 2]>,
    z: f64,
    frame: cam_model::SetupFrame,
    picking: bool,
    catch_px: f32,
    gizmo_size: Option<f32>,
}
impl TabOverlay {
    fn project(&self, point: [f64; 2], bounds: iced::Rectangle) -> Option<iced::Point> {
        let world = self.frame.to_world_vector([point[0], point[1], self.z]);
        picking_view::project(self.camera, bounds, world)
            .map(|p| p.0)
            .filter(|p| p.x.is_finite() && p.y.is_finite())
    }

    fn nearest(&self, cursor: iced::Point, bounds: iced::Rectangle) -> Option<[f64; 2]> {
        let mut best = (self.catch_px * self.catch_px, None);
        for (a, b) in self.points.iter().zip(self.points.iter().cycle().skip(1)) {
            let (Some(pa), Some(pb)) = (self.project(*a, bounds), self.project(*b, bounds)) else {
                continue;
            };
            let delta = pb - pa;
            let length2 = delta.x * delta.x + delta.y * delta.y;
            if length2 <= 1e-12 {
                continue;
            }
            let t = (((cursor.x - pa.x) * delta.x + (cursor.y - pa.y) * delta.y) / length2)
                .clamp(0.0, 1.0);
            let projected = pa + delta * t;
            let distance2 = (cursor.x - projected.x).powi(2) + (cursor.y - projected.y).powi(2);
            if distance2 <= best.0 {
                best = (
                    distance2,
                    Some([
                        a[0] + (b[0] - a[0]) * t as f64,
                        a[1] + (b[1] - a[1]) * t as f64,
                    ]),
                );
            }
        }
        best.1
    }

    fn cursor(&self, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Option<iced::Point> {
        let point = cursor.position_in(bounds)?;
        if point.x < self.browser_width {
            return None;
        }
        if let Some(size) = self.gizmo_size {
            let (x, y, size) = gizmo_rect(bounds.width, bounds.height, size, GIZMO_MARGIN);
            if point.x >= x && point.x <= x + size && point.y >= y && point.y <= y + size {
                return None;
            }
        }
        Some(point)
    }
}
impl canvas::Program<Message> for TabOverlay {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !self.picking {
            return None;
        }
        let cursor = self.cursor(bounds, cursor)?;
        if matches!(
            event,
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
        ) {
            let local = iced::Rectangle::with_size(bounds.size());
            for (index, point) in self.positions.iter().enumerate() {
                if self
                    .project(*point, local)
                    .is_some_and(|p| p.distance(cursor) <= 8.)
                {
                    return Some(
                        canvas::Action::publish(Message::ProfileTabRemove(index)).and_capture(),
                    );
                }
            }
            return Some(if let Some(point) = self.nearest(cursor, local) {
                canvas::Action::publish(Message::ProfileTabPicked(point)).and_capture()
            } else {
                canvas::Action::capture()
            });
        }
        if matches!(
            event,
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left
            ))
        ) {
            return Some(canvas::Action::capture());
        }
        None
    }
    fn mouse_interaction(
        &self,
        _: &(),
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if self.picking && self.cursor(bounds, cursor).is_some() {
            iced::mouse::Interaction::Crosshair
        } else {
            iced::mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let local = iced::Rectangle::with_size(bounds.size());
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let color = Color::from_rgb(1.0, 0.72, 0.18);
        let stroke = |width| {
            canvas::Stroke::default()
                .with_color(color)
                .with_width(width)
        };
        let mut outline = canvas::path::Builder::new();
        for (index, point) in self.points.iter().enumerate() {
            if let Some(point) = self.project(*point, local) {
                if index == 0 {
                    outline.move_to(point);
                } else {
                    outline.line_to(point);
                }
            }
        }
        outline.close();
        frame.stroke(&outline.build(), stroke(1.5));
        for (index, point) in self.positions.iter().enumerate() {
            if let Some(p) = self.project(*point, local) {
                frame.fill(
                    &canvas::Path::circle(p, 6.),
                    Color::from_rgb(0.12, 0.14, 0.17),
                );
                frame.stroke(&canvas::Path::circle(p, 6.), stroke(2.));
                frame.fill_text(canvas::Text {
                    content: (index + 1).to_string(),
                    position: p + iced::Vector::new(9., -16.),
                    color,
                    size: 12.0.into(),
                    ..Default::default()
                });
            }
        }
        if self.picking {
            if let Some(point) = self
                .cursor(bounds, cursor)
                .and_then(|p| self.nearest(p, local))
                .and_then(|p| self.project(p, local))
            {
                frame.fill(&canvas::Path::circle(point, 4.), color);
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;

    #[test]
    fn manual_tab_screen_picking_tracks_rotated_views_and_setup_frames() {
        let bounds = iced::Rectangle {
            x: 43.,
            y: 27.,
            width: 900.,
            height: 700.,
        };
        for (yaw, pitch) in [(0., 0.), (0.5, 0.7), (-0.4, 1.0)] {
            let mut camera = OrbitCamera::framed([-20., -20., -20.], [80., 80., 80.]);
            camera.orient = cam_render::orientation(yaw, pitch);
            let mut frame = cam_model::SetupFrame::default();
            frame.world_from_setup.translation = [10., 20., 5.];
            frame.world_from_setup.rotation = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
            let overlay = TabOverlay {
                camera,
                frame,
                browser_width: 0.,
                z: 3.,
                picking: true,
                catch_px: 12.,
                gizmo_size: None,
                points: vec![[0., 0.], [40., 0.], [40., 40.], [0., 40.]],
                positions: vec![],
            };
            let expected = [17., 0.];
            let screen = overlay.project(expected, bounds).unwrap();
            let local = screen - iced::Vector::new(bounds.x, bounds.y);
            let point = overlay
                .nearest(local, iced::Rectangle::with_size(bounds.size()))
                .unwrap();
            assert!((point[0] - expected[0]).abs() < 1e-4 && point[1].abs() < 1e-4);
            let action = overlay
                .update(
                    &mut (),
                    &canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )),
                    bounds,
                    iced::mouse::Cursor::Available(screen),
                )
                .unwrap();
            let (message, _, _) = action.into_inner();
            assert!(
                matches!(message, Some(Message::ProfileTabPicked(point)) if (point[0] - expected[0]).abs() < 1e-4)
            );
            let mut overlay = overlay;
            overlay.positions.push(expected);
            let action = overlay
                .update(
                    &mut (),
                    &canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )),
                    bounds,
                    iced::mouse::Cursor::Available(screen),
                )
                .unwrap();
            assert!(matches!(
                action.into_inner().0,
                Some(Message::ProfileTabRemove(0))
            ));
            assert!(overlay
                .nearest(
                    iced::Point::new(-100., -100.),
                    iced::Rectangle::with_size(bounds.size())
                )
                .is_none());
        }
    }

    #[test]
    fn manual_tabs_are_transactional_and_keep_locations_across_positioning_modes() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let before = app.controller.operation(id).unwrap().clone();
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::FieldChanged(Field::TabSpacing, "42".into()));
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        assert!(
            !operation_fields(app.operation_draft.as_ref().unwrap()).contains(&Field::TabSpacing)
        );
        let _ = app.update(Message::ProfileTabPickToggle);
        assert!(app.profile_tab_picking);
        let _ = app.update(Message::ProfileTabPicked([10., 0.]));
        let _ = app.update(Message::ProfileTabPicked([20., 0.]));
        let _ = app.update(Message::ProfileTabRemove(0));
        let _ = app.update(Message::CadEscape);
        assert!(!app.profile_tab_picking);
        assert!(app.operation_draft.is_some());
        let _ = app.update(Message::ProfileTabPositioning(Positioning::ByDistance));
        assert_eq!(app.fields[&Field::TabSpacing], "42");
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        assert_eq!(
            app.profile_tab_operation().unwrap().tabs.positions,
            vec![[20., 0.]]
        );
        assert_eq!(app.controller.operation(id), Some(&before));
        let _ = app.update(Message::Apply);
        assert!(
            matches!(app.controller.operation(id), Some(Operation::Profile(p)) if p.tabs.manual && p.tabs.positions == vec![[20., 0.]])
        );
        assert!(app.controller.undo());
        app.begin_operation_draft(id);
        assert_eq!(app.controller.operation(id), Some(&before));
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        let _ = app.update(Message::ProfileTabPickToggle);
        let _ = app.update(Message::ProfileTabPicked([10., 0.]));
        let _ = app.update(Message::ProfileTabsClear);
        assert!(app
            .profile_tab_operation()
            .unwrap()
            .tabs
            .positions
            .is_empty());
        let _ = app.update(Message::CancelOperationDraft);
        assert!(!app.profile_tab_picking);
        assert_eq!(app.controller.operation(id), Some(&before));
    }
}
