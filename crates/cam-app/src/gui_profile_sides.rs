//! Per-contour cutter-side editing and viewport markers.
use super::*;
use cam_model::{ProfileContour, ProfileOp};

impl App {
    pub(super) fn apply_profile_side_overrides(&self, profile: &mut ProfileOp) {
        let sides: Vec<_> = profile
            .contours()
            .map(|contour| {
                self.profile_side_overrides
                    .iter()
                    .find(|edited| edited.chain == contour.chain && edited.closed == contour.closed)
                    .map_or(contour.side, |edited| edited.side)
            })
            .collect();
        for (index, side) in sides.into_iter().enumerate() {
            profile.set_contour_side(index, side);
        }
    }

    /// Geometry picking works before a cutter is chosen or numeric fields are valid.
    /// It never commits a replacement or modifies the source file.
    pub(super) fn profile_selection_geometry(&self) -> Option<(ProfileOp, f64)> {
        let Operation::Profile(mut profile) = self.operation_draft.as_ref()?.clone() else {
            return None;
        };
        if !self.creating_operation()
            && self.controller.selection() != Selection::Operation(profile.id)
        {
            return None;
        }
        if profile.selections.is_some() {
            let geometry =
                crate::cad_operations::geometry_for_setup(self.controller.document()).ok()?;
            let mut all = Vec::new();
            let mut plane = 0.;
            let mut entries = self.preview_profile_entries();
            for entry in &mut entries {
                if let Ok((contours, heights)) =
                    cam_geometry3d::profile_entry_contours(&geometry, &entry)
                {
                    if all.is_empty() && entry.kind != cam_model::ProfileGeometryKind::Silhouette {
                        plane = heights.map_or(0., |h| h.top);
                    }
                    entry.contour_count = contours.len();
                    all.extend(contours);
                }
            }
            profile.selections = Some(entries);
            if all.is_empty() {
                return None;
            }
            let first = all.remove(0);
            profile.chain = first.chain;
            profile.closed = first.closed;
            profile.side = first.side;
            profile.additional_chains = all;
            return Some((profile, plane));
        }
        let original = self
            .operation_binding_preview
            .as_ref()
            .or_else(|| self.controller.geometry_state().bindings.get(&profile.id));
        let mut binding = original.cloned();
        let mut preserve_sides = true;
        if self.cad_pending_kind == Some(OpKind::Profile) || self.cad_editing_op.is_some() {
            let selected = self.controller.cad_selection();
            preserve_sides =
                original.is_some_and(|old| old.selected == selected) && !self.creating_operation();
            if !preserve_sides {
                binding = Some(
                    crate::cad_operations::create_operation(
                        self.controller.document(),
                        OpKind::Profile,
                        selected.to_vec(),
                        profile.id,
                        profile.tool,
                        profile.work_offset,
                        *self.controller.defaults(),
                    )
                    .ok()?
                    .1,
                );
            }
        } else if self.controller.pending_op().is_some() {
            let operation = self
                .controller
                .operation_with_drawing_geometry(
                    &Operation::Profile(profile),
                    &self.controller.pending_operation_geometry(),
                )
                .ok()?;
            let Operation::Profile(drawing) = operation else {
                return None;
            };
            profile = drawing;
            binding = None;
        }
        let z = if let Some(binding) = binding {
            let geometry =
                crate::cad_operations::geometry_for_setup(self.controller.document()).ok()?;
            let resolved = cam_geometry3d::resolve_binding(&geometry, &binding).ok()?;
            let old_sides: Vec<_> = profile.contours().map(|c| c.side).collect();
            if let Some(first) = resolved.profiles.first() {
                profile.chain = first.chain.clone();
                profile.closed = first.closed;
                profile.side = if preserve_sides {
                    profile.side
                } else {
                    first.side
                };
                profile.additional_chains = resolved
                    .profiles
                    .iter()
                    .skip(1)
                    .cloned()
                    .enumerate()
                    .map(|(index, mut contour)| {
                        if preserve_sides {
                            if let Some(side) = old_sides.get(index + 1) {
                                contour.side = *side;
                            }
                        }
                        contour
                    })
                    .collect();
            } else {
                profile.chain = resolved.regions.first()?.outer().clone();
                profile.closed = true;
                profile.additional_chains.clear();
                if !preserve_sides {
                    if let cam_model::ExtractionRecipe::ProfileLoop { side } = binding.recipe {
                        profile.side = side;
                    }
                }
            }
            let reference = binding.selected.first()?;
            match reference.kind {
                EntityKind::Silhouette => 0.,
                EntityKind::Face | EntityKind::Edge => resolved.heights.top,
                EntityKind::Loop => {
                    let asset = geometry.asset(reference.asset_id)?;
                    let instance = geometry.instance(reference.instance_id)?;
                    let wire = asset.topology.loops.iter().find(|w| w.id == reference.id)?;
                    let edge_id = wire.edge_uses.first()?.edge_id;
                    let edge = asset.topology.edges.iter().find(|e| e.id == edge_id)?;
                    let point = match edge.curve {
                        cam_model::CadCurve::Line { start, .. } => start,
                        cam_model::CadCurve::CircleArc { center, .. } => center,
                        _ => return None,
                    };
                    instance.placement.transform_point(point)[2]
                }
                _ => return None,
            }
        } else {
            0.
        };
        self.apply_profile_side_overrides(&mut profile);
        Some((profile, z))
    }

