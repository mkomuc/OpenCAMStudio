//! Main Machining/Tools surfaces. Camera and semantic selection stay in App.
use super::*;
use iced::widget::column;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Workspace {
    #[default]
    Machining,
    Tools,
}
impl Workspace {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Machining => "Machining",
            Self::Tools => "Tools",
        }
    }
}

pub(super) fn stock_world_bounds(controller: &AppController) -> Option<([f32; 3], [f32; 3])> {
    let (min, max) = controller.stock_box();
    if min[0] >= max[0] || min[1] >= max[1] {
        return None;
    }
    let frame = &controller.document().setup().frame;
    let points: Vec<_> = (0..8)
        .map(|index| {
            frame
                .to_world_vector(std::array::from_fn(|axis| {
                    if index & (1 << axis) == 0 {
                        min[axis]
                    } else {
                        max[axis]
                    }
                }))
                .map(|v| v as f32)
        })
        .collect();
    Some((
        std::array::from_fn(|axis| points.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min)),
        std::array::from_fn(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .fold(f32::NEG_INFINITY, f32::max)
        }),
    ))
}

/// Uncut stock before a run, using the setup box and its orientation exactly.
pub(super) fn uncut_stock_mesh(controller: &AppController) -> (Vec<MeshVertex>, Vec<u32>) {
    let (min, max) = controller.stock_box();
    if !controller.has_geometry() || (0..3).any(|axis| max[axis] <= min[axis]) {
        return (vec![], vec![]);
    }
    let frame = &controller.document().setup().frame;
    let corners: [[f64; 3]; 8] = std::array::from_fn(|index| {
        std::array::from_fn(|axis| {
            if index & (1 << axis) == 0 {
                min[axis]
            } else {
                max[axis]
            }
        })
    });
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (quad, normal) in [
        ([0, 4, 6, 2], [-1., 0., 0.]),
        ([1, 3, 7, 5], [1., 0., 0.]),
        ([0, 1, 5, 4], [0., -1., 0.]),
        ([2, 6, 7, 3], [0., 1., 0.]),
        ([0, 2, 3, 1], [0., 0., -1.]),
        ([4, 5, 7, 6], [0., 0., 1.]),
    ] {
        let base = vertices.len() as u32;
        vertices.extend(quad.map(|corner| MeshVertex {
            position: frame.to_world_vector(corners[corner]).map(|v| v as f32),
            normal: frame.to_world_vector(normal).map(|v| v as f32),
            color: [0.55, 0.58, 0.63, 1.],
        }));
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}
impl App {
    pub(super) fn switch_workspace(&mut self, workspace: Workspace) {
        if self.workspace == workspace {
            return;
        }
        if self.setup_draft_dirty()
            || self.inspector_dirty()
            || self.any_field_invalid()
            || !self.operation_height_overrides_valid()
        {
            self.status = "Apply or Cancel inspector changes before switching workspaces.".into();
            return;
        }
        if self.controller.pending_op().is_some()
            || self.cad_pending_kind.is_some()
            || self.cad_editing_op.is_some()
        {
            self.status =
                "Confirm or Cancel operation selection before switching workspaces.".into();
            return;
        }
        self.cancel_tree_drag();
        if workspace == Workspace::Tools {
            self.machining_tab = self.active_tab;
            self.active_tab = RibbonTab::Tooling;
        } else {
            self.active_tab = self.machining_tab;
        }
        self.workspace = workspace;
        if self.setup_draft.is_some() {
            self.end_setup_draft();
        }
        self.reload_tool_edit();
        self.status = format!("{} workspace.", workspace.label());
    }
    pub(super) fn workspace_picker(&self) -> Element<'_, Message> {
        let choice = |workspace: Workspace| {
            let selected = self.workspace == workspace;
            button(text(workspace.label()).size(12))
                .on_press(Message::WorkspaceChanged(workspace))
                .style(move |_, status| tab_button_style(selected, status))
        };
        row![choice(Workspace::Machining), choice(Workspace::Tools)]
            .spacing(3)
            .into()
    }
    pub(super) fn workspace_main(&self) -> Element<'_, Message> {
        if self.workspace == Workspace::Tools {
            return self.tools_table();
        }
        let viewport = self.machining_viewport();
        let workpiece =
            self.workpiece_axes(&viewport.setup_geometry)
                .map(|axes| wcs_view::WorkpieceOverlay {
                    axes,
                    camera: viewport.camera(),
                    scale: self.settings.view.origin_marker_scale,
                    interactive: !self.in_origin_pick(),
                    browser_overlay_width: viewport.browser_overlay_width,
                });
        let labels = grid_view::GridLabels {
            camera: viewport.camera(),
        };
        let height_preview = self.height_preview(viewport.camera(), viewport.browser_overlay_width);
        let mut layers = iced::widget::stack![
            shader(viewport).width(Length::Fill).height(Length::Fill),
            canvas::Canvas::new(labels)
                .width(Length::Fill)
                .height(Length::Fill)
        ];
        if let Some(preview) = height_preview {
            layers = layers.push(
                canvas::Canvas::new(preview)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        if let Some(workpiece) = workpiece {
            layers = layers.push(
                canvas::Canvas::new(workpiece)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        if self.show_project_browser {
            let browser =
                container(column![text("Project").size(13), self.project_tree()].spacing(6))
                    .width(Length::Fixed(self.project_px.clamp(220., 360.)))
                    .height(Length::Fill)
                    .padding(8)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(
                            0.14, 0.16, 0.18, 0.96,
                        ))),
                        border: Border {
                            color: palette::SEPARATOR,
                            width: 1.,
                            radius: 5.0.into(),
                        },
                        ..Default::default()
                    });
            layers = layers.push(container(browser).padding(8).height(Length::Fill));
        }
        column![
            layers.width(Length::Fill).height(Length::Fill),
            self.viewport_pick_toolbar()
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
    fn machining_viewport(&self) -> Viewport {
        let hidden: Vec<_> = self
            .controller
            .setups()
            .iter()
            .flat_map(|setup| {
                let hidden_setup = self.hidden_setups.contains(&setup.id);
                setup
                    .operations
                    .iter()
                    .map(|op| op.id())
                    .filter(move |id| hidden_setup || self.hidden_ops.contains(id))
            })
            .collect();
        let mut viewport = Viewport::new(
            &self.controller,
            self.cad_filter.kind(),
            self.cad_hover.as_ref(),
            &self.cad_hidden,
            self.cad_select_through,
            self.show_stock,
            self.view,
            self.show_gizmo,
            self.gizmo_size,
            &self.settings,
            &self.focus_ops.iter().copied().collect::<Vec<_>>(),
            &hidden,
            self.snap_hover.map(|hit| (hit, self.snap_aperture)),
            self.hover_loop,
            self.in_origin_pick(),
            self.show_origin && self.controller.selection() != Selection::Setup,
            self.origin_first.map(|p| {
                self.setup_draft
                    .as_ref()
                    .unwrap_or_else(|| self.controller.document().setup())
                    .frame
                    .to_world_vector(p)
            }),
            self.show_envelope.then(|| {
                let (x, y, z) = self.controller.machine().envelope.extent();
                [x as f32, y as f32, z as f32]
            }),
        );
        viewport.setup_geometry = SetupPickGeometry::new(
            &self.controller,
            self.setup_draft
                .as_ref()
                .unwrap_or_else(|| self.controller.document().setup()),
            &self.cad_hidden,
        );
        viewport.setup_axis = self.setup_axis_pick.is_some();
        viewport.setup_hover = self.setup_hover.clone();
        viewport.setup_snaps = if self.settings.snapping.enabled {
            self.snaps.clone()
        } else {
            vec![]
        };
        viewport.frame_bounds = union_bounds(
            viewport.setup_geometry.stock_bounds(),
            viewport.cad_scene.bounds(),
        );
        viewport.cad_silhouette = self.modifiers.control() && self.height_reference_pick.is_none();
        viewport.cad_operation_kind = self.cad_pending_kind.or_else(|| {
            self.cad_editing_op.and_then(|id| {
                self.current_operation(id)
                    .map(AppController::operation_kind)
            })
        });
        if self.height_reference_pick.is_some() {
            viewport.cad_operation_kind = None;
        }
        viewport.browser_overlay_width = if self.show_project_browser {
            self.project_px.clamp(220., 360.)
        } else {
            0.
        };
        viewport.planar_features = self.planar_selection.clone();
        viewport
    }
    fn viewport_pick_toolbar(&self) -> Element<'_, Message> {
        let toggle = |label: &'static str, active, message| {
            button(text(label).size(12))
                .padding([4, 8])
                .on_press(message)
                .style(move |_, status| snap_toggle_style(active, status))
        };
        let hint = self
            .setup_hover
            .snap
            .as_ref()
            .map(|hit| {
                let kind = match hit.kind {
                    SnapKind::End => "Endpoint",
                    SnapKind::Mid => "Midpoint",
                    SnapKind::Quadrant => "Quadrant",
                    SnapKind::Nearest => "Nearest",
                };
                let source = match hit.source {
                    SetupFeature::Stock(_) => "stock",
                    SetupFeature::Model(_) => "model",
                    SetupFeature::Drawing(_) => "drawing",
                };
                format!("{} · {}", kind, source)
            })
            .unwrap_or_default();
        let controls = row![
            toggle("Show stock · F8", self.show_stock, Message::ToggleStock),
            toggle(
                "Transparent stock",
                self.settings.view.transparent_stock,
                Message::ToggleTransparentStock
            ),
            toggle(
                "Highlight features",
                self.settings.snapping.highlight_features,
                Message::ToggleFeatureHighlights
            ),
            toggle(
                "Snap",
                self.settings.snapping.enabled,
                Message::ToggleSnapping
            ),
            toggle(
                "Endpoints",
                self.snaps.contains(&SnapKind::End),
                Message::ToggleSnap(SnapKind::End)
            ),
            toggle(
                "Midpoints",
                self.snaps.contains(&SnapKind::Mid),
                Message::ToggleSnap(SnapKind::Mid)
            ),
            toggle(
                "Quadrants",
                self.snaps.contains(&SnapKind::Quadrant),
                Message::ToggleSnap(SnapKind::Quadrant)
            ),
            toggle(
                "Nearest",
                self.snaps.contains(&SnapKind::Nearest),
                Message::ToggleSnap(SnapKind::Nearest)
            ),
            text(hint).size(12),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        container(
            scrollable(controls).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            )),
        )
        .padding(6)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(palette::RIBBON_BG)),
            ..Default::default()
        })
        .into()
    }
    pub(super) fn tools_table(&self) -> Element<'_, Message> {
        let mode = |view, label| {
            let active = self.library_view == view;
            button(text(label).size(12))
                .on_press(Message::SetLibraryView(view))
                .style(move |_, status| snap_toggle_style(active, status))
        };
        let mut list = column![
            row![
                text("Tool library").size(18),
                mode(LibraryView::Ordered, "By number"),
                mode(LibraryView::Grouped, "By family")
            ]
            .spacing(12),
            row![
                text("Tool").width(54),
                text("Cutter").width(Length::Fill),
                text("Ø mm").width(62),
                text("Length mm").width(80),
                text("RPM").width(65),
                text("Feed mm/min").width(100)
            ]
        ]
        .spacing(6)
        .padding(16);
        let mut tools: Vec<_> = self.library.tools.iter().enumerate().collect();
        match self.library_view {
            LibraryView::Ordered => tools.sort_by_key(|(_, tool)| tool.number),
            LibraryView::Grouped => tools.sort_by(|(_, a), (_, b)| {
                kind_order(a.kind)
                    .cmp(&kind_order(b.kind))
                    .then(a.diameter.total_cmp(&b.diameter))
            }),
        }
        for (index, tool) in tools {
            let selected = index == self.lib_sel;
            let cells = row![
                text(format!("T{}", tool.number)).width(54),
                text(tool.kind.to_string()).width(Length::Fill),
                text(fmt_num(tool.diameter)).width(62),
                text(fmt_num(tool.length)).width(80),
                text(fmt_num(tool.nominal_rpm)).width(65),
                text(fmt_num(tool.nominal_feed)).width(100)
            ]
            .spacing(4);
            let item = container(cells)
                .padding(7)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: selected.then_some(Background::Color(palette::SELECT_BG)),
                    ..Default::default()
                });
            list = list.push(
                mouse_area(item)
                    .on_press(Message::SelectLibraryTool(index))
                    .on_right_press(Message::LibToolMenu(index)),
            );
        }
        scrollable(list)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    pub(super) fn tools_inspector(&self) -> Element<'_, Message> {
        let mut body = column![].spacing(6);
        if let Some(tool) = self.preview_tool() {
            body = body.push(
                canvas::Canvas::new(ToolCanvas::new(&tool))
                    .width(Length::Fill)
                    .height(190),
            );
        }
        body = body.push(self.library_editor());
        column![
            body.height(Length::Fill),
            inspector_footer(
                row![
                    inspector_button("Cancel").on_press(Message::CancelToolDraft),
                    Space::new().width(Length::Fill),
                    inspector_button("Apply changes")
                        .style(inspector_primary)
                        .on_press_maybe(
                            (!self.any_field_invalid() && self.inspector_dirty())
                                .then_some(Message::Apply)
                        ),
                ]
                .spacing(8)
            )
        ]
        .height(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_visibility_and_transparency_work_before_and_after_simulation() {
        let mut app = picking_app();
        app.setup_draft = None;
        app.show_stock = true;
        app.settings.view.transparent_stock = false;
        let opaque = app.machining_viewport();
        assert_eq!(opaque.mesh_indices.len(), 36, "uncut stock before a run");
        assert!(opaque.transparent_stock_indices.is_empty());
        let frame_bounds = opaque.frame_bounds;
        let drawing = opaque.vertices.clone();

        app.settings.view.transparent_stock = true;
        let transparent = app.machining_viewport();
        assert!(transparent.mesh_indices.is_empty());
        assert_eq!(transparent.transparent_stock_indices.len(), 36);
        assert!(transparent
            .transparent_stock_vertices
            .iter()
            .all(|v| v.color[3] == 0.25));
        assert_eq!(transparent.frame_bounds, frame_bounds);
        assert_eq!(transparent.vertices, drawing, "drawing stays visible");

        app.show_stock = false;
        let hidden = app.machining_viewport();
        assert!(hidden.mesh_indices.is_empty() && hidden.transparent_stock_indices.is_empty());
        assert_eq!(
            hidden.frame_bounds, frame_bounds,
            "visibility must not reframe"
        );

        app.controller.run(&cam_toolpath::CancelToken::new());
        app.show_stock = true;
        let simulated = app.machining_viewport();
        let outcome = app.controller.outcome().unwrap();
        assert_eq!(
            simulated.transparent_stock_indices.as_slice(),
            outcome.stock_indices
        );
        assert_eq!(
            simulated.transparent_stock_vertices.len(),
            outcome.stock_vertices.len()
        );
        assert!(!simulated.transparent_stock_indices.is_empty());
    }

    #[test]
    fn uncut_stock_follows_rotated_setup_in_world_space() {
        let mut app = picking_app();
        let mut setup = app.controller.document().setup().clone();
        setup.stock = cam_model::Stock::RelativeBox {
            offsets: [[5.; 3]; 2],
        };
        setup.frame =
            cam_model::SetupFrame::from_axes(Some([0., 1., 0.]), Some([0., 0., 1.]), None).unwrap();
        app.controller.apply_setup_settings(setup).unwrap();
        let (min, max) = app.controller.stock_box();
        let frame = &app.controller.document().setup().frame;
        let (vertices, indices) = uncut_stock_mesh(&app.controller);
        assert_eq!(indices.len(), 36);
        for vertex in vertices {
            let point = frame.to_setup_vector(vertex.position.map(f64::from));
            for axis in 0..3 {
                assert!(
                    (point[axis] - min[axis]).abs() < 1e-4
                        || (point[axis] - max[axis]).abs() < 1e-4
                );
            }
        }
    }

    fn picking_app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.begin_setup_draft();
        let draft = app.setup_draft.as_mut().unwrap();
        draft.stock = cam_model::Stock::RelativeBox {
            offsets: [[5., 5., 5.], [5., 5., 5.]],
        };
        app.show_project_browser = false;
        app.show_gizmo = false;
        app.settings.snapping.enabled = true;
        app.snaps = vec![SnapKind::End, SnapKind::Mid, SnapKind::Quadrant];
        app
    }
    fn assert_point(a: [f64; 3], b: [f64; 3]) {
        assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-5), "{a:?} != {b:?}");
    }
    #[test]
    fn stock_origin_hover_and_click_agree_in_rotated_and_portrait_views() {
        use shader::Program;
        let mut app = picking_app();
        let frame = cam_model::SetupFrame::from_xy([0., 0., 1.], [1., 0., 0.]).unwrap();
        app.setup_draft.as_mut().unwrap().frame = frame.clone();
        app.setting_origin = true;
        for (yaw, pitch) in [(0., 0.), (0.63, 0.71)] {
            app.view.yaw = yaw;
            app.view.pitch = pitch;
            for (width, height) in [(900., 600.), (400., 900.)] {
                let bounds =
                    iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(width, height));
                let viewport = app.machining_viewport();
                // Choose the frontmost corner so coincident front/back points in a
                // top view have an unambiguous expected depth.
                let point = viewport
                    .setup_geometry
                    .stock_lines()
                    .flat_map(|(_, p)| p)
                    .min_by(|a, b| {
                        picking_view::project(viewport.camera(), bounds, *a)
                            .unwrap()
                            .1
                            .total_cmp(
                                &picking_view::project(viewport.camera(), bounds, *b)
                                    .unwrap()
                                    .1,
                            )
                    })
                    .unwrap();
                let projected = picking_view::project(viewport.camera(), bounds, point)
                    .unwrap()
                    .0;
                let cursor = iced::Point::new(projected.x + 3., projected.y - 2.);
                let hover = viewport.setup_hover_at(cursor, bounds).snap.unwrap();
                assert!(matches!(hover.source, SetupFeature::Stock(_)));
                assert_point(hover.point, point);
                let action = viewport
                    .update(
                        &mut ViewportState::default(),
                        &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                            iced::mouse::Button::Left,
                        )),
                        bounds,
                        iced::mouse::Cursor::Available(cursor),
                    )
                    .unwrap();
                let (message, _, _) = action.into_inner();
                let Some(Message::OriginPointPicked(picked)) = message else {
                    panic!("expected origin pick")
                };
                assert_point(picked, hover.point);
                let _ = app.update(Message::OriginPointPicked(picked));
                assert_point(
                    app.setup_draft.as_ref().unwrap().origin,
                    frame.to_setup_vector(point),
                );
                assert_ne!(app.setup_draft.as_ref().unwrap().origin[2], 0.);
                app.setting_origin = true;
            }
        }
    }
    #[test]
    fn stock_axis_clicks_commit_round_trip_and_undo_without_cad_entities() {
        use shader::Program;
        let mut app = picking_app();
        let committed = app.controller.document().setup().clone();
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(900., 600.));
        app.view.yaw = 0.6;
        app.view.pitch = 0.7;
        for axis in 0..2 {
            let _ = app.update(Message::SetupAxisPick(axis));
            let viewport = app.machining_viewport();
            let (_, edge) = viewport
                .setup_geometry
                .stock_lines()
                .find(|(_, p)| (p[1][axis] - p[0][axis]).abs() > 1.)
                .unwrap();
            let midpoint = std::array::from_fn(|i| (edge[0][i] + edge[1][i]) / 2.);
            let cursor = picking_view::project(viewport.camera(), bounds, midpoint)
                .unwrap()
                .0;
            assert!(matches!(
                viewport.setup_hover_at(cursor, bounds).edge,
                Some(SetupFeature::Stock(_))
            ));
            let action = viewport
                .update(
                    &mut ViewportState::default(),
                    &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )),
                    bounds,
                    iced::mouse::Cursor::Available(cursor),
                )
                .unwrap();
            let (message, _, _) = action.into_inner();
            assert!(matches!(message, Some(Message::SetupStockEdgePicked(_))));
            let _ = app.update(message.unwrap());
            assert!(app.setup_axis_pick.is_none());
        }
        let draft = app.setup_draft.clone().unwrap();
        assert!(app.setup_frame_issue(&draft).is_none());
        assert!(draft.frame.references.stock_axes[0].is_some());
        assert!(draft.frame.references.stock_axes[1].is_some());
        app.controller.apply_setup_settings(draft.clone()).unwrap();
        let json = serde_json::to_string(app.controller.document()).unwrap();
        let loaded: cam_model::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.setup().frame, draft.frame);
        assert!(app.controller.undo());
        assert_eq!(app.controller.document().setup(), &committed);
        assert!(app.controller.redo());
        assert_eq!(app.controller.document().setup().frame, draft.frame);
    }
    #[test]
    fn snap_filters_disable_catching_and_free_origin_uses_the_setup_plane() {
        let mut app = picking_app();
        app.setting_origin = true;
        app.setup_draft.as_mut().unwrap().frame =
            cam_model::SetupFrame::from_xy([0., 0., 1.], [1., 0., 0.]).unwrap();
        app.setup_draft.as_mut().unwrap().heights.top_of_stock = 3.;
        app.view.yaw = 0.5;
        app.view.pitch = 0.6;
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(400., 900.));
        let viewport = app.machining_viewport();
        let (_, edge) = viewport.setup_geometry.stock_lines().next().unwrap();
        let midpoint = std::array::from_fn(|i| (edge[0][i] + edge[1][i]) / 2.);
        let cursor = picking_view::project(viewport.camera(), bounds, midpoint)
            .unwrap()
            .0;
        assert_eq!(
            viewport.setup_hover_at(cursor, bounds).snap.unwrap().kind,
            SnapKind::Mid
        );
        app.snaps = vec![SnapKind::End];
        assert!(app
            .machining_viewport()
            .setup_hover_at(cursor, bounds)
            .snap
            .is_none());
        app.settings.snapping.enabled = false;
        let viewport = app.machining_viewport();
        assert!(viewport.setup_hover_at(cursor, bounds).snap.is_none());
        let point = viewport.origin_point_at(cursor, bounds).unwrap();
        let local = app
            .setup_draft
            .as_ref()
            .unwrap()
            .frame
            .to_setup_vector(point);
        assert!((local[2] - 3.).abs() < 1e-5);
    }
    #[cfg(feature = "occt")]
    #[test]
    fn native_cad_vertices_snap_in_3d_and_hidden_instances_do_not_contribute() {
        let mut app = picking_app();
        app.controller = AppController::new(crate::default_machine());
        app.controller
            .import_cad(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../cam-kernel/fixtures/box-mm.step"),
            )
            .unwrap();
        app.begin_setup_draft();
        app.setup_draft.as_mut().unwrap().stock = cam_model::Stock::RelativeBox {
            offsets: [[5.; 3], [5.; 3]],
        };
        app.setting_origin = true;
        app.view.yaw = 0.6;
        app.view.pitch = 0.7;
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(900., 600.));
        let viewport = app.machining_viewport();
        let instance = &app.controller.geometry_state().instances[0];
        let asset = app
            .controller
            .geometry_state()
            .asset(instance.asset_id)
            .unwrap();
        let point = asset
            .topology
            .vertices
            .iter()
            .map(|v| instance.placement.transform_point(v.point))
            .min_by(|a, b| {
                picking_view::project(viewport.camera(), bounds, *a)
                    .unwrap()
                    .1
                    .total_cmp(
                        &picking_view::project(viewport.camera(), bounds, *b)
                            .unwrap()
                            .1,
                    )
            })
            .unwrap();
        let cursor = picking_view::project(viewport.camera(), bounds, point)
            .unwrap()
            .0;
        let hit = viewport.setup_hover_at(cursor, bounds).snap.unwrap();
        assert!(matches!(hit.source, SetupFeature::Model(_)));
        assert_point(hit.point, point);
        app.cad_hidden.insert(instance.id);
        assert!(app
            .machining_viewport()
            .setup_hover_at(cursor, bounds)
            .snap
            .is_none());
    }
    #[test]
    fn default_workspace_and_clean_round_trip_preserve_machining_context() {
        let (mut app, _) = App::new();
        assert_eq!(app.workspace, Workspace::Machining);
        assert!(app.show_project_browser);
        assert!(
            app.pane_handle(Pane::Project).is_none(),
            "browser overlays the main view"
        );
        app.controller.open_dxf(SAMPLE_DXF, "sample").unwrap();
        app.setup_draft = None;
        app.refresh_fields();
        app.view.zoom = 1.4;
        let before = app.view;
        let selection = app.controller.selection();
        let setup = app.controller.active_setup_id();
        app.switch_workspace(Workspace::Tools);
        assert!(app.library_mode());
        app.switch_workspace(Workspace::Machining);
        assert_eq!(app.controller.selection(), selection);
        assert_eq!(app.controller.active_setup_id(), setup);
        assert_eq!(app.view.zoom, before.zoom);
        assert_eq!(app.view.pan, before.pan);
    }
    #[test]
    fn dirty_tool_cannot_be_abandoned_by_workspace_or_library_navigation() {
        let (mut app, _) = App::new();
        app.switch_workspace(Workspace::Tools);
        let original = app.library.tools[app.lib_sel];
        app.tool_edit.as_mut().unwrap().length += 1.;
        app.switch_workspace(Workspace::Machining);
        assert_eq!(app.workspace, Workspace::Tools);
        let _ = app.update(Message::SelectLibraryTool(
            (app.lib_sel + 1) % app.library.tools.len(),
        ));
        assert_eq!(app.tool_edit.unwrap().length, original.length + 1.);
        let _ = app.update(Message::CancelToolDraft);
        assert_eq!(app.tool_edit.unwrap(), original);
        app.switch_workspace(Workspace::Machining);
        assert_eq!(app.workspace, Workspace::Machining);
    }
    #[test]
    fn browser_blocks_viewport_click_drag_wheel_and_releases_armed_picks() {
        use iced::mouse::{Button, Cursor, Event as Mouse, ScrollDelta};
        use shader::Program;
        let controller = AppController::new(crate::default_machine());
        let mut viewport = Viewport::new(
            &controller,
            EntityKind::Face,
            None,
            &BTreeSet::new(),
            false,
            true,
            ViewControls::default(),
            true,
            80.,
            &crate::Settings::default(),
            &[],
            &[],
            None,
            None,
            false,
            true,
            None,
            None,
        );
        viewport.browser_overlay_width = 250.;
        let bounds = iced::Rectangle {
            x: 40.,
            y: 60.,
            width: 800.,
            height: 600.,
        };
        let point = iced::Point::new(100., 620.); // blank lower browser area
        for event in [
            Mouse::ButtonPressed(Button::Left),
            Mouse::ButtonReleased(Button::Left),
            Mouse::CursorMoved { position: point },
            Mouse::WheelScrolled {
                delta: ScrollDelta::Lines { x: 0., y: 1. },
            },
        ] {
            let mut state = ViewportState {
                drag: Some(DragMode::Orbit),
                last: Some(point),
                press: Some(point),
                gizmo_arm: Some((1., 1.)),
                cad_arm: Some(vec![]),
                planar_arm: Some(([0., 0.], 1.)),
            };
            let action = viewport
                .update(
                    &mut state,
                    &iced::Event::Mouse(event),
                    bounds,
                    Cursor::Available(point),
                )
                .unwrap();
            let (message, _, status) = action.into_inner();
            assert!(
                message.is_none(),
                "overlay must not publish picks or camera movement"
            );
            assert_eq!(status, iced::event::Status::Captured);
            assert!(
                state.drag.is_none()
                    && state.cad_arm.is_none()
                    && state.planar_arm.is_none()
                    && state.gizmo_arm.is_none()
            );
        }
        assert!(
            !browser_contains(point, bounds, 0.),
            "hidden browser cannot intercept input"
        );
        assert!(!browser_contains(
            iced::Point::new(400., 200.),
            bounds,
            250.
        ));
    }
    #[test]
    fn renumber_and_tool_enum_changes_cannot_discard_invalid_buffers() {
        let (mut app, _) = App::new();
        app.switch_workspace(Workspace::Tools);
        let original = app.library.clone();
        let _ = app.update(Message::FieldChanged(Field::ToolLength, "invalid".into()));
        let _ = app.update(Message::ToolCuttingDirChanged(CutDir::Up));
        assert_eq!(app.fields[&Field::ToolLength], "invalid");
        assert!(app.any_field_invalid());
        let _ = app.update(Message::RenumberLibrary);
        let _ = app.update(Message::RenumberConfirmed(true));
        assert_eq!(app.library.tools, original.tools);
        assert_eq!(app.fields[&Field::ToolLength], "invalid");
        let _ = app.update(Message::CancelToolDraft);
        assert!(!app.inspector_dirty());
    }
    #[test]
    fn creating_setup_leaves_passive_cad_inspection_for_the_setup_editor() {
        let (mut app, _) = App::new();
        app.cad_inspecting = true;
        let _ = app.update(Message::SetupNew);
        assert!(!app.cad_inspecting);
        assert_eq!(app.controller.selection(), Selection::Setup);
        assert!(app.setup_draft.is_some());
    }
    #[test]
    fn first_motion_past_click_slop_orbits_without_a_second_move_event() {
        use shader::Program;
        let controller = AppController::new(crate::default_machine());
        let viewport = Viewport::new(
            &controller,
            EntityKind::Face,
            None,
            &BTreeSet::new(),
            false,
            true,
            ViewControls::default(),
            true,
            80.,
            &crate::Settings::default(),
            &[],
            &[],
            None,
            None,
            false,
            true,
            None,
            None,
        );
        let bounds = iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 600.,
        };
        let start = iced::Point::new(400., 300.);
        let end = iced::Point::new(440., 330.);
        for arm in 0..3 {
            let mut state = ViewportState {
                drag: Some(DragMode::Orbit),
                last: Some(start),
                press: Some(start),
                ..Default::default()
            };
            match arm {
                0 => state.planar_arm = Some(([0., 0.], 1.)),
                1 => state.cad_arm = Some(vec![]),
                _ => state.gizmo_arm = Some((0., 0.)),
            }
            let action = viewport
                .update(
                    &mut state,
                    &iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: end }),
                    bounds,
                    iced::mouse::Cursor::Available(end),
                )
                .unwrap();
            let (message, _, status) = action.into_inner();
            assert!(matches!(message, Some(Message::OrbitBy(x, y)) if x > 0. && y > 0.));
            assert_eq!(status, iced::event::Status::Captured);
            assert!(
                state.planar_arm.is_none() && state.cad_arm.is_none() && state.gizmo_arm.is_none()
            );
        }
    }
    #[cfg(feature = "occt")]
    #[test]
    fn native_step_face_click_selects_profile_boundary_in_the_operation_viewport() {
        use shader::Program;
        let mut controller = AppController::new(crate::default_machine());
        controller
            .import_cad(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../cam-kernel/fixtures/box-mm.step"),
            )
            .unwrap();
        let mut viewport = Viewport::new(
            &controller,
            EntityKind::Loop,
            None,
            &BTreeSet::new(),
            false,
            true,
            ViewControls::default(),
            true,
            80.,
            &crate::Settings::default(),
            &[],
            &[],
            None,
            None,
            false,
            true,
            None,
            None,
        );
        viewport.cad_operation_kind = Some(OpKind::Profile);
        let body = &controller.geometry_state().assets[0].topology.bodies[0];
        let world = [
            (body.bounds[0][0] + body.bounds[1][0]) as f32 * 0.5,
            (body.bounds[0][1] + body.bounds[1][1]) as f32 * 0.5,
            body.bounds[1][2] as f32,
        ];
        let bounds = iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 700.,
        };
        let matrix = viewport.camera().view_proj(bounds.width / bounds.height);
        let clip: [f32; 4] = std::array::from_fn(|row| {
            matrix[0][row] * world[0]
                + matrix[1][row] * world[1]
                + matrix[2][row] * world[2]
                + matrix[3][row]
        });
        let point = iced::Point::new(
            (clip[0] / clip[3] + 1.) * bounds.width / 2.,
            (1. - clip[1] / clip[3]) * bounds.height / 2.,
        );
        let mut state = ViewportState::default();
        let _ = viewport.update(
            &mut state,
            &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            bounds,
            iced::mouse::Cursor::Available(point),
        );
        let action = viewport
            .update(
                &mut state,
                &iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )),
                bounds,
                iced::mouse::Cursor::Available(point),
            )
            .unwrap();
        let (message, _, _) = action.into_inner();
        assert!(
            matches!(message, Some(Message::CadPicked(ref candidates)) if !candidates.is_empty() && candidates[0].entity.kind == EntityKind::Loop)
        );
        // Ctrl takes precedence over the operation/filter. The actual click
        // message retains the body-instance reference and replaces face picks.
        viewport.cad_silhouette = true;
        let candidates = viewport.cad_candidates_at(point, bounds);
        assert_eq!(candidates[0].entity.kind, EntityKind::Silhouette);
        let selected = candidates[0].entity.clone();
        let (mut app, _) = App::new();
        app.controller = controller;
        app.modifiers = iced::keyboard::Modifiers::CTRL;
        let _ = app.update(Message::CadPicked(candidates.clone()));
        assert_eq!(app.controller.cad_selection(), &[selected.clone()]);
        assert!(
            app.machining_viewport()
                .vertices
                .iter()
                .any(|vertex| vertex.color == PICK_BOUNDARY && vertex.position[2].abs() < 1e-6),
            "the XY selection overlay remains visible through the stock"
        );
        let _ = app.update(Message::CadPicked(candidates.clone()));
        assert_eq!(app.controller.cad_selection(), &[selected]);
        app.modifiers = iced::keyboard::Modifiers::SHIFT;
        let _ = app.update(Message::CadPicked(candidates));
        assert!(app.controller.cad_selection().is_empty());
    }
}
