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
    pub(super) fn tab_location_controls(&self, op: &cam_model::ProfileOp) -> Element<'_, Message> {
        let mut header = row![text("Tab locations").size(12).width(Length::Fill)]
            .spacing(6)
            .align_y(Alignment::Center);
        if op.tabs.manual {
            header = header
                .push(
                    inspector_button(if self.profile_tab_picking {
                        "Done"
                    } else {
                        "Select"
                    })
                    .on_press(Message::ProfileTabPickToggle),
                )
                .push(inspector_button("Clear").on_press_maybe(
                    (!op.tabs.positions.is_empty()).then_some(Message::ProfileTabsClear),
                ));
        }
        let mut items = column![].spacing(3);
        for (index, point) in self.profile_tab_locations().into_iter().enumerate() {
            let active = self.profile_tab_selection == Some(index);
            let mut item = row![button(
                text(format!(
                    "{}   X {}   Y {}",
                    index + 1,
                    fmt_num(point[0]),
                    fmt_num(point[1])
                ))
                .size(12)
                .width(Length::Fill)
            )
            .width(Length::Fill)
            .padding([7, 9])
            .style(move |theme, status| inspector_tab(theme, status, active))
            .on_press(Message::ProfileTabSelected(index))]
            .spacing(4)
            .align_y(Alignment::Center);
            if op.tabs.manual {
                item = item.push(inspector_button("×").on_press(Message::ProfileTabRemove(index)));
            }
            items = items.push(item);
        }
        let list = column![
            header,
            container(scrollable(items).height(Length::Shrink)).max_height(160)
        ]
        .spacing(4);
        list.into()
    }

    pub(super) fn profile_tab_operation(&self) -> Option<cam_model::ProfileOp> {
        self.profile_tab_geometry().map(|(op, _)| op)
    }

    /// Resolve the selected source rather than the cached machining contour or
    /// a stock/height plane. Replacement geometry remains transactional.
    pub(super) fn profile_tab_geometry(&self) -> Option<(cam_model::ProfileOp, f64)> {
        if self.cad_editing_op.is_some() || self.drawing_editing_op.is_some() {
            return None;
        }
        let (mut operation, binding) = if self.creating_operation() {
            self.creation_operation().ok()?
        } else {
            let Selection::Operation(id) = self.controller.selection() else {
                return None;
            };
            let operation = self
                .operation_draft
                .as_ref()
                .filter(|op| op.id() == id)?
                .clone();
            let binding = self
                .operation_binding_preview
                .as_ref()
                .or_else(|| self.controller.geometry_state().bindings.get(&id))
                .cloned();
            (operation, binding)
        };
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
        apply_op_fields(&mut operation, &parsed);
        let Operation::Profile(mut op) = operation else {
            return None;
        };
        let z = if let Some(binding) = binding {
            let geometry =
                crate::cad_operations::geometry_for_setup(self.controller.document()).ok()?;
            let resolved = cam_geometry3d::resolve_binding(&geometry, &binding).ok()?;
            op.chain = resolved.regions.first()?.outer().clone();
            let reference = binding.selected.first()?;
            if reference.kind == EntityKind::Silhouette {
                // The selected XY silhouette is displayed on the setup XY plane.
                0.0
            } else {
                let asset = geometry.asset(reference.asset_id)?;
                let instance = geometry.instance(reference.instance_id)?;
                let wire = asset
                    .topology
                    .loops
                    .iter()
                    .find(|wire| wire.id == reference.id)?;
                let edge_id = wire.edge_uses.first()?.edge_id;
                let edge = asset
                    .topology
                    .edges
                    .iter()
                    .find(|edge| edge.id == edge_id)?;
                let point = match edge.curve {
                    cam_model::CadCurve::Line { start, .. } => start,
                    cam_model::CadCurve::CircleArc { center, .. } => center,
                    cam_model::CadCurve::Unsupported { .. } => return None,
                };
                instance.placement.transform_point(point)[2]
            }
        } else {
            // Selected drawing contours live on setup XY, independently of the
            // operation's cutting heights and the stock top.
            0.0
        };
        Some((op, z))
    }

    pub(super) fn profile_tab_overlay(
        &self,
        camera: OrbitCamera,
        browser_width: f32,
    ) -> Option<TabOverlay> {
        if !self.inspector_active || !self.inspector_enabled {
            return None;
        }
        let (op, z) = self.profile_tab_geometry()?;
        if !op.chain.is_valid() {
            return None;
        }
        Some(TabOverlay {
            camera,
            browser_width,
            points: op.chain.points().iter().map(|p| [p.x, p.y]).collect(),
            positions: self.profile_tab_locations(),
            z,
            frame: self.controller.document().setup().frame.clone(),
            picking: self.profile_tab_picking
                && op.tabs.enabled
                && op.tabs.manual
                && self.operation_tab == OperationTab::Geometry,
            selected: self.profile_tab_selection,
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
    selected: Option<usize>,
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
        frame.stroke(
            &outline.build(),
            canvas::Stroke::default()
                .with_color(Color::from_rgb(0.2, 0.6, 0.98))
                .with_width(2.),
        );
        for (index, point) in self.positions.iter().enumerate() {
            if let Some(p) = self.project(*point, local) {
                frame.fill(
                    &canvas::Path::circle(p, 6.),
                    Color::from_rgb(0.12, 0.14, 0.17),
                );
                let active = self.selected == Some(index);
                frame.stroke(
                    &canvas::Path::circle(p, if active { 9. } else { 6. }),
                    stroke(if active { 4. } else { 2. }),
                );
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

    fn selected_cad_profile() -> (App, u32) {
        use cam_model::*;
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        let points = [
            [0., 0., 0.],
            [20., 0., 0.],
            [20., 20., 0.],
            [0., 20., 0.],
            [0., 0., -5.],
            [20., 0., -5.],
            [20., 20., -5.],
            [0., 20., -5.],
        ];
        let mut topology = TopologySnapshot {
            bodies: vec![CadBody {
                id: 1,
                name: "Plate".into(),
                valid_solid: true,
                bounds: [[0., 0., -5.], [20., 20., 0.]],
            }],
            vertices: points
                .iter()
                .enumerate()
                .map(|(i, p)| CadVertex {
                    id: i as u64 + 1,
                    body_id: 1,
                    point: *p,
                })
                .collect(),
            ..Default::default()
        };
        for (index, (corners, normal)) in [
            ([0, 1, 2, 3], [0., 0., 1.]),
            ([7, 6, 5, 4], [0., 0., -1.]),
            ([4, 5, 1, 0], [0., -1., 0.]),
            ([5, 6, 2, 1], [1., 0., 0.]),
            ([6, 7, 3, 2], [0., 1., 0.]),
            ([7, 4, 0, 3], [-1., 0., 0.]),
        ]
        .into_iter()
        .enumerate()
        {
            let face_id = 100 + index as u64;
            let loop_id = 200 + index as u64;
            let mut edge_uses = Vec::new();
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(points[a][axis]);
                    bounds[1][axis] = bounds[1][axis].max(points[a][axis]);
                }
                let ids = [a as u64 + 1, b as u64 + 1];
                let (edge_id, reversed) = if let Some(edge) = topology
                    .edges
                    .iter_mut()
                    .find(|edge| edge.vertices == ids || edge.vertices == [ids[1], ids[0]])
                {
                    edge.face_ids.push(face_id);
                    (edge.id, edge.vertices != ids)
                } else {
                    let edge_id = 300 + topology.edges.len() as u64;
                    topology.edges.push(CadEdge {
                        id: edge_id,
                        body_id: 1,
                        vertices: ids,
                        face_ids: vec![face_id],
                        curve: CadCurve::Line {
                            start: points[a],
                            end: points[b],
                        },
                    });
                    (edge_id, false)
                };
                edge_uses.push(EdgeUse { edge_id, reversed });
            }
            topology.loops.push(CadLoop {
                id: loop_id,
                body_id: 1,
                face_id,
                edge_uses,
                is_outer: true,
            });
            topology.faces.push(CadFace {
                id: face_id,
                body_id: 1,
                surface: CadSurface::Plane {
                    origin: points[corners[0]],
                    normal,
                },
                loop_ids: vec![loop_id],
                bounds,
            });
        }
        topology.projection = Some(CadProjectionMesh {
            mesh: CadMesh {
                vertices: points.to_vec(),
                triangles: vec![
                    [0, 1, 2],
                    [0, 2, 3],
                    [7, 6, 5],
                    [7, 5, 4],
                    [4, 5, 1],
                    [4, 1, 0],
                    [5, 6, 2],
                    [5, 2, 1],
                    [6, 7, 3],
                    [6, 3, 2],
                    [7, 4, 0],
                    [7, 0, 3],
                ],
                triangle_faces: (100..106).flat_map(|id| [id, id]).collect(),
            },
            chord_tolerance: 0.002,
        });
        app.controller
            .add_cad_asset(
                GeometryAsset {
                    id: 1,
                    revision: 1,
                    name: "Plate".into(),
                    source_format: CadFormat::Step,
                    source_data: vec![].into(),
                    import_options: None,
                    import_diagnostics: vec![],
                    source_fingerprint: String::new(),
                    source_units: SourceUnits::Millimetres,
                    backend: "analytic fixture".into(),
                    topology: topology.into(),
                },
                (1..=2)
                    .map(|id| GeometryInstance {
                        id,
                        asset_id: 1,
                        body_id: 1,
                        name: format!("Plate {id}"),
                        visible: true,
                        placement: RigidTransform {
                            translation: [(id - 1) as f64 * 60., 0., 5.],
                            ..Default::default()
                        },
                    })
                    .collect(),
            )
            .unwrap();
        app.controller
            .edit_setup(|setup| setup.model_instances = vec![1, 2]);
        app.controller.edit_stock(|stock| {
            *stock = Stock::BoundingBox {
                x_offset: 0.,
                y_offset: 0.,
                top: 20.,
                thickness: 30.,
            }
        });
        let reference = EntityRef {
            asset_id: 1,
            revision: 1,
            instance_id: 1,
            body_id: 1,
            kind: EntityKind::Loop,
            id: 200,
        };
        let id = app
            .controller
            .create_cad_operation(OpKind::Profile, vec![reference])
            .unwrap();
        app.begin_operation_draft(id);
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        (app, id)
    }

    #[test]
    fn tab_picker_uses_selected_cad_geometry_and_its_source_plane() {
        let (mut app, id) = selected_cad_profile();
        let camera = OrbitCamera::framed([-20., -20., -10.], [100., 40., 30.]);
        let bounds = iced::Rectangle::with_size(iced::Size::new(900., 700.));
        let overlay = app.profile_tab_overlay(camera, 0.).unwrap();
        assert_eq!(overlay.z, 5.); // Selected loop; stock top is 20 mm.
        let p = overlay.project([10., 0.], bounds).unwrap();
        let actual = overlay.nearest(p, bounds).unwrap();
        assert!((actual[0] - 10.).abs() < 1e-4 && actual[1].abs() < 1e-4);
        let unrelated = overlay.project([70., 0.], bounds).unwrap();
        assert!(overlay.nearest(unrelated, bounds).is_none());

        // Use replacement geometry without applying the operation. Cached XY
        // remains on the first instance while the current selection is on #2.
        let mut binding = app.controller.geometry_state().bindings[&id].clone();
        binding.selected[0].instance_id = 2;
        app.operation_binding_preview = Some(binding.clone());
        app.operation_geometry = Some(binding.selected.clone());
        assert!(
            matches!(app.operation_draft.as_ref(), Some(Operation::Profile(op)) if op.chain.points().iter().all(|p| p.x <= 20.))
        );
        let replacement = app.profile_tab_overlay(camera, 0.).unwrap();
        assert!(replacement.points.iter().all(|p| p[0] >= 60.));
        assert!(replacement.nearest(p, bounds).is_none());
        assert!(replacement.nearest(unrelated, bounds).is_some());

        // Silhouettes are selected on setup XY, even when the body is elevated.
        binding.selected[0].kind = EntityKind::Silhouette;
        binding.selected[0].id = 1;
        app.operation_binding_preview = Some(binding);
        let silhouette = app.profile_tab_overlay(camera, 0.).unwrap();
        assert_eq!(silhouette.z, 0.);
        assert!(silhouette.points.iter().all(|p| p[0] > 59.));
    }

    #[test]
    fn drawing_tab_picker_stays_on_selected_contour_when_stock_height_changes() {
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
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        app.controller.edit_heights(|h| h.top_of_stock = 25.);
        let overlay = app
            .profile_tab_overlay(OrbitCamera::framed([-30.; 3], [100.; 3]), 0.)
            .unwrap();
        assert_eq!(overlay.z, 0.);
        let Operation::Profile(op) = app.operation_draft.as_ref().unwrap() else {
            unreachable!()
        };
        assert_eq!(
            overlay.points,
            op.chain
                .points()
                .iter()
                .map(|p| [p.x, p.y])
                .collect::<Vec<_>>()
        );
    }

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
                selected: None,
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