    pub(super) fn set_profile_contour_side(&mut self, index: usize, side: Side) {
        if matches!(&self.operation_draft,Some(Operation::Profile(p)) if p.selections.is_some()) {
            self.set_profile_entry_contour_side(index, side);
            return;
        }
        let resolved = self.profile_selection_geometry().map(|p| p.0).or_else(|| {
            match self.operation_draft.as_ref()? {
                Operation::Profile(p) => Some(p.clone()),
                _ => None,
            }
        });
        let Some(mut profile) = resolved else {
            return;
        };
        let Some(mut contour) = profile.contours().nth(index) else {
            return;
        };
        contour.side = side;
        self.profile_side_overrides
            .retain(|old| old.chain != contour.chain || old.closed != contour.closed);
        self.profile_side_overrides.push(contour);
        profile.set_contour_side(index, side);
        self.profile_contour_selection = index;
        self.edit_operation_draft(|operation| {
            if let Operation::Profile(draft) = operation {
                draft.chain = profile.chain;
                draft.closed = profile.closed;
                draft.side = profile.side;
                draft.additional_chains = profile.additional_chains;
            }
        });
        self.status = format!(
            "Contour {}: {side}. Click its arrow to cycle Outside / Inside / On.",
            index + 1
        );
        self.refresh_operation_fields();
    }

    pub(super) fn profile_side_overlay(
        &self,
        camera: OrbitCamera,
        browser_width: f32,
    ) -> Option<SideOverlay> {
        if !self.operation_preview_active()
            || self.operation_tab != OperationTab::Geometry
            || self.profile_tab_picking
            || self.height_reference_pick.is_some()
        {
            return None;
        }
        let (profile, z) = self.profile_selection_geometry()?;
        Some(SideOverlay {
            camera,
            browser_width,
            z,
            frame: self.controller.document().setup().frame.clone(),
            radius: self.profile_tab_diameter(&profile).unwrap_or(0.) * 0.5 + profile.offset,
            contours: profile.contours().collect(),
            climb: profile.clearing.climb,
            reversed: (0..=profile.additional_chains.len())
                .map(|i| profile.contour_reversed(i))
                .collect(),
            selected: self.profile_contour_selection,
            gizmo_size: self.show_gizmo.then_some(self.gizmo_size),
        })
    }
}

