//! Workpiece axes follow the setup draft and pending picks. A canvas overlay
//! keeps the datum and labels readable even when the origin is inside a solid.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct WorkpieceAxes {
    pub origin: [f64; 3],
    pub directions: [[f64; 3]; 3],
    pub provisional: bool,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
impl WorkpieceAxes {
    fn from_setup(setup: &cam_model::Setup, geometry: &cam_model::GeometryState) -> Self {
        let refs = &setup.frame.references;
        let mut axes = std::array::from_fn(|i| {
            setup
                .frame
                .to_world_vector(std::array::from_fn(|j| if i == j { 1. } else { 0. }))
        });
        let resolved = setup.frame.resolve_references(geometry, refs.clone());
        if let Ok(frame) = &resolved {
            axes = std::array::from_fn(|i| {
                frame.to_world_vector(std::array::from_fn(|j| if i == j { 1. } else { 0. }))
            });
        }
        Self {
            origin: setup.frame.to_world_vector(setup.origin),
            directions: axes,
            provisional: resolved.is_err(),
        }
    }
}
impl App {
    pub(super) fn workpiece_axes(&self, picking: &SetupPickGeometry) -> Option<WorkpieceAxes> {
        if self.model_view.is_some()
            || self.controller.selection() != Selection::Setup
            || self.cad_inspecting
            || self.controller.pending_op().is_some()
            || self.cad_pending_kind.is_some()
        {
            return None;
        }
        let mut setup = self
            .setup_draft
            .as_ref()
            .unwrap_or_else(|| self.controller.document().setup())
            .clone();
        if let Some(axis) = self.setup_axis_pick.filter(|i| *i < 3) {
            if let Some(direction) = self
                .setup_hover
                .edge
                .as_ref()
                .and_then(|edge| picking.direction(edge))
            {
                let refs = &mut setup.frame.references;
                let target = match axis {
                    0 => &mut refs.x,
                    1 => &mut refs.y,
                    _ => &mut refs.z,
                };
                let flipped = matches!(self.setup_hover.edge, Some(SetupFeature::Model(_)))
                    && target.as_ref().is_some_and(|r| r.flipped);
                *target = None;
                refs.stock_axes[axis] = Some(direction);
                refs.stock_flipped[axis] = flipped;
            }
        }
        let mut axes = WorkpieceAxes::from_setup(&setup, self.controller.geometry_state());
        if self.in_origin_pick() {
            if let Some(hit) = &self.setup_hover.snap {
                axes.origin = if let Some(first) = self.origin_first {
                    let second = setup.frame.to_setup_vector(hit.point);
                    setup
                        .frame
                        .to_world_vector([first[0], second[1], (first[2] + second[2]) / 2.])
                } else {
                    hit.point
                };
            }
        }
        Some(axes)
    }
}

