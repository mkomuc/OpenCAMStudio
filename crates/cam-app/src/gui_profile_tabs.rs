//! Manual holding-tab placement on the operation contour in every camera view.
use super::*;
use iced::widget::column;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Positioning {
    ByDistance,
    ByCount,
    Manual,
}
impl Positioning {
    pub(super) const ALL: [Self; 3] = [Self::ByDistance, Self::ByCount, Self::Manual];
}
impl std::fmt::Display for Positioning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ByDistance => "By distance",
            Self::ByCount => "By count",
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
            .height(32)
            .padding([7, 9])
            .style(move |theme, status| inspector_tab(theme, status, active))
            .on_press(Message::ProfileTabSelected(index))]
            .spacing(4)
            .align_y(Alignment::Center);
            item = item.push(
                inspector_button("×")
                    .height(32)
                    .on_press(Message::ProfileTabRemove(index)),
            );
            items = items.push(item);
        }
        let list = column![
            header,
            container(
                scrollable(items)
                    .id(iced::widget::Id::from("profile-tab-locations"))
                    .width(Length::Fill)
                    .height(136)
            )
            .padding(4)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.12, 0.14, 0.17))),
                border: Border {
                    color: Color::from_rgb(0.25, 0.28, 0.32),
                    width: 1.0,
                    radius: 5.0.into()
                },
                ..Default::default()
            })
        ]
        .spacing(4);
        list.into()
    }

    pub(super) fn profile_tab_operation(&self) -> Option<cam_model::ProfileOp> {
        self.profile_tab_geometry().map(|(op, _)| op)
    }

    /// Curves stay on their source plane; projected silhouettes follow the
    /// operation's Bottom plane. Replacement geometry remains transactional.
    pub(super) fn profile_tab_geometry(&self) -> Option<(cam_model::ProfileOp, f64)> {
        if self.cad_editing_op.is_some() || self.drawing_editing_op.is_some() {
            return None;
        }
        let (op, z) = self.profile_selection_geometry()?;
        let mut operation = Operation::Profile(op);
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
        let Operation::Profile(op) = operation else {
            return None;
        };
        Some((op, z))
    }

    pub(super) fn profile_tab_overlay(
        &self,
        camera: OrbitCamera,
        browser_width: f32,
    ) -> Option<TabOverlay> {
        if !self.inspector_active
            || !self.inspector_enabled
            || self.operation_position_pick.is_some()
        {
            return None;
        }
        let (op, z) = self.profile_tab_geometry()?;
        if !op.chain.is_valid() {
            return None;
        }
        let (vertices, indices) = self.profile_tab_mesh();
        let triangles = indices
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(|(index, triangle)| {
                (
                    index / 12,
                    std::array::from_fn(|i| vertices[triangle[i] as usize].position.map(f64::from)),
                )
            })
            .collect();
        Some(TabOverlay {
            camera,
            triangles,
            scene_mesh: None,
            browser_width,
            points: op.chain.points().iter().map(|p| [p.x, p.y]).collect(),
            positions: self.profile_tab_locations(),
            z,
            frame: self.controller.inspection_setup().frame.clone(),
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

type SceneMesh = (Arc<Vec<MeshVertex>>, Arc<Vec<u32>>);

pub(super) struct TabOverlay {
    camera: OrbitCamera,
    triangles: Vec<(usize, [[f64; 3]; 3])>,
    scene_mesh: Option<SceneMesh>,
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
    pub(super) fn with_scene_mesh(
        mut self,
        vertices: Arc<Vec<MeshVertex>>,
        indices: Arc<Vec<u32>>,
    ) -> Self {
        self.scene_mesh = Some((vertices, indices));
        self
    }

    fn tab_at(&self, point: iced::Point, bounds: iced::Rectangle) -> Option<usize> {
        let ray = self.camera.pick_ray(
            2. * point.x / bounds.width - 1.,
            1. - 2. * point.y / bounds.height,
            bounds.width / bounds.height.max(1.),
        );
        let hit = self
            .triangles
            .iter()
            .filter_map(|(index, triangle)| {
                triangle_depth(ray.origin, ray.direction, *triangle).map(|depth| (*index, depth))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, depth)) = hit {
            let blocked = self.scene_mesh.as_ref().is_some_and(|(vertices, indices)| {
                indices.as_chunks::<3>().0.iter().any(|triangle| {
                    let points = std::array::from_fn(|i| {
                        vertices[triangle[i] as usize].position.map(f64::from)
                    });
                    triangle_depth(ray.origin, ray.direction, points)
                        .is_some_and(|other| other < depth - 1e-4)
                })
            });
            if !blocked {
                return Some(index);
            }
        }
        self.positions
            .iter()
            .enumerate()
            .filter_map(|(index, &position)| {
                self.project(position, bounds)
                    .map(|p| (index, p.distance(point)))
            })
            .filter(|(_, distance)| *distance <= 8.)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }

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
        let cursor = self.cursor(bounds, cursor)?;
        let local = iced::Rectangle::with_size(bounds.size());
        if matches!(
            event,
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
        ) {
            if let Some(index) = self.tab_at(cursor, local) {
                return Some(
                    canvas::Action::publish(Message::ProfileTabSelected(index)).and_capture(),
                );
            }
            if !self.picking {
                return None;
            }
            return Some(if let Some(point) = self.nearest(cursor, local) {
                canvas::Action::publish(Message::ProfileTabPicked(point)).and_capture()
            } else {
                canvas::Action::capture()
            });
        }
        if self.picking
            && matches!(
                event,
                canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left
                ))
            )
        {
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
        if self.cursor(bounds, cursor).is_some_and(|point| {
            self.tab_at(point, iced::Rectangle::with_size(bounds.size()))
                .is_some()
        }) {
            iced::mouse::Interaction::Pointer
        } else if self.picking && self.cursor(bounds, cursor).is_some() {
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
        // Tab solids sit at the profile's bottom plane, inside the CAD body.
        // The viewport's normal depth test therefore hides them when there is
        // no stock mesh. Draw a lightweight projected preview over the scene
        // so tabs remain visible in that case too.
        let mut tab_faces = self
            .triangles
            .iter()
            .filter_map(|(index, triangle)| {
                let projected = triangle.map(|point| {
                    let world = self.frame.to_world_vector(point);
                    picking_view::project(self.camera, local, world)
                });
                let points = projected;
                let points = [points[0]?, points[1]?, points[2]?];
                let depth = points.iter().map(|(_, depth)| depth).sum::<f32>() / 3.0;
                Some((*index, depth, points.map(|(screen, _)| screen)))
            })
            .collect::<Vec<_>>();
        tab_faces.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (index, _, points) in tab_faces {
            let selected = self.selected == Some(index);
            let fill = if selected {
                Color::from_rgba(1.0, 0.60, 0.08, 0.58)
            } else {
                Color::from_rgba(0.86, 0.86, 0.65, 0.52)
            };
            let face = canvas::Path::new(|path| {
                path.move_to(points[0]);
                path.line_to(points[1]);
                path.line_to(points[2]);
                path.close();
            });
            frame.fill(&face, fill);
            frame.stroke(&face, stroke(if selected { 1.5 } else { 0.75 }));
        }
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

/// Two-sided ray/triangle intersection for the rendered tab solids.
fn triangle_depth(origin: [f64; 3], direction: [f64; 3], triangle: [[f64; 3]; 3]) -> Option<f64> {
    let sub = |a: [f64; 3], b: [f64; 3]| std::array::from_fn::<_, 3, _>(|i| a[i] - b[i]);
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let e1 = sub(triangle[1], triangle[0]);
    let e2 = sub(triangle[2], triangle[0]);
    let h = cross(direction, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = sub(origin, triangle[0]);
    let u = dot(s, h) / det;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let distance = dot(e2, q) / det;
    (distance >= 0.).then_some(distance)
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
        // Exercise the legacy binding-backed profile path; named selections
        // intentionally resolve their own entries instead of this binding.
        app.controller.edit_operation(id, |operation| {
            if let Operation::Profile(profile) = operation {
                profile.selections = None;
            }
        });
        app.begin_operation_draft(id);
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        let _ = app.update(Message::ProfileTabsEnabled(true));
        let _ = app.update(Message::ProfileTabPositioning(Positioning::Manual));
        (app, id)
    }

    #[test]
    fn open_chain_edit_preview_stays_on_the_source_plane_in_a_rotated_setup() {
        use cam_model::*;
        let (mut app, id) = selected_cad_profile();
        let rotation = [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]];
        app.controller
            .place_cad_instance(
                1,
                RigidTransform {
                    rotation,
                    translation: [0., 30., 0.],
                },
            )
            .unwrap();
        app.controller.edit_setup(|setup| {
            setup.frame.world_from_setup.rotation = rotation;
        });
        app.begin_operation_draft(id);
        let before = app.controller.document().clone();
        let Operation::Profile(profile) = app.operation_draft.as_mut().unwrap() else {
            panic!("profile")
        };
        profile.selections = Some(
            (300..303)
                .map(|edge| {
                    let mut entry = ProfileGeometryEntry::new(
                        ProfileGeometryKind::Chain,
                        format!("Edge {edge}"),
                    );
                    entry.chain_mode = ProfileChainMode::Open;
                    entry.source = ProfileGeometrySource::Cad(vec![EntityRef {
                        asset_id: 1,
                        revision: 1,
                        instance_id: 1,
                        body_id: 1,
                        kind: EntityKind::Edge,
                        id: edge,
                    }]);
                    entry
                })
                .collect(),
        );
        let geometry =
            crate::cad_operations::geometry_for_setup(app.controller.document()).unwrap();
        for entry in app.preview_profile_entries() {
            let (_, interval) = cam_geometry3d::profile_entry_contours(&geometry, &entry).unwrap();
            assert!(
                interval.is_none(),
                "an open path has no certified wall depth"
            );
        }
        let (profile, z) = app.profile_selection_geometry().unwrap();
        assert_eq!(profile.contours().count(), 3);
        assert_eq!(z, 30.);
        let camera = OrbitCamera::framed([-20.; 3], [100.; 3]);
        assert_eq!(app.profile_tab_geometry().unwrap().1, 30.);
        assert!(app.profile_side_overlay(camera, 0.).is_some());
        let preview = app.operation_geometry_preview();
        assert_eq!(preview.strips.len(), 3);
        assert!(preview
            .strips
            .iter()
            .flat_map(|s| &s.points)
            .all(|p| (p[1] - 30.).abs() < 1e-5));
        assert_eq!(app.controller.document(), &before);
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

        // This body's bottom happens to be setup Z0.
        binding.selected[0].kind = EntityKind::Silhouette;
        binding.selected[0].id = 1;
        binding.recipe = cam_model::ExtractionRecipe::ProfileSilhouette;
        app.operation_binding_preview = Some(binding);
        let silhouette = app.profile_tab_overlay(camera, 0.).unwrap();
        assert_eq!(silhouette.z, 0.);
        assert!(silhouette.points.iter().all(|p| p[0] > 59.));
    }

    #[test]
    fn silhouette_outlines_and_pickers_follow_draft_bottom_in_rotated_setups() {
        use cam_model::*;
        for named in [false, true] {
            let (mut app, id) = selected_cad_profile();
            let rotation = [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]];
            app.controller
                .place_cad_instance(
                    2,
                    RigidTransform {
                        rotation,
                        translation: [60., 20., 0.],
                    },
                )
                .unwrap();
            app.controller
                .edit_setup(|setup| setup.frame.world_from_setup.rotation = rotation);
            app.begin_operation_draft(id);
            let mut reference = app.controller.geometry_state().bindings[&id].selected[0].clone();
            reference.kind = EntityKind::Silhouette;
            reference.id = reference.body_id;
            reference.instance_id = 2;
            let geometry = app.controller.setup_geometry_state().unwrap();
            let binding = cam_geometry3d::resolve_selection(
                &geometry,
                std::slice::from_ref(&reference),
                ExtractionRecipe::ProfileSilhouette,
                30.,
            )
            .unwrap();
            assert_eq!(binding.heights.bottom, 15.);
            app.operation_binding_preview = Some(binding);
            if named {
                let mut entry = ProfileGeometryEntry::new(
                    ProfileGeometryKind::Silhouette,
                    "Projected body".into(),
                );
                entry.source = ProfileGeometrySource::Cad(vec![reference]);
                let Some(Operation::Profile(profile)) = &mut app.operation_draft else {
                    panic!("profile")
                };
                profile.selections = Some(vec![entry]);
            }
            let before = app.controller.document().clone();
            let _ = app.update(Message::HeightReferenceChanged(
                HeightPlaneKind::Bottom,
                HeightReference::SelectedContours,
            ));
            let camera = OrbitCamera::framed([-20.; 3], [100.; 3]);
            for (offset, expected) in [("-2", 13.), ("-4", 11.)] {
                let _ = app.update(Message::HeightOffsetChanged(
                    HeightPlaneKind::Bottom,
                    offset.into(),
                ));
                assert_eq!(app.resolve_height_editor(id).unwrap().bottom, expected);
                assert_eq!(app.profile_tab_overlay(camera, 0.).unwrap().z, expected);
                let frame = &app.controller.inspection_setup().frame;
                let preview = app.operation_geometry_preview();
                assert!(!preview.strips.is_empty());
                assert!(preview
                    .strips
                    .iter()
                    .flat_map(|strip| &strip.points)
                    .all(
                        |point| (frame.to_setup_vector(point.map(f64::from))[2] - expected).abs()
                            < 1e-5
                    ));
                let viewport = app.machining_viewport();
                let selected: Vec<_> = viewport
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.color == PICK_BOUNDARY)
                    .collect();
                assert!(!selected.is_empty());
                assert!(
                    selected.iter().all(|vertex| (frame
                        .to_setup_vector(vertex.position.map(f64::from))[2]
                        - expected)
                        .abs()
                        < 1e-5),
                    "all silhouette highlights must follow Bottom, named={named}"
                );
            }
            assert_eq!(
                app.controller.document(),
                &before,
                "preview edits must stay transactional"
            );
        }
    }

    #[test]
    #[ignore = "qualification against the supplied test2.ocam project"]
    fn supplied_test2_silhouette_preview_uses_bottom() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller
            .open_project(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testfiles/test2.ocam"),
            )
            .unwrap();
        let id = app.controller.inspection_setup().operations[0].id();
        app.begin_operation_draft(id);
        let bottom = app.resolve_height_editor(id).unwrap().bottom;
        assert_eq!(bottom, -3.);
        assert_eq!(app.profile_selection_geometry().unwrap().1, bottom);
        let viewport = app.machining_viewport();
        let frame = &app.controller.inspection_setup().frame;
        let selected: Vec<_> = viewport
            .vertices
            .iter()
            .filter(|vertex| vertex.color == PICK_BOUNDARY)
            .collect();
        assert!(!selected.is_empty());
        assert!(selected.iter().all(|vertex| (frame
            .to_setup_vector(vertex.position.map(f64::from))[2]
            - bottom)
            .abs()
            < 1e-5));
    }

    #[test]
    fn drawing_tab_picker_stays_on_selected_contour_when_stock_height_changes() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .inspection_setup()
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
                triangles: Vec::new(),
                scene_mesh: None,
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
                Some(Message::ProfileTabSelected(0))
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
    fn clicking_a_tab_solid_selects_its_row_without_changing_placements() {
        let (mut app, _) = selected_cad_profile();
        let _ = app.update(Message::FieldChanged(Field::TabWidth, "4".into()));
        let _ = app.update(Message::FieldChanged(Field::TabHeight, "2".into()));
        let _ = app.update(Message::ProfileTabPickToggle);
        let _ = app.update(Message::ProfileTabPicked([10., 0.]));
        let _ = app.update(Message::ProfileTabPicked([20., 10.]));
        let placements = app.profile_tab_operation().unwrap().tabs.positions;
        let bounds = iced::Rectangle {
            x: 23.,
            y: 31.,
            width: 900.,
            height: 700.,
        };
        for (yaw, pitch) in [(0., 0.), (0.5, 0.7), (-0.4, 1.)] {
            let mut camera = OrbitCamera::framed([-10., -10., -20.], [30., 30., 30.]);
            camera.orient = cam_render::orientation(yaw, pitch);
            for placing in [false, true] {
                app.profile_tab_picking = placing;
                let overlay = app.profile_tab_overlay(camera, 0.).unwrap();
                assert_eq!(overlay.triangles.len(), 24);
                // Project the top-face centroid of the second solid, away from
                // its source-plane marker, so this exercises triangle picking.
                let (_, indices) = app.profile_tab_mesh();
                let (vertices, _) = app.profile_tab_mesh();
                let solid = &vertices[24..48];
                let top = solid
                    .iter()
                    .map(|v| v.position[2])
                    .fold(f32::NEG_INFINITY, f32::max);
                let face = indices[36..]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .find(|t| {
                        t.iter()
                            .all(|i| (vertices[*i as usize].position[2] - top).abs() < 1e-5)
                    })
                    .unwrap();
                let center: [f64; 3] = std::array::from_fn(|axis| {
                    face.iter()
                        .map(|i| vertices[*i as usize].position[axis] as f64)
                        .sum::<f64>()
                        / 3.
                });
                let screen = picking_view::project(camera, bounds, center).unwrap().0;
                let action = overlay
                    .update(
                        &mut (),
                        &canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                            iced::mouse::Button::Left,
                        )),
                        bounds,
                        iced::mouse::Cursor::Available(screen),
                    )
                    .expect("tab click captured");
                let message = action.into_inner().0.unwrap();
                assert!(matches!(message, Message::ProfileTabSelected(1)));
                app.operation_tab = OperationTab::Passes;
                let _ = app.update(message);
                assert_eq!(app.operation_tab, OperationTab::Geometry);
                assert_eq!(app.profile_tab_selection, Some(1));
                assert_eq!(
                    app.profile_tab_operation().unwrap().tabs.positions,
                    placements
                );
            }
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
            .inspection_setup()
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