pub(super) struct SideOverlay {
    camera: OrbitCamera,
    browser_width: f32,
    frame: cam_model::SetupFrame,
    z: f64,
    radius: f64,
    contours: Vec<ProfileContour>,
    climb: bool,
    reversed: Vec<bool>,
    selected: usize,
    gizmo_size: Option<f32>,
}
#[derive(Default)]
pub(super) struct SideState {
    armed: Option<usize>,
    hovered: Option<usize>,
}
#[derive(Clone, Copy)]
struct SideArrow {
    index: usize,
    anchor: iced::Point,
    tail: iced::Point,
    tip: iced::Point,
}

fn path_sample(contour: &ProfileContour, fraction: f64) -> Option<(cam_geo::Point, [f64; 2])> {
    let points = contour.chain.points();
    let count = if contour.closed {
        points.len()
    } else {
        points.len().saturating_sub(1)
    };
    let segments: Vec<_> = (0..count)
        .filter_map(|index| {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            let length = a.distance(b);
            (length > 1e-9 && length.is_finite()).then_some((a, b, length))
        })
        .collect();
    let mut remaining = segments.iter().map(|s| s.2).sum::<f64>() * fraction;
    for (a, b, length) in segments {
        if remaining <= length {
            let t = remaining / length;
            return Some((
                cam_geo::Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
                [(b.x - a.x) / length, (b.y - a.y) / length],
            ));
        }
        remaining -= length;
    }
    None
}

/// Unit normal points toward the cutter, independently of travel direction.
fn side_normal(contour: &ProfileContour, tangent: [f64; 2]) -> [f64; 2] {
    let left = [-tangent[1], tangent[0]];
    let sign = match contour.side {
        Side::On => 0.,
        Side::Outside if contour.closed && contour.chain.is_ccw() => -1.,
        Side::Inside if !contour.closed || !contour.chain.is_ccw() => -1.,
        _ => 1.,
    };
    left.map(|v| v * sign)
}