pub(super) struct WorkpieceOverlay {
    pub axes: WorkpieceAxes,
    pub camera: OrbitCamera,
    pub scale: f32,
    pub interactive: bool,
    pub browser_overlay_width: f32,
}
impl WorkpieceOverlay {
    fn axis_at(&self, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Option<usize> {
        if !self.interactive
            || cursor
                .position()
                .is_some_and(|point| browser_contains(point, bounds, self.browser_overlay_width))
        {
            return None;
        }
        let point = cursor.position_in(bounds)?;
        let (origin, tips) = self.projected(bounds.size())?;
        tips.iter()
            .enumerate()
            .filter_map(|(axis, tip)| {
                let delta = *tip - origin;
                let length = delta.x.hypot(delta.y);
                let distance = if length < 7. {
                    point.distance(origin)
                } else {
                    let offset = point - origin;
                    let along = (offset.x * delta.x + offset.y * delta.y) / (length * length);
                    // Keep the shared datum clear so the visible arrows are unambiguous.
                    if along * length < 12. {
                        return None;
                    }
                    point.distance(origin + delta * along.clamp(0., 1.))
                };
                (distance <= 8.).then_some((axis, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(axis, _)| axis)
    }
    fn projected(&self, size: iced::Size) -> Option<(iced::Point, [iced::Point; 3])> {
        if size.width <= 0. || size.height <= 0. {
            return None;
        }
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, size);
        let origin = picking_view::project(self.camera, bounds, self.axes.origin)?.0;
        let length = 64. * self.scale * self.camera.world_per_pixel(size.height)
            / (size.width / size.height).min(1.);
        let tips = self.axes.directions.map(|axis| {
            let point = std::array::from_fn(|i| self.axes.origin[i] + axis[i] * length as f64);
            picking_view::project(self.camera, bounds, point).unwrap().0
        });
        Some((origin, tips))
    }
}
impl canvas::Program<Message> for WorkpieceOverlay {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if matches!(
            event,
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
        ) {
            let axis = self.axis_at(bounds, cursor)?;
            return Some(canvas::Action::publish(Message::SetupAxisToggle(axis)).and_capture());
        }
        None
    }
    fn mouse_interaction(
        &self,
        _: &(),
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if self.axis_at(bounds, cursor).is_some() {
            iced::mouse::Interaction::Pointer
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
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let Some((origin, tips)) = self.projected(bounds.size()) else {
            return vec![];
        };
        let colors = [
            Color::from_rgb(1., 0.32, 0.28),
            Color::from_rgb(0.3, 0.9, 0.4),
            Color::from_rgb(0.3, 0.63, 1.),
        ];
        let stroke = |color, width| {
            canvas::Stroke::default()
                .with_color(color)
                .with_width(width)
        };
        let label = |content: &str, position, color| canvas::Text {
            content: content.into(),
            position,
            color,
            size: 14.0.into(),
            ..Default::default()
        };
        for i in 0..3 {
            let end = tips[i];
            let delta = end - origin;
            let length = delta.x.hypot(delta.y);
            if length < 7. {
                // An axis aimed at the camera still has a readable symbol.
                frame.stroke(&canvas::Path::circle(origin, 6.), stroke(colors[i], 2.));
                let toward = dot(
                    self.axes.directions[i],
                    self.camera.pick_ray(0., 0., 1.).direction,
                ) < 0.;
                if toward {
                    frame.fill(&canvas::Path::circle(origin, 2.), colors[i]);
                } else {
                    frame.stroke(
                        &canvas::Path::line(
                            origin + iced::Vector::new(-3., -3.),
                            origin + iced::Vector::new(3., 3.),
                        ),
                        stroke(colors[i], 1.5),
                    );
                    frame.stroke(
                        &canvas::Path::line(
                            origin + iced::Vector::new(-3., 3.),
                            origin + iced::Vector::new(3., -3.),
                        ),
                        stroke(colors[i], 1.5),
                    );
                }
                frame.fill_text(label(
                    ["X", "Y", "Z"][i],
                    origin + iced::Vector::new(10., -19.),
                    colors[i],
                ));
                continue;
            }
            let direction = delta / length;
            let perpendicular = iced::Vector::new(-direction.y, direction.x);
            let line = canvas::Path::line(origin, end);
            // A dark outline preserves legibility over shaded stock and toolpaths.
            frame.stroke(&line, stroke(Color::from_rgba(0.05, 0.06, 0.07, 0.85), 5.));
            frame.stroke(&line, stroke(colors[i], 2.5));
            let arrow = canvas::Path::new(|p| {
                p.move_to(end);
                p.line_to(end - direction * 10. + perpendicular * 4.);
                p.line_to(end - direction * 10. - perpendicular * 4.);
                p.close();
            });
            frame.fill(&arrow, colors[i]);
            frame.fill_text(label(
                ["X", "Y", "Z"][i],
                end + direction * 9. + iced::Vector::new(-4., -9.),
                colors[i],
            ));
        }
        frame.stroke(&canvas::Path::circle(origin, 3.), stroke(Color::WHITE, 1.5));
        frame.fill_text(label(
            if self.axes.provisional {
                "WCS preview"
            } else {
                "WCS"
            },
            origin + iced::Vector::new(9., 12.),
            Color::WHITE,
        ));
        vec![frame.into_geometry()]
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
        app.begin_setup_draft();
        app
    }
    fn axes(app: &App) -> Option<WorkpieceAxes> {
        let setup = app
            .setup_draft
            .as_ref()
            .unwrap_or_else(|| app.controller.document().setup());
        let picking = SetupPickGeometry::new(&app.controller, setup, &app.cad_hidden);
        app.workpiece_axes(&picking)
    }
    fn close(a: [f64; 3], b: [f64; 3]) {
        assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-8), "{a:?} != {b:?}");
    }
    #[test]
    fn escape_aborts_wcs_edge_pick_and_preserves_setup_edits() {
        let mut app = app();
        let _ = app.update(Message::SetupNameChanged("Fixture draft".into()));
        let before = app.setup_draft.clone().unwrap();
        let committed = app.controller.document().clone();
        for axis in 0..3 {
            let _ = app.update(Message::SetupAxisPick(axis));
            assert_eq!(app.setup_axis_pick, Some(axis));
            app.setup_hover.edge = Some(SetupFeature::Stock(0));
            let _ = app.update(Message::CadEscape);
            assert!(app.setup_axis_pick.is_none());
            assert!(app.setup_hover.edge.is_none());
            assert_eq!(app.setup_draft.as_ref(), Some(&before));
            assert_eq!(app.controller.document(), &committed);
            assert!(!app.machining_viewport().setup_axis);
        }
    }
    #[test]
    fn selected_setup_always_shows_axes_and_numeric_origin_edits_preview_at_true_z() {
        let mut app = app();
        app.show_origin = false;
        assert!(axes(&app).is_some());
        let committed = app.controller.document().setup().clone();
        for (i, value) in ["12.5", "23.5", "7.5"].into_iter().enumerate() {
            let _ = app.update(Message::SetupOriginChanged(i, value.into()));
        }
        close(axes(&app).unwrap().origin, [12.5, 23.5, 7.5]);
        assert_eq!(app.controller.document().setup(), &committed);
        let _ = app.update(Message::SetupCancel);
        close(
            axes(&app).unwrap().origin,
            committed.frame.to_world_vector(committed.origin),
        );
        app.controller.select(Selection::Machine);
        assert!(axes(&app).is_none());
    }
    #[test]
    fn first_edge_then_second_edge_and_flips_preview_without_applying() {
        let mut app = app();
        let committed = app.controller.document().setup().clone();
        app.set_setup_stock_axis(0, [0., 1., 0.]);
        let first = axes(&app).unwrap();
        assert!(!first.provisional);
        close(first.directions[0], [0., 1., 0.]);
        close(
            first.origin,
            committed.frame.to_world_vector(committed.origin),
        );
        app.set_setup_stock_axis(1, [0., 0., 1.]);
        let second = axes(&app).unwrap();
        assert!(!second.provisional);
        assert_eq!(
            second.directions,
            [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]]
        );
        let _ = app.update(Message::SetupAxisFlip(0, true));
        let flipped = axes(&app).unwrap();
        assert_eq!(
            flipped.directions,
            [[0., -1., 0.], [0., 0., -1.], [1., 0., 0.]]
        );
        assert_eq!(app.controller.document().setup(), &committed);
        let _ = app.update(Message::SetupCancel);
        assert_eq!(
            axes(&app).unwrap().directions,
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
        );
    }
    #[test]
    fn a_single_axis_can_apply_round_trip_and_undo_with_the_preview_orientation() {
        for axis in 0..3 {
            let mut app = app();
            let committed = app.controller.document().setup().clone();
            for (index, value) in ["12", "23", "34"].into_iter().enumerate() {
                let _ = app.update(Message::SetupOriginChanged(index, value.into()));
            }
            app.set_setup_stock_axis(axis, [1., 2., 3.]);
            let preview = axes(&app).unwrap();
            close(preview.origin, [12., 23., 34.]);
            let draft = app.setup_draft.clone().unwrap();
            assert!(app.setup_frame_issue(&draft).is_none());
            let _ = app.update(Message::SetupCommit);
            assert_eq!(app.controller.document().setup(), &draft, "{}", app.status);
            let saved = app.controller.document().setup();
            for index in 0..3 {
                close(
                    saved.frame.to_world_vector(std::array::from_fn(|i| {
                        if i == index {
                            1.
                        } else {
                            0.
                        }
                    })),
                    preview.directions[index],
                );
            }
            let json = serde_json::to_string(app.controller.document()).unwrap();
            let loaded: cam_model::Document = serde_json::from_str(&json).unwrap();
            assert!(app.setup_frame_issue(loaded.setup()).is_none());
            assert_eq!(loaded.setup().frame, draft.frame);
            assert!(app.controller.undo());
            assert_eq!(app.controller.document().setup(), &committed);
            assert!(app.controller.redo());
            assert_eq!(app.controller.document().setup(), &draft);
        }
    }
    #[test]
    fn arrow_flips_preserve_handedness_and_datum_for_every_reference_combination() {
        for mask in 0..8 {
            for axis in 0..3 {
                let mut app = app();
                for index in 0..3 {
                    if mask & (1 << index) != 0 {
                        app.set_setup_stock_axis(
                            index,
                            std::array::from_fn(|i| if i == index { 1. } else { 0. }),
                        );
                    }
                }
                for (index, value) in ["12", "23", "34"].into_iter().enumerate() {
                    let _ = app.update(Message::SetupOriginChanged(index, value.into()));
                }
                let before = app.setup_draft.clone().unwrap();
                let original = axes(&app).unwrap();
                let _ = app.update(Message::SetupAxisToggle(axis));
                let flipped = axes(&app).unwrap();
                close(
                    flipped.directions[axis],
                    original.directions[axis].map(|v| -v),
                );
                close(flipped.origin, original.origin);
                let draft = app.setup_draft.clone().unwrap();
                assert!(
                    app.setup_frame_issue(&draft).is_none(),
                    "mask={mask}, axis={axis}"
                );
                app.controller.apply_setup_settings(draft).unwrap();
                let _ = app.update(Message::SetupAxisToggle(axis));
                assert_eq!(app.setup_draft.as_ref().unwrap(), &before);
            }
        }
    }
    #[test]
    fn viewport_arrows_publish_flip_clicks_and_leave_other_clicks_available() {
        use canvas::Program;
        let app = app();
        let bounds = iced::Rectangle::new(iced::Point::new(50., 80.), iced::Size::new(900., 600.));
        let mut camera = OrbitCamera::framed([-50.; 3], [50.; 3]);
        camera.orient = cam_render::orientation(0.6, 0.7);
        let overlay = WorkpieceOverlay {
            axes: axes(&app).unwrap(),
            camera,
            scale: 1.,
            interactive: true,
            browser_overlay_width: 0.,
        };
        let (_, tips) = overlay.projected(bounds.size()).unwrap();
        for (axis, point) in tips.into_iter().enumerate() {
            let cursor =
                iced::mouse::Cursor::Available(point + iced::Vector::new(bounds.x, bounds.y));
            assert_eq!(overlay.axis_at(bounds, cursor), Some(axis));
            assert_eq!(
                overlay.mouse_interaction(&(), bounds, cursor),
                iced::mouse::Interaction::Pointer
            );
            let action = overlay
                .update(
                    &mut (),
                    &canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )),
                    bounds,
                    cursor,
                )
                .unwrap();
            let (message, _, _) = action.into_inner();
            assert!(matches!(message, Some(Message::SetupAxisToggle(index)) if index == axis));
        }
        let tip = iced::mouse::Cursor::Available(tips[0] + iced::Vector::new(bounds.x, bounds.y));
        let mut overlay = overlay;
        overlay.interactive = false;
        assert!(overlay.axis_at(bounds, tip).is_none());
        overlay.interactive = true;
        overlay.browser_overlay_width = bounds.width;
        assert!(overlay.axis_at(bounds, tip).is_none());
        overlay.browser_overlay_width = 0.;
        let cursor = iced::mouse::Cursor::Available(iced::Point::new(bounds.x + 5., bounds.y + 5.));
        assert!(overlay
            .update(
                &mut (),
                &canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                bounds,
                cursor
            )
            .is_none());
    }
    #[test]
    fn edge_hover_and_snapped_origin_preview_are_temporary() {
        let mut app = app();
        let before = app.setup_draft.clone().unwrap();
        let picking = SetupPickGeometry::new(&app.controller, &before, &app.cad_hidden);
        let (id, _) = picking
            .stock_lines()
            .find(|(_, edge)| (edge[1][1] - edge[0][1]).abs() > 1.)
            .unwrap();
        app.setup_axis_pick = Some(0);
        app.setup_hover.edge = Some(SetupFeature::Stock(id));
        let hover = app.workpiece_axes(&picking).unwrap();
        close(hover.directions[0], [0., 1., 0.]);
        assert_eq!(app.setup_draft.as_ref().unwrap(), &before);
        app.setup_axis_pick = None;
        app.setting_origin = true;
        app.setup_hover.snap = Some(picking_view::SetupSnap {
            point: [4., 5., 6.],
            kind: SnapKind::End,
            source: SetupFeature::Stock(id),
        });
        close(app.workpiece_axes(&picking).unwrap().origin, [4., 5., 6.]);
        app.setting_origin = false;
        app.setting_origin_2pt = true;
        app.origin_first = Some([10., 20., 30.]);
        close(app.workpiece_axes(&picking).unwrap().origin, [10., 5., 18.]);
        assert_eq!(app.setup_draft.as_ref().unwrap(), &before);
    }
    #[test]
    fn triad_stays_at_the_datum_and_legible_at_different_zooms_and_aspects() {
        let app = app();
        let mut axes = axes(&app).unwrap();
        axes.origin = [8., 12., 25.];
        let mut camera = OrbitCamera::framed([-50.; 3], [50.; 3]);
        for zoom in [0., 2., -2.] {
            camera.zoom = zoom;
            for size in [iced::Size::new(900., 600.), iced::Size::new(400., 900.)] {
                let overlay = WorkpieceOverlay {
                    axes: axes.clone(),
                    camera,
                    scale: 1.,
                    interactive: true,
                    browser_overlay_width: 0.,
                };
                let (origin, tips) = overlay.projected(size).unwrap();
                let expected = picking_view::project(
                    camera,
                    iced::Rectangle::new(iced::Point::ORIGIN, size),
                    axes.origin,
                )
                .unwrap()
                .0;
                assert!(origin.distance(expected) < 0.01);
                assert!((origin.distance(tips[0]) - 64.).abs() < 0.01);
                assert!((origin.distance(tips[1]) - 64.).abs() < 0.01);
                assert!(origin.distance(tips[2]) < 0.01);
            }
        }
    }
}