impl SideOverlay {
    fn project(&self, point: [f64; 2], bounds: iced::Rectangle) -> Option<iced::Point> {
        picking_view::project(
            self.camera,
            bounds,
            self.frame.to_world_vector([point[0], point[1], self.z]),
        )
        .filter(|(p, depth)| p.x.is_finite() && p.y.is_finite() && (0.0..=1.0).contains(depth))
        .map(|p| p.0)
    }
    fn available(&self, point: iced::Point, bounds: iced::Rectangle) -> bool {
        if point.x < self.browser_width + 20.
            || point.x > bounds.width - 20.
            || point.y < 20.
            || point.y > bounds.height - 20.
        {
            return false;
        }
        if let Some(size) = self.gizmo_size {
            let (x, y, size) = gizmo_rect(bounds.width, bounds.height, size, GIZMO_MARGIN);
            if point.x >= x - 18.
                && point.y >= y - 18.
                && point.x <= x + size + 18.
                && point.y <= y + size + 18.
            {
                return false;
            }
        }
        true
    }
    fn arrows(&self, bounds: iced::Rectangle) -> Vec<SideArrow> {
        let mut arrows = Vec::new();
        for (index, contour) in self.contours.iter().enumerate() {
            for fraction in [0.12, 0.38, 0.64, 0.86] {
                let Some((point, tangent)) = path_sample(contour, fraction) else {
                    continue;
                };
                let normal = side_normal(contour, tangent);
                let Some(anchor) = self.project([point.x, point.y], bounds) else {
                    continue;
                };
                let scale = f64::from(self.camera.world_per_pixel(bounds.height)) * 32.;
                let Some(along) = self.project(
                    [point.x + tangent[0] * scale, point.y + tangent[1] * scale],
                    bounds,
                ) else {
                    continue;
                };
                let mut direction = along - anchor;
                let length = direction.x.hypot(direction.y);
                if length < 1. {
                    continue;
                }
                direction = direction / length;
                let mut centre = anchor;
                if contour.side != Side::On {
                    let Some(beside) = self.project(
                        [point.x + normal[0] * scale, point.y + normal[1] * scale],
                        bounds,
                    ) else {
                        continue;
                    };
                    let offset = beside - anchor;
                    let length = offset.x.hypot(offset.y);
                    if length < 1. {
                        continue;
                    }
                    let Some(cutter) = self.project(
                        [
                            point.x + normal[0] * self.radius,
                            point.y + normal[1] * self.radius,
                        ],
                        bounds,
                    ) else {
                        continue;
                    };
                    let gap = cutter.distance(anchor).clamp(14., 28.);
                    centre = anchor + offset / length * gap;
                }
                // The application strategies use a clockwise spindle.
                let material_right =
                    self.climb ^ self.reversed.get(index).copied().unwrap_or(false);
                let reverse = if contour.closed {
                    let ccw = match contour.side {
                        Side::Inside => material_right,
                        Side::Outside => !material_right,
                        Side::On => contour.chain.is_ccw() == material_right,
                    };
                    contour.chain.is_ccw() != ccw
                } else {
                    match contour.side {
                        Side::Inside => material_right,
                        _ => !material_right,
                    }
                };
                if reverse {
                    direction = direction * -1.;
                }
                let arrow = SideArrow {
                    index,
                    anchor,
                    tail: centre - direction * 15.,
                    tip: centre + direction * 15.,
                };
                if !self.available(arrow.tail, bounds) || !self.available(arrow.tip, bounds) {
                    continue;
                }
                if arrows
                    .iter()
                    .any(|other: &SideArrow| other.tail.distance(arrow.tail) < 24.)
                {
                    continue;
                }
                arrows.push(arrow);
                break;
            }
        }
        arrows
    }
    fn hit(&self, point: iced::Point, bounds: iced::Rectangle) -> Option<usize> {
        if !self.available(point, bounds) {
            return None;
        }
        self.arrows(bounds)
            .into_iter()
            .filter_map(|arrow| {
                let vector = arrow.tip - arrow.tail;
                let length2 = vector.x * vector.x + vector.y * vector.y;
                let t = (((point.x - arrow.tail.x) * vector.x
                    + (point.y - arrow.tail.y) * vector.y)
                    / length2)
                    .clamp(0., 1.);
                let distance = point.distance(arrow.tail + vector * t);
                (distance <= 10.).then_some((arrow.index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
    }
}

impl canvas::Program<Message> for SideOverlay {
    type State = SideState;
    fn update(
        &self,
        state: &mut SideState,
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let local = iced::Rectangle::with_size(bounds.size());
        let hovered = cursor.position_in(bounds).and_then(|p| self.hit(p, local));
        match event {
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
                if hovered.is_some() =>
            {
                state.armed = hovered;
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                let armed = state.armed.take()?;
                if hovered == Some(armed) {
                    Some(
                        canvas::Action::publish(Message::ProfileContourSideCycle(armed))
                            .and_capture(),
                    )
                } else {
                    Some(canvas::Action::request_redraw().and_capture())
                }
            }
            canvas::Event::Mouse(
                iced::mouse::Event::CursorMoved { .. } | iced::mouse::Event::CursorLeft,
            ) => {
                if state.hovered != hovered {
                    state.hovered = hovered;
                    Some(canvas::Action::request_redraw())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn mouse_interaction(
        &self,
        _: &SideState,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if cursor
            .position_in(bounds)
            .and_then(|p| self.hit(p, iced::Rectangle::with_size(bounds.size())))
            .is_some()
        {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        _: &SideState,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let local = iced::Rectangle::with_size(bounds.size());
        let hovered = cursor.position_in(bounds).and_then(|p| self.hit(p, local));
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for contour in &self.contours {
            let points = contour.chain.points();
            let count = if contour.closed {
                points.len()
            } else {
                points.len().saturating_sub(1)
            };
            for index in 0..count {
                if let (Some(a), Some(b)) = (
                    self.project([points[index].x, points[index].y], local),
                    self.project(
                        [
                            points[(index + 1) % points.len()].x,
                            points[(index + 1) % points.len()].y,
                        ],
                        local,
                    ),
                ) {
                    frame.stroke(
                        &canvas::Path::line(a, b),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb(0.2, 0.6, 0.98))
                            .with_width(1.5),
                    );
                }
            }
        }
        for arrow in self.arrows(local) {
            let color = if hovered == Some(arrow.index) {
                Color::from_rgb(1., 0.78, 0.22)
            } else {
                Color::from_rgb(1., 0.24, 0.31)
            };
            let direction = arrow.tip - arrow.tail;
            let direction = direction / direction.x.hypot(direction.y);
            let wing = iced::Vector::new(-direction.y, direction.x);
            let centre = arrow.tail + direction * 15.;
            frame.stroke(
                &canvas::Path::line(arrow.anchor, centre),
                canvas::Stroke::default()
                    .with_width(1.5)
                    .with_color(Color { a: 0.65, ..color }),
            );
            frame.fill(&canvas::Path::circle(arrow.anchor, 3.), color);
            let mut path = canvas::path::Builder::new();
            path.move_to(arrow.tail);
            path.line_to(arrow.tip);
            path.move_to(arrow.tip - direction * 10. + wing * 6.);
            path.line_to(arrow.tip);
            path.line_to(arrow.tip - direction * 10. - wing * 6.);
            let path = path.build();
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.08, 0.08, 0.1, 0.8))
                    .with_width(7.),
            );
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(if self.selected == arrow.index { 4. } else { 3. }),
            );
            frame.fill_text(canvas::Text {
                content: (arrow.index + 1).to_string(),
                position: centre + wing * 10.,
                color,
                size: 12.0.into(),
                ..Default::default()
            });
            if hovered == Some(arrow.index) {
                let text = format!(
                    "Contour {}: {} · Click to change side",
                    arrow.index + 1,
                    self.contours[arrow.index].side
                );
                let width = text.len() as f32 * 6.5 + 16.;
                let position = iced::Point::new(
                    (centre.x + 18.).clamp(
                        self.browser_width + 4.,
                        (bounds.width - width - 4.).max(self.browser_width + 4.),
                    ),
                    (centre.y + 22.).min(bounds.height - 28.),
                );
                frame.fill_rectangle(
                    position,
                    iced::Size::new(width, 26.),
                    Color::from_rgba(0.10, 0.12, 0.15, 0.97),
                );
                frame.fill_text(canvas::Text {
                    content: text,
                    position: position + iced::Vector::new(8., 5.),
                    color: Color::WHITE,
                    size: 12.0.into(),
                    ..Default::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;

    fn square(side: Side) -> ProfileContour {
        ProfileContour {
            chain: cam_geo::Contour::new(vec![
                cam_geo::Point::new(0., 0.),
                cam_geo::Point::new(40., 0.),
                cam_geo::Point::new(40., 40.),
                cam_geo::Point::new(0., 40.),
            ]),
            closed: true,
            side,
        }
    }
    fn overlay(side: Side) -> SideOverlay {
        SideOverlay {
            camera: OrbitCamera::framed([-10., -10., -5.], [50., 50., 5.]),
            browser_width: 0.,
            frame: Default::default(),
            z: 0.,
            radius: 3.,
            contours: vec![square(side)],
            climb: true,
            reversed: vec![false],
            selected: 0,
            gizmo_size: None,
        }
    }
    #[test]
    fn arrow_moves_to_each_cutting_side_and_captures_only_its_own_click() {
        let bounds = iced::Rectangle::with_size(iced::Size::new(900., 700.));
        let outside = overlay(Side::Outside);
        let inside = overlay(Side::Inside);
        let on = overlay(Side::On);
        let a = outside.arrows(bounds)[0];
        let b = inside.arrows(bounds)[0];
        let c = on.arrows(bounds)[0];
        let middle = |arrow: SideArrow| arrow.tail + (arrow.tip - arrow.tail) * 0.5;
        assert!(
            middle(a).y > a.anchor.y,
            "CCW bottom edge: Outside is below the boundary"
        );
        assert!(middle(b).y < b.anchor.y, "Inside is above the boundary");
        assert!(middle(c).distance(c.anchor) < 1e-4);
        let mut state = SideState::default();
        let cursor = iced::mouse::Cursor::Available(middle(a));
        let press =
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let release = canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ));
        assert_eq!(
            outside
                .update(&mut state, &press, bounds, cursor)
                .unwrap()
                .into_inner()
                .2,
            iced::event::Status::Captured
        );
        let (message, _, status) = outside
            .update(&mut state, &release, bounds, cursor)
            .unwrap()
            .into_inner();
        assert!(matches!(message, Some(Message::ProfileContourSideCycle(0))));
        assert_eq!(status, iced::event::Status::Captured);
        let empty = iced::mouse::Cursor::Available(iced::Point::new(850., 650.));
        assert!(outside.update(&mut state, &press, bounds, empty).is_none());
        assert!(outside
            .update(&mut state, &release, bounds, empty)
            .is_none());
        let _ = outside.update(&mut state, &press, bounds, cursor);
        assert!(
            outside
                .update(&mut state, &release, bounds, empty)
                .unwrap()
                .into_inner()
                .0
                .is_none(),
            "a drag away must not change the side"
        );
    }
    #[test]
    fn marker_side_is_independent_of_closed_winding_and_open_chains_do_not_close() {
        let mut contour = square(Side::Outside);
        let normal = side_normal(&contour, [1., 0.]);
        contour.chain.reverse();
        assert_eq!(side_normal(&contour, [-1., 0.]), normal);
        contour.closed = false;
        contour.chain = cam_geo::Contour::new(vec![
            cam_geo::Point::new(0., 0.),
            cam_geo::Point::new(10., 0.),
        ]);
        assert_eq!(side_normal(&contour, [1., 0.]), [0., 1.]);
        contour.side = Side::Inside;
        assert_eq!(side_normal(&contour, [1., 0.]), [0., -1.]);
        assert_eq!(
            path_sample(&contour, 0.9).unwrap().0,
            cam_geo::Point::new(9., 0.)
        );
        let mut hidden = overlay(Side::Outside);
        hidden.browser_width = 900.;
        assert!(hidden
            .arrows(iced::Rectangle::with_size(iced::Size::new(900., 700.)))
            .is_empty());
    }
    fn drawing_app() -> App {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        app.setup_draft = None;
        app.operation_draft = None;
        app.controller.select(Selection::Setup);
        app.refresh_fields();
        app
    }
    #[test]
    fn each_feature_retains_its_side_during_creation_and_when_other_features_are_removed() {
        let mut app = drawing_app();
        let before = app.controller.document().clone();
        let _ = app.update(Message::BeginOp(OpKind::Profile));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Geometry));
        app.controller
            .set_pending_operation_geometry(&[LoopRef::outer(0), LoopRef::hole(0, 0)], None)
            .unwrap();
        app.set_profile_contour_side(1, Side::On);
        let (Operation::Profile(profile), _) = app.creation_operation().unwrap() else {
            panic!("profile")
        };
        assert_eq!(profile.side, Side::Outside);
        assert_eq!(profile.additional_chains[0].side, Side::On);
        app.controller
            .set_pending_operation_geometry(&[LoopRef::hole(0, 0)], None)
            .unwrap();
        let (Operation::Profile(profile), _) = app.creation_operation().unwrap() else {
            panic!("profile")
        };
        assert_eq!(
            profile.side,
            Side::On,
            "side follows geometry rather than a shifting row index"
        );
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::CancelOp);
        assert_eq!(app.controller.document(), &before);
    }
    #[test]
    fn per_contour_choices_preview_commit_cancel_save_and_undo_independently() {
        let mut app = drawing_app();
        let mut op = app.controller.operation_template(OpKind::Profile, 1);
        let Operation::Profile(profile) = &mut op else {
            unreachable!()
        };
        profile.chain = app.controller.regions()[0].outer().clone();
        profile.additional_chains = vec![ProfileContour {
            chain: app.controller.regions()[0].holes()[0].clone(),
            closed: true,
            side: Side::Inside,
        }];
        app.controller.add_operation(op);
        let id = app
            .controller
            .document()
            .setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        app.operation_tab = OperationTab::Geometry;
        let before = app.controller.document().clone();
        let _ = app.update(Message::ProfileContourSideCycle(1));
        let (profile, _) = app.profile_tab_geometry().unwrap();
        assert_eq!(profile.side, Side::Outside);
        assert_eq!(profile.additional_chains[0].side, Side::On);
        app.set_profile_contour_side(0, Side::Inside);
        let _ = app.update(Message::Apply);
        let Some(Operation::Profile(saved)) = app.controller.operation(id) else {
            panic!("profile")
        };
        assert_eq!(saved.side, Side::Inside);
        assert_eq!(saved.additional_chains[0].side, Side::On);
        let path = std::env::temp_dir().join(format!("profile-side-{}.ocam", std::process::id()));
        app.controller.save_project(&path).unwrap();
        let mut reopened = AppController::new(crate::default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(reopened.operation(id), app.controller.operation(id));
        assert!(app.controller.undo());
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    #[ignore = "requires local testfiles/test_project.ocam"]
    fn saved_test_project_silhouette_accepts_inside_and_on_without_adapter_errors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testfiles/test_project.ocam");
        let (mut app, _) = App::new();
        app.controller.open_project(&path).unwrap();
        let id = 2;
        let binding = &app.controller.geometry_state().bindings[&id];
        assert_eq!(binding.selected[0].kind, EntityKind::Silhouette);
        app.controller.set_operation_excluded(0, true);
        app.controller.set_operation_excluded(1, true);
        for side in [Side::Inside, Side::On, Side::Outside] {
            app.begin_operation_draft(id);
            app.operation_tab = OperationTab::Geometry;
            let _ = app.update(Message::SideChanged(side));
            assert_eq!(app.profile_tab_geometry().unwrap().0.side, side);
            let _ = app.update(Message::Apply);
            assert!(
                matches!(app.controller.operation(id),Some(Operation::Profile(p)) if p.side==side),
                "{}",
                app.status
            );
            let result = app.controller.run(&cam_toolpath::CancelToken::new());
            assert!(
                !result
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains("certified CAD adapter")),
                "{:?}",
                result.diagnostics
            );
            assert_eq!(
                result.completion,
                cam_toolpath::JobCompletion::Complete,
                "{:?}",
                result.diagnostics
            );
        }
        use super::super::profile_entries_view::EntryMessage;
        use cam_model::ProfileSideRule;
        app.begin_operation_draft(id);
        let previous = app.operation_draft.clone();
        app.profile_entry_message(EntryMessage::EditSelected);
        app.profile_entry_message(EntryMessage::Name("Discard this edit".into()));
        app.profile_entry_message(EntryMessage::Cancel);
        assert_eq!(app.operation_draft, previous);
        for (rule, side) in [
            (ProfileSideRule::AlwaysInside, Side::Inside),
            (ProfileSideRule::On, Side::On),
        ] {
            app.begin_operation_draft(id);
            app.operation_tab = OperationTab::Geometry;
            app.geometry_row_selection = Some(0);
            app.profile_entry_message(EntryMessage::EditSelected);
            app.profile_entry_message(EntryMessage::Side(rule));
            app.profile_entry_message(EntryMessage::Confirm);
            assert!(app.profile_entry_editor.is_none(), "{}", app.status);
            let _ = app.update(Message::Apply);
            assert!(
                matches!(app.controller.operation(id), Some(Operation::Profile(p)) if p.contours().all(|c|c.side == side)),
                "{}",
                app.status
            );
            let result = app.controller.run(&cam_toolpath::CancelToken::new());
            assert_eq!(
                result.completion,
                cam_toolpath::JobCompletion::Complete,
                "{:?}",
                result.diagnostics
            );
        }
    }
}
