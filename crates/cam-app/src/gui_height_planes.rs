//! Operation height preview: machining-plane rectangles and labels.
use super::*;
use cam_model::{HeightPlaneKind as Plane, RotaryAxis, RotaryIndex};
use canvas::path::lyon_path::math::Transform;
use iced::advanced::text::Paragraph as _;

pub(super) fn field_id(field: Field) -> String {
    format!("height-{field:?}")
}

#[derive(Clone, Debug)]
pub(super) struct HeightPlane {
    pub name: &'static str,
    pub z: f64,
    pub value_label: String,
    pub rotary_angle: Option<(RotaryAxis, f64)>,
    pub color: Color,
    pub active: bool,
    pub corners: [[f32; 3]; 4],
    pub outlines: Vec<Vec<[f32; 3]>>,
}

pub(super) struct HeightOverlay {
    pub camera: OrbitCamera,
    pub planes: Vec<HeightPlane>,
    pub browser_width: f32,
}

#[derive(PartialEq)]
struct IndexedHeightPreviewKey {
    revision: u64,
    setup_id: u64,
    operation: Operation,
    index: RotaryIndex,
    settings: Option<cam_model::OperationHeightSettings>,
    tools: Vec<cam_model::Tool>,
}

pub(super) struct CachedIndexedHeightPreview {
    key: IndexedHeightPreviewKey,
    planes: Vec<IndexedHeightPlane>,
}

impl App {
    pub(super) fn preview_safety_heights(&self, id: u32) -> cam_model::Heights {
        let saved = self.controller.inspection_setup().heights_for(id);
        let value = |field, fallback| {
            self.fields
                .get(&field)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(fallback)
        };
        cam_model::Heights::new(
            value(Field::Clearance, saved.clearance),
            value(Field::Retract, saved.retract),
            value(Field::TopOfStock, saved.top_of_stock),
        )
    }

    pub(super) fn height_input_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .inspector_fields()
            .into_iter()
            .filter(|f| operation_tabs::field_tab(*f) == OperationTab::Heights)
            .map(field_id)
            .collect();
        ids.extend(["height-cad-top".into(), "height-cad-bottom".into()]);
        ids.extend(
            cam_model::HeightPlaneKind::ALL
                .into_iter()
                .map(super::height_editor::plane_input_id),
        );
        ids
    }

    pub(super) fn height_preview(
        &self,
        camera: OrbitCamera,
        browser_width: f32,
    ) -> Option<HeightOverlay> {
        let focused = self
            .height_reference_pick
            .map(super::height_editor::plane_input_id)
            .or_else(|| self.focused_height.clone());
        if self.workspace != Workspace::Machining
            || self.operation_tab != OperationTab::Heights
            || !self.inspector_enabled
            || !self.inspector_active
            || self.cad_editing_op.is_some()
            || self.drawing_editing_op.is_some()
        {
            return None;
        }
        let id = if self.creating_operation() {
            0
        } else if let Selection::Operation(id) = self.controller.selection() {
            id
        } else {
            return None;
        };
        let setup = self.controller.inspection_setup();
        let colors = [
            Color::from_rgb(1., 0.38, 0.16),
            Color::from_rgb(0.87, 0.75, 0.16),
            Color::from_rgb(0.25, 0.94, 0.35),
            Color::from_rgb(0.12, 0.78, 1.),
            Color::from_rgb(0.77, 0.52, 1.),
        ];
        let names = ["Clearance", "Retract", "Feed", "Top", "Bottom"];
        let make_plane = |plane: Plane, z, rotary_angle, corners, outlines| HeightPlane {
            name: names[plane.index()],
            z,
            value_label: self.height_display_value(id, plane, z),
            rotary_angle,
            color: colors[plane.index()],
            active: focused.as_deref()
                == Some(super::height_editor::plane_input_id(plane).as_str()),
            corners,
            outlines,
        };
        let index = self.rotary_index_draft.as_ref();
        let planes = if let Some(index) = index.filter(|index| index.uses_automatic_cad()) {
            let mut fixed = RotaryIndex {
                axis: index.axis,
                center: index.center,
                ..RotaryIndex::default()
            };
            self.automatic_height_planes(id, index)
                .ok()?
                .into_iter()
                .map(|plane| {
                    fixed.angle = plane.angle;
                    make_plane(
                        plane.kind,
                        plane.z,
                        Some((fixed.axis, fixed.angle)),
                        plane_corners(&setup.frame, Some(&fixed), plane.xy, plane.z),
                        Vec::new(),
                    )
                })
                .collect()
        } else {
            let heights = self.resolve_height_editor(id).ok()?;
            let document = self.rotary_draft_document();
            let indexed = document.indexed_view(id).ok()?;
            let (min, max) = indexed.as_ref().map_or_else(
                || self.controller.stock_box(),
                |view| view.stock_bounds_for(view.setup()),
            );
            let mut operation = self.current_operation(id)?.clone();
            let parsed = self.parsed_operation_fields();
            apply_op_fields(&mut operation, &parsed);
            if operation.rotary_wrap().is_some() {
                let geometry =
                    crate::cad_operations::geometry_for_setup(self.controller.document()).ok()?;
                crate::cad_operations::resolve_wrapped_operation(&geometry, &mut operation).ok()?;
            }
            let xy = padded_bounds([min[0], min[1]], [max[0], max[1]]);
            Plane::ALL
                .into_iter()
                .filter_map(|plane| {
                    let z = heights.plane(plane)?;
                    if operation
                        .rotary_wrap()
                        .is_some_and(|w| w.effective_radius() + z - w.surface_z <= 0.)
                    {
                        return None;
                    }
                    let outlines = operation.rotary_wrap().map_or_else(Vec::new, |wrap| {
                        wrapped_plane_outlines(wrap, z, min, max, &setup.frame)
                    });
                    Some(make_plane(
                        plane,
                        z,
                        None,
                        plane_corners(&setup.frame, index, xy, z),
                        outlines,
                    ))
                })
                .collect()
        };
        Some(HeightOverlay {
            camera,
            planes,
            browser_width,
        })
    }

    fn automatic_height_planes(
        &self,
        id: u32,
        index: &RotaryIndex,
    ) -> Result<Vec<IndexedHeightPlane>, String> {
        let (mut document, mut operation) = self.height_editor_input(id)?;
        if !self.creating_operation() {
            self.apply_parameter_sources(&mut operation)?;
        }
        let settings = (self.creating_operation()
            || document.setup().height_settings.contains_key(&id)
            || self.height_rules_dirty())
        .then(|| self.edited_height_settings())
        .transpose()?;
        let key = IndexedHeightPreviewKey {
            revision: self.controller.revision(),
            setup_id: document.setup().id,
            operation,
            index: index.clone(),
            settings,
            tools: self.operation_tools.clone(),
        };
        let mut cache = self.indexed_height_preview.borrow_mut();
        if let Some(cached) = cache.as_ref().filter(|cached| cached.key == key) {
            return Ok(cached.planes.clone());
        }
        if let Some(settings) = &key.settings {
            document
                .setup_mut()
                .height_settings
                .insert(id, settings.clone());
        }
        let geometry = crate::cad_operations::geometry_for_setup(&document)?;
        let cuts = crate::cad_operations::automatic_index::resolve(
            &document,
            &geometry,
            &key.operation,
            index,
            index.auto.as_ref().unwrap(),
            &|| false,
        )?;
        let mut planes = Vec::new();
        for cut in cuts {
            let xy = padded_bounds(cut.stock_xy.0, cut.stock_xy.1);
            for kind in Plane::ALL {
                if let Some(z) = cut.heights.plane(kind) {
                    planes.push(IndexedHeightPlane {
                        kind,
                        z,
                        angle: cut.angle,
                        xy,
                    });
                }
            }
        }
        planes.sort_by(|a, b| {
            a.angle
                .total_cmp(&b.angle)
                .then(a.kind.cmp(&b.kind))
                .then(a.z.total_cmp(&b.z))
        });
        planes.dedup_by(|a, b| a.angle == b.angle && a.kind == b.kind && (a.z - b.z).abs() < 1e-6);
        separate_indexed_planes(&mut planes, index);
        *cache = Some(CachedIndexedHeightPreview {
            key,
            planes: planes.clone(),
        });
        Ok(planes)
    }
}

fn padded_bounds(min: [f64; 2], max: [f64; 2]) -> [[f64; 2]; 2] {
    let pad = (max[0] - min[0]).max(max[1] - min[1]).max(1.) * 0.08;
    [min.map(|v| v - pad), max.map(|v| v + pad)]
}

fn plane_corners(
    frame: &cam_model::SetupFrame,
    index: Option<&RotaryIndex>,
    [min, max]: [[f64; 2]; 2],
    z: f64,
) -> [[f32; 3]; 4] {
    [
        [min[0], min[1], z],
        [max[0], min[1], z],
        [max[0], max[1], z],
        [min[0], max[1], z],
    ]
    .map(|p| rotary_view::index_point_to_world(frame, index, p).map(|v| v as f32))
}

#[derive(Clone)]
struct IndexedHeightPlane {
    kind: Plane,
    z: f64,
    angle: f64,
    xy: [[f64; 2]; 2],
}

fn separate_indexed_planes(planes: &mut [IndexedHeightPlane], index: &RotaryIndex) {
    let (axial, transverse, sign) = match index.axis {
        RotaryAxis::A => (0, 1, -1.),
        RotaryAxis::B => (1, 0, 1.),
    };
    let center = index.center[transverse];
    let gap = |width: f64| (width * 0.05).min(0.25);
    let trim = |plane: &mut IndexedHeightPlane, crossing: f64| {
        let left = crossing - plane.xy[0][transverse];
        let right = plane.xy[1][transverse] - crossing;
        if left >= right {
            plane.xy[1][transverse] = crossing - gap(left);
        } else {
            plane.xy[0][transverse] = crossing + gap(right);
        }
    };
    for i in 0..planes.len() {
        let (previous, rest) = planes.split_at_mut(i + 1);
        let plane = &mut previous[i];
        for other in rest {
            let axial_min = plane.xy[0][axial].max(other.xy[0][axial]);
            let axial_max = plane.xy[1][axial].min(other.xy[1][axial]);
            if plane.angle == other.angle || axial_min > axial_max {
                continue;
            }
            let (sin, cos) = (sign * (plane.angle - other.angle).to_radians()).sin_cos();
            let z = plane.z - index.center[2];
            let other_z = other.z - index.center[2];
            if sin.abs() < 1e-12 {
                // At a half turn two different heights can describe the same
                // physical plane. Give each a section along the shared axis.
                let [a, b] = plane.xy.map(|p| center + (p[transverse] - center) * cos);
                if (other_z - z * cos).abs() < 1e-6
                    && a.min(b) <= other.xy[1][transverse]
                    && a.max(b) >= other.xy[0][transverse]
                {
                    let mid = axial_min.midpoint(axial_max);
                    let space = gap(axial_max - axial_min);
                    plane.xy[1][axial] = mid - space;
                    other.xy[0][axial] = mid + space;
                }
                continue;
            }
            // All intersections run along the rotary axis. Find the transverse
            // coordinate of the intersection in each rectangle's own frame.
            let crossing = (other_z - z * cos) / sin;
            let other_crossing = center + crossing * cos - z * sin;
            let crossing = center + crossing;
            if (plane.xy[0][transverse]..=plane.xy[1][transverse]).contains(&crossing)
                && (other.xy[0][transverse]..=other.xy[1][transverse]).contains(&other_crossing)
            {
                trim(plane, crossing);
                trim(other, other_crossing);
            }
        }
    }
}

fn wrapped_plane_outlines(
    wrap: &cam_model::RotaryWrap,
    z: f64,
    min: [f64; 3],
    max: [f64; 3],
    frame: &cam_model::SetupFrame,
) -> Vec<Vec<[f32; 3]>> {
    let radius = wrap.effective_radius() + z - wrap.surface_z;
    if !radius.is_finite() || radius <= 0. {
        return Vec::new();
    }
    let axial = match wrap.axis {
        cam_model::RotaryAxis::A => 0,
        cam_model::RotaryAxis::B => 1,
    };
    let pad = (max[axial] - min[axial]).max(1.) * 0.08;
    let ends = [min[axial] - pad, max[axial] + pad];
    let point = |along: f64, angle: f64| {
        let (s, c) = angle.sin_cos();
        let p = match wrap.axis {
            cam_model::RotaryAxis::A => [
                along,
                wrap.center[1] + radius * s,
                wrap.center[2] + radius * c,
            ],
            cam_model::RotaryAxis::B => [
                wrap.center[0] + radius * s,
                along,
                wrap.center[2] + radius * c,
            ],
        };
        frame.to_world_vector(p).map(|v| v as f32)
    };
    let mut paths: Vec<_> = ends
        .into_iter()
        .map(|along| {
            (0..=128)
                .map(|i| point(along, std::f64::consts::TAU * i as f64 / 128.))
                .collect()
        })
        .collect();
    paths.extend((0..4).map(|i| {
        ends.into_iter()
            .map(|along| point(along, std::f64::consts::FRAC_PI_2 * i as f64))
            .collect()
    }));
    paths
}

fn project(matrix: &[[f32; 4]; 4], point: [f32; 3], size: iced::Size) -> iced::Point {
    let clip: [f32; 4] = std::array::from_fn(|r| {
        matrix[0][r] * point[0] + matrix[1][r] * point[1] + matrix[2][r] * point[2] + matrix[3][r]
    });
    iced::Point::new(
        (clip[0] / clip[3] + 1.) * size.width / 2.,
        (1. - clip[1] / clip[3]) * size.height / 2.,
    )
}

fn label_text(plane: &HeightPlane) -> canvas::Text {
    let mut content = format!("{}: {} mm", plane.name, plane.value_label);
    if let Some((axis, angle)) = plane.rotary_angle {
        let angle = fmt_num((angle * 1000.).round() / 1000.);
        content = format!("{}{angle}° · {content}", axis.word());
    }
    canvas::Text {
        content,
        position: iced::Point::new(6., 2.),
        size: 14.0.into(),
        color: Color::from_rgb(0.05, 0.07, 0.09),
        ..Default::default()
    }
}

fn label_size(text: &canvas::Text) -> iced::Size {
    let layout = iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
        content: text.content.as_str(),
        bounds: iced::Size::INFINITE,
        size: text.size,
        line_height: text.line_height,
        font: text.font,
        align_x: text.align_x,
        align_y: text.align_y,
        shaping: text.shaping,
        wrapping: Default::default(),
    });
    layout.min_bounds() + iced::Size::new(12., 4.)
}

fn cross_2d(a: iced::Vector, b: iced::Vector) -> f32 {
    a.x * b.y - a.y * b.x
}

fn planar_label_placement(
    corners: [iced::Point; 4],
    edge_lengths: [f32; 2],
    mm_per_pixel: f32,
    size: iced::Size,
    visible: iced::Rectangle,
    labels: &[iced::Rectangle],
) -> Option<(Transform, iced::Rectangle)> {
    let mut best = None;
    for i in 0..4 {
        let edge_length = edge_lengths[i % 2] / mm_per_pixel;
        let side_length = edge_lengths[(i + 1) % 2] / mm_per_pixel;
        if edge_length <= 0. || side_length <= 0. {
            continue;
        }
        let (mut start, end) = (corners[i], corners[(i + 1) % 4]);
        let mut x = (end - start) * (1. / edge_length);
        let outside = (start - corners[(i + 3) % 4]) * (1. / side_length);
        if x.x < 0. || (x.x == 0. && x.y > 0.) {
            start = end;
            x = x * -1.;
        }
        // Keep the text readable from either side of the plane. The label
        // extends outward from the edge whether its top or bottom touches it.
        let handedness = cross_2d(x, outside);
        let y = outside * handedness.signum();
        let scale = (edge_length / size.width).clamp(0.75, 1.);
        if handedness.abs() * size.height * scale < 1. {
            continue;
        }
        let width = x * (size.width * scale);
        let height = y * (size.height * scale);
        if handedness < 0. {
            start = start - height;
        }
        let slack = edge_length - size.width * scale;
        for slot in [0.5, 0., 1.] {
            let origin = start + x * (slack.max(0.) * slot + slack.min(0.) / 2.);
            let rect = iced::Rectangle {
                x: origin.x + width.x.min(0.) + height.x.min(0.),
                y: origin.y + width.y.min(0.) + height.y.min(0.),
                width: width.x.abs() + height.x.abs(),
                height: width.y.abs() + height.y.abs(),
            };
            let clipped = if rect.is_within(&visible) {
                0.
            } else {
                1. - rect.intersection(&visible).map_or(0., |r| r.area()) / rect.area()
            };
            let overlap = labels
                .iter()
                .filter_map(|label| rect.intersection(label))
                .map(|r| r.area())
                .sum::<f32>();
            let score = [clipped, overlap, 1. - scale, 1. - x.x.abs()];
            if best.as_ref().is_none_or(|(current, _, _)| score < *current) {
                let transform = Transform::new(
                    x.x * scale,
                    x.y * scale,
                    y.x * scale,
                    y.y * scale,
                    origin.x,
                    origin.y,
                );
                best = Some((score, transform, rect));
            }
        }
    }
    best.map(|(_, transform, rect)| (transform, rect))
}

impl canvas::Program<Message> for HeightOverlay {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let matrix = self.camera.view_proj(size.width / size.height.max(1.));
        let project = |point| project(&matrix, point, size);
        let mut frame = canvas::Frame::new(renderer, size);
        let left = if self.browser_width > 0. {
            self.browser_width + 16.
        } else {
            8.
        };
        // Finish every outline before painting labels so another plane's edge
        // cannot run through a label (especially at coincident top planes).
        for plane in &self.planes {
            if !plane.z.is_finite() {
                continue;
            }
            let outlines = if plane.outlines.is_empty() {
                vec![plane
                    .corners
                    .into_iter()
                    .chain(std::iter::once(plane.corners[0]))
                    .collect()]
            } else {
                plane.outlines.clone()
            };
            for outline in outlines {
                let points: Vec<_> = outline.into_iter().map(project).collect();
                if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                    continue;
                }
                let path = canvas::Path::new(|p| {
                    p.move_to(points[0]);
                    for point in &points[1..] {
                        p.line_to(*point);
                    }
                });
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(plane.color)
                        .with_width(if plane.active { 2.5 } else { 1.2 }),
                );
            }
        }
        let visible = iced::Rectangle {
            x: left,
            y: 8.,
            width: (size.width - left - 8.).max(0.),
            height: (size.height - 16.).max(0.),
        };
        let mm_per_pixel = 2. * self.camera.half_height() / size.width.min(size.height).max(1.);
        let mut labels: Vec<iced::Rectangle> = Vec::new();
        let mut captions = Vec::new();
        for (index, plane) in self.planes.iter().enumerate() {
            let text = label_text(plane);
            let Some(ring) = plane.outlines.first() else {
                let corners = plane.corners.map(project);
                if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                    continue;
                }
                let lengths = [1, 3].map(|end| {
                    (0..3)
                        .map(|i| (plane.corners[end][i] - plane.corners[0][i]).powi(2))
                        .sum::<f32>()
                        .sqrt()
                });
                let size = label_size(&text);
                if let Some((transform, rect)) =
                    planar_label_placement(corners, lengths, mm_per_pixel, size, visible, &labels)
                {
                    labels.push(rect);
                    let plate = canvas::Path::rectangle(iced::Point::ORIGIN, size);
                    frame.fill(&plate.transform(&transform), plane.color);
                    // Project the glyph outlines with both in-plane axes so
                    // their shear and foreshortening match the plane exactly.
                    text.draw_with(|glyph, color| frame.fill(&glyph.transform(&transform), color));
                }
                continue;
            };
            let points: Vec<_> = ring.iter().copied().map(project).collect();
            if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                continue;
            }
            // Wrap heights describe cylinders, so their callouts follow the end ring.
            let t = (index as f32 + 0.5) / self.planes.len().max(1) as f32;
            let anchor = points[((points.len() - 1) as f32 * t) as usize];
            let width = text.content.chars().count() as f32 * 6.8 + 16.;
            let mut rect = iced::Rectangle {
                x: anchor.x.clamp(
                    left.min((bounds.width - width - 8.).max(0.)),
                    (bounds.width - width - 8.).max(left),
                ),
                y: (anchor.y - 23.).clamp(8., (bounds.height - 30.).max(8.)),
                width,
                height: 22.,
            };
            for _ in 0..self.planes.len() {
                if !labels.iter().any(|r| r.intersects(&rect)) {
                    break;
                }
                rect.y += 25.;
                if rect.y + rect.height > bounds.height - 8. {
                    rect.y = 8.;
                }
            }
            labels.push(rect);
            let target = iced::Point::new(
                anchor.x.clamp(rect.x, rect.x + rect.width),
                anchor.y.clamp(rect.y, rect.y + rect.height),
            );
            frame.stroke(
                &canvas::Path::line(anchor, target),
                canvas::Stroke::default()
                    .with_color(plane.color)
                    .with_width(1.),
            );
            captions.push((plane, text, rect));
        }
        for (plane, text, rect) in captions {
            frame.fill_rectangle(rect.position(), rect.size(), plane.color);
            frame.fill_text(canvas::Text {
                position: rect.position() + iced::Vector::new(8., 3.),
                size: 12.0.into(),
                ..text
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planar_labels_follow_the_projected_edges_from_above_and_below() {
        let world = [
            [-40., -30., 0.],
            [40., -30., 0.],
            [40., 30., 0.],
            [-40., 30., 0.],
        ];
        let mut camera = OrbitCamera::framed(world[0], world[2]);
        let viewport = iced::Size::new(800., 600.);
        let label = iced::Size::new(112., 22.);
        let visible = iced::Rectangle::with_size(viewport);
        let mm_per_pixel = camera.world_per_pixel(viewport.height);
        for pitch in [0.9, 2.1] {
            camera.orient = cam_render::orientation(0.55, pitch);
            let matrix = camera.view_proj(viewport.width / viewport.height);
            let corners = world.map(|p| project(&matrix, p, viewport));
            let (transform, _) =
                planar_label_placement(corners, [80., 60.], mm_per_pixel, label, visible, &[])
                    .unwrap();
            let x = iced::Vector::new(transform.m11, transform.m12);
            let y = iced::Vector::new(transform.m21, transform.m22);
            assert!(cross_2d(x, y) > 0., "the text must read from either side");
            assert!(
                (x.x * y.x + x.y * y.y).abs() > 0.1,
                "the glyphs must shear with the plane"
            );
            let origin = iced::Point::new(transform.m31, transform.m32);
            assert!((0..4).any(|i| {
                let edge = corners[(i + 1) % 4] - corners[i];
                let interior = corners[(i + 3) % 4] - corners[i];
                let top = cross_2d(edge, origin - corners[i]);
                let bottom = cross_2d(edge, origin + y * label.height - corners[i]);
                cross_2d(edge, x).abs() < 1e-3
                    && cross_2d(interior, y).abs() < 1e-3
                    && top.abs().min(bottom.abs()) < 0.02
                    && (top + bottom) * cross_2d(edge, interior) < 0.
            }));
        }

        camera.orient = cam_render::orientation(0.55, std::f32::consts::FRAC_PI_2);
        let matrix = camera.view_proj(viewport.width / viewport.height);
        let corners = world.map(|p| project(&matrix, p, viewport));
        assert!(
            planar_label_placement(corners, [80., 60.], mm_per_pixel, label, visible, &[])
                .is_none()
        );
    }

    fn operation_editor() -> (App, u32) {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller.open_dxf(SAMPLE_DXF, "sample.dxf").unwrap();
        // Height expectations use the drawing surface as stock top.
        app.controller.edit_stock(|s| {
            *s = cam_model::Stock::RelativeBox {
                offsets: [[1., 1., 0.]; 2],
            }
        });
        app.controller.new_operation(OpKind::Profile);
        let id = app
            .controller
            .inspection_setup()
            .operations
            .last()
            .unwrap()
            .id();
        app.begin_operation_draft(id);
        let _ = app.update(Message::OperationTabChanged(OperationTab::Heights));
        (app, id)
    }

    #[test]
    fn indexed_rectangles_clear_crossing_and_coplanar_planes_on_both_axes() {
        for axis in [RotaryAxis::A, RotaryAxis::B] {
            let index = RotaryIndex {
                axis,
                center: [2., 3., 4.],
                ..Default::default()
            };
            let (axial, transverse) = if axis == RotaryAxis::A {
                (0, 1)
            } else {
                (1, 0)
            };
            let bounds = [
                [index.center[0] - 10., index.center[1] - 10.],
                [index.center[0] + 10., index.center[1] + 10.],
            ];
            let plane = |angle, z| IndexedHeightPlane {
                kind: Plane::Top,
                z,
                angle,
                xy: bounds,
            };

            let mut crossing = [plane(0., 4.), plane(90., 4.), plane(90., 40.)];
            separate_indexed_planes(&mut crossing, &index);
            for plane in &crossing[..2] {
                assert!(plane.xy[1][transverse] < index.center[transverse]);
                assert_eq!(plane.xy[0], bounds[0]);
                assert_eq!(plane.xy[1][axial], bounds[1][axial]);
            }
            assert_eq!(crossing[2].xy, bounds);

            let mut oblique = [plane(-15., 5.), plane(40., 6.)];
            separate_indexed_planes(&mut oblique, &index);
            for (plane, other) in [(&oblique[0], &oblique[1]), (&oblique[1], &oblique[0])] {
                let distances = plane.xy.map(|xy| {
                    axis.material_point(
                        cam_cldata::Point3::new(xy[0], xy[1], plane.z),
                        cam_cldata::Point3::new(index.center[0], index.center[1], index.center[2]),
                        plane.angle - other.angle,
                    )
                    .z - other.z
                });
                assert!(distances[0] * distances[1] > 0.);
                assert!(plane.xy[0][transverse] < plane.xy[1][transverse]);
            }

            let mut coplanar = [plane(-180., 3.), plane(0., 5.)];
            separate_indexed_planes(&mut coplanar, &index);
            assert!(coplanar[0].xy[1][axial] < coplanar[1].xy[0][axial]);
            assert!(coplanar[0].xy[1][axial] > coplanar[0].xy[0][axial]);
            assert!(coplanar[1].xy[1][axial] > coplanar[1].xy[0][axial]);
            for plane in coplanar {
                assert_eq!(
                    plane.xy.map(|p| p[transverse]),
                    bounds.map(|p| p[transverse])
                );
            }
        }
    }

    #[test]
    fn reference_offsets_are_transactional_persisted_and_operation_scoped() {
        use cam_model::HeightPlaneKind as P;
        let (mut app, id) = operation_editor();
        let original = app.controller.document().clone();
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "20".into()));
        let _ = app.update(Message::HeightOffsetChanged(P::Retract, "8".into()));
        assert_eq!(app.resolve_height_editor(id).unwrap().clearance, 28.);
        assert_eq!(app.controller.document(), &original);
        let _ = app.update(Message::OperationTabChanged(OperationTab::Tool));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Heights));
        assert_eq!(app.height_offsets[&P::Clearance], "20");
        let _ = app.update(Message::Apply);
        assert_eq!(
            app.controller.inspection_setup().height_settings[&id]
                .definition(P::Clearance)
                .offset,
            20.
        );
        assert_eq!(
            app.controller.inspection_setup().heights,
            original.setup().heights
        );
        assert!(app
            .controller
            .inspection_setup()
            .operations
            .iter()
            .filter(|o| o.id() != id)
            .all(|o| !app
                .controller
                .inspection_setup()
                .height_settings
                .contains_key(&o.id())));
        let path = std::env::temp_dir().join(format!(
            "opencam-relative-heights-{}.ocam",
            std::process::id()
        ));
        app.controller.save_project(&path).unwrap();
        let mut reopened = AppController::new(crate::default_machine());
        reopened.open_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            reopened.document().setup().height_settings[&id],
            app.controller.inspection_setup().height_settings[&id]
        );
        let _ = app.update(Message::Undo);
        assert_eq!(app.controller.document(), &original);
        assert!(!app.inspector_dirty());
        let _ = app.update(Message::Redo);
        assert!(!app.inspector_dirty());
        app.begin_operation_draft(id);
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "40".into()));
        let _ = app.update(Message::CancelOperationDraft);
        assert_eq!(app.height_offsets[&P::Clearance], "20");
    }
    #[test]
    fn drill_tip_through_bottom_is_staged_with_the_operation() {
        let (mut app, _) = operation_editor();
        app.controller.new_operation(OpKind::Drill);
        let Selection::Operation(id) = app.controller.selection() else {
            panic!("expected selected drill");
        };
        let cutter = app.controller.use_tool(cam_model::Tool {
            diameter: 5.0,
            length: 40.0,
            kind: cam_model::ToolKind::Drill {
                point_angle_deg: 118.0,
            },
            ..Default::default()
        });
        app.controller.edit_operation(id, |op| op.set_tool(cutter));
        app.begin_operation_draft(id);
        let before = app.controller.document().clone();
        let bottom = app.resolve_height_editor(id).unwrap().bottom;

        let _ = app.update(Message::DrillTipThroughBottomToggled(true));
        assert_eq!(app.controller.document(), &before);
        assert_eq!(app.resolve_height_editor(id).unwrap().bottom, bottom);
        let _ = app.update(Message::CancelOperationDraft);
        assert!(matches!(
            app.current_operation(id),
            Some(Operation::Drill(drill)) if !drill.tip_through_bottom
        ));

        app.begin_operation_draft(id);
        let _ = app.update(Message::DrillTipThroughBottomToggled(true));
        let _ = app.update(Message::Apply);
        let Some(Operation::Drill(drill)) = app.controller.operation(id) else {
            panic!("expected saved drill");
        };
        assert!(drill.tip_through_bottom);
        let setup = app.controller.inspection_setup();
        let tip_bottom = bottom
            - setup.origin_position(drill.work_offset)[2]
            - 2.5 / 59.0_f64.to_radians().tan();
        let outcome = app.controller.run(&cam_toolpath::CancelToken::new());
        assert!(
            outcome.program.steps().iter().any(|step| match step {
                cam_cldata::Step::Drill(cycle) =>
                    cycle.tag.op_id == id && (cycle.depth - tip_bottom).abs() < 1e-9,
                cam_cldata::Step::Linear { to, tag, .. } =>
                    tag.op_id == id && (to.z - tip_bottom).abs() < 1e-9,
                _ => false,
            }),
            "{:?}",
            outcome.diagnostics
        );
        let saved = serde_json::to_string(app.controller.document()).unwrap();
        let reopened: cam_model::Document = serde_json::from_str(&saved).unwrap();
        assert!(reopened.setup().operations.iter().any(
            |operation| matches!(operation, Operation::Drill(drill) if drill.id == id && drill.tip_through_bottom)
        ));
        let _ = app.update(Message::Undo);
        assert_eq!(app.controller.document(), &before);
    }

    #[test]
    fn all_five_planes_preview_live_offsets_in_the_setup_orientation_while_heights_is_selected() {
        use cam_model::HeightPlaneKind as P;
        let (mut app, id) = operation_editor();
        let camera = OrbitCamera::framed([0., 0., -10.], [100., 80., 25.]);
        let preview = app.height_preview(camera, 0.).unwrap();
        assert_eq!(preview.planes.len(), 5);
        assert!(preview.planes.iter().all(|p| !p.active));
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "23.5".into()));
        let preview = app.height_preview(camera, 0.).unwrap();
        assert_eq!(preview.planes.len(), 5);
        assert!(preview.planes[0].active);
        assert_eq!(preview.planes[0].z, 28.5);
        assert!(preview.planes[0].corners.iter().all(|p| p[2] == 28.5));
        let _ = app.update(Message::HeightOffsetChanged(P::Bottom, "-0.25".into()));
        let preview = app.height_preview(camera, 0.).unwrap();
        assert_eq!(
            preview
                .planes
                .iter()
                .find(|p| p.name == "Bottom")
                .unwrap()
                .z,
            -4.25
        );
        app.controller.edit_setup(|s| {
            s.frame.world_from_setup.rotation = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]]
        });
        let preview = app.height_preview(camera, 0.).unwrap();
        assert!(preview.planes[0]
            .corners
            .iter()
            .all(|p| p[0] == preview.planes[0].z as f32));
        let _ = app.update(Message::HeightFocus(
            super::super::height_editor::plane_input_id(P::Bottom),
            false,
        ));
        let preview = app.height_preview(camera, 0.).unwrap();
        assert_eq!(preview.planes.len(), 5);
        assert!(preview.planes.iter().all(|p| !p.active));
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "24".into()));
        let _ = app.update(Message::OperationTabChanged(OperationTab::Tool));
        assert!(app.height_preview(camera, 0.).is_none());
        assert!(app
            .controller
            .inspection_setup()
            .height_settings
            .contains_key(&id));
    }

    #[test]
    fn indexed_height_planes_match_rotated_stock_and_machine_frame_for_both_axes() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R, RotaryAxis};
        let (mut app, id) = operation_editor();
        app.controller.edit_stock(|stock| {
            *stock = cam_model::Stock::FixedBox {
                size: [30., 20., 12.],
                model_position: [0.3, 0.4, 0.2],
            }
        });
        app.controller.edit_setup(|setup| {
            setup.frame.world_from_setup.rotation = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
        });
        app.rotary_action(rotary_view::Action::Indexed(true));
        app.rotary_action(rotary_view::Action::IndexMode(
            rotary_view::IndexMode::Manual,
        ));
        for (plane, reference, offset) in [
            (P::Top, R::StockTop, 0.),
            (P::Bottom, R::StockBottom, 0.),
            (P::Feed, R::Plane(P::Top), 2.),
            (P::Retract, R::Plane(P::Top), 4.),
            (P::Clearance, R::Plane(P::Retract), 6.),
        ] {
            *app.height_draft.as_mut().unwrap().definition_mut(plane) =
                cam_model::HeightDefinition::new(reference, offset);
            app.height_offsets.insert(plane, fmt_num(offset));
        }
        let (min, max) = app.controller.stock_box();
        let frame = app.controller.inspection_setup().frame.clone();
        let camera = OrbitCamera::framed([-50.; 3], [100.; 3]);
        for (axis, angle) in [
            (RotaryAxis::A, 90.),
            (RotaryAxis::B, -45.),
            (RotaryAxis::A, 270.),
        ] {
            app.rotary_action(rotary_view::Action::IndexAxis(axis));
            for (i, value) in [(0, angle), (1, 2.), (2, 3.), (3, -4.)] {
                app.rotary_action(rotary_view::Action::IndexValue(i, value.to_string()));
            }
            let center = cam_cldata::Point3::new(2., 3., -4.);
            let rotated: Vec<_> = (0..8)
                .map(|bits| {
                    let p = std::array::from_fn::<_, 3, _>(|i| {
                        if bits & (1 << i) == 0 {
                            min[i]
                        } else {
                            max[i]
                        }
                    });
                    axis.material_point(cam_cldata::Point3::new(p[0], p[1], p[2]), center, -angle)
                })
                .collect();
            let zmin = rotated.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
            let zmax = rotated
                .iter()
                .map(|p| p.z)
                .fold(f64::NEG_INFINITY, f64::max);
            let heights = app.resolve_height_editor(id).unwrap();
            assert!((heights.top - zmax).abs() < 1e-8);
            assert!((heights.bottom - zmin).abs() < 1e-8);
            assert!((heights.clearance - zmax - 10.).abs() < 1e-8);
            let overlay = app.height_preview(camera, 0.).unwrap();
            assert_eq!(overlay.planes.len(), 5);
            for plane in overlay.planes {
                for point in plane.corners {
                    let p = frame.to_setup_vector(point.map(f64::from));
                    let machine = axis.material_point(
                        cam_cldata::Point3::new(p[0], p[1], p[2]),
                        center,
                        -angle,
                    );
                    assert!(
                        (machine.z - plane.z).abs() < 1e-4,
                        "{} {axis:?} {angle}",
                        plane.name
                    );
                }
                assert!((plane.value_label.parse::<f64>().unwrap() - plane.z).abs() < 0.001);
            }
        }
    }

    #[test]
    fn indexed_round_stock_heights_follow_the_rotated_cylinder() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R, RotaryAxis};
        let (mut app, id) = operation_editor();
        app.controller.edit_stock(|stock| {
            *stock = cam_model::Stock::FixedRound {
                diameter: 20.,
                inner_diameter: 0.,
                length: 50.,
                position: 0.5,
                tube: false,
                axis: Some(cam_model::StockAxis::Origin {
                    origin_axis: cam_model::StockOriginAxis::X,
                }),
            }
        });
        app.rotary_action(rotary_view::Action::Indexed(true));
        app.rotary_action(rotary_view::Action::IndexMode(
            rotary_view::IndexMode::Manual,
        ));
        *app.height_draft.as_mut().unwrap().definition_mut(P::Top) =
            cam_model::HeightDefinition::new(R::StockTop, 0.);
        *app.height_draft.as_mut().unwrap().definition_mut(P::Bottom) =
            cam_model::HeightDefinition::new(R::StockBottom, 0.);
        app.height_offsets.insert(P::Top, "0".into());
        app.height_offsets.insert(P::Bottom, "0".into());
        let setup = app.controller.inspection_setup();
        let (frame, min, max) = app
            .controller
            .document()
            .round_stock_geometry(setup)
            .unwrap()
            .unwrap();
        let setup_frame = setup.frame.clone();
        for (axis, angle) in [(RotaryAxis::A, 47.), (RotaryAxis::B, 90.)] {
            app.rotary_action(rotary_view::Action::IndexAxis(axis));
            app.rotary_action(rotary_view::Action::IndexValue(0, angle.to_string()));
            app.rotary_action(rotary_view::Action::IndexValue(2, "3".into()));
            let center = cam_cldata::Point3::new(0., 3., 0.);
            let mut lower = f64::INFINITY;
            let mut upper = f64::NEG_INFINITY;
            for axial in [min[2], max[2]] {
                for i in 0..1440 {
                    let angle_sample = std::f64::consts::TAU * i as f64 / 1440.;
                    let p = [
                        (min[0] + max[0]) * 0.5 + (max[0] - min[0]) * 0.5 * angle_sample.cos(),
                        (min[1] + max[1]) * 0.5 + (max[0] - min[0]) * 0.5 * angle_sample.sin(),
                        axial,
                    ];
                    let p = setup_frame.to_setup_vector(frame.to_world_vector(p));
                    let p = axis.material_point(
                        cam_cldata::Point3::new(p[0], p[1], p[2]),
                        center,
                        -angle,
                    );
                    lower = lower.min(p.z);
                    upper = upper.max(p.z);
                }
            }
            let heights = app.resolve_height_editor(id).unwrap();
            assert!(
                (heights.top - upper).abs() < 0.001,
                "{axis:?}: {} vs {upper}",
                heights.top
            );
            assert!(
                (heights.bottom - lower).abs() < 0.001,
                "{axis:?}: {} vs {lower}",
                heights.bottom
            );
        }
    }
    #[test]
    fn cycles_invalid_offsets_and_missing_selections_block_apply_and_defaults_recover() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R};
        let (mut app, id) = operation_editor();
        let before = app.controller.document().clone();
        let _ = app.update(Message::HeightReferenceChanged(
            P::Retract,
            R::Plane(P::Clearance),
        ));
        assert!(app
            .resolve_height_editor(id)
            .unwrap_err()
            .contains("Circular"));
        let _ = app.update(Message::Apply);
        assert_eq!(app.controller.document(), &before);
        let _ = app.update(Message::HeightDefaults);
        let _ = app.update(Message::HeightOffsetChanged(P::Top, "NaN".into()));
        assert!(app.any_field_invalid());
        let _ = app.update(Message::HeightDefaults);
        let _ = app.update(Message::HeightReferenceChanged(P::Top, R::Selection));
        assert!(app.any_field_invalid());
        let _ = app.update(Message::HeightDefaults);
        let _ = app.update(Message::HeightReferenceChanged(P::Feed, R::Disabled));
        assert!(app.resolve_height_editor(id).unwrap().feed.is_none());
        let _ = app.update(Message::Apply);
        assert_eq!(
            app.controller.inspection_setup().height_settings[&id]
                .definition(P::Feed)
                .reference,
            R::Disabled
        );
    }
    #[test]
    fn selected_drawing_reference_is_persistent_and_picking_preserves_the_operation_editor() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R};
        let (mut app, id) = operation_editor();
        let _ = app.update(Message::HeightReferenceChanged(P::Top, R::Selection));
        let _ = app.update(Message::HeightPickToggle(P::Top));
        let _ = app.update(Message::PickWorld([10., 10.], 1.));
        assert_eq!(app.controller.selection(), Selection::Operation(id));
        assert!(!app.cad_inspecting);
        assert_eq!(
            app.height_draft
                .as_ref()
                .unwrap()
                .definition(P::Top)
                .drawings
                .len(),
            1
        );
        assert_eq!(app.resolve_height_editor(id).unwrap().top, 0.);
        let _ = app.update(Message::HeightPickToggle(P::Top));
        let _ = app.update(Message::Apply);
        assert_eq!(
            app.controller.inspection_setup().height_settings[&id]
                .definition(P::Top)
                .drawings
                .len(),
            1
        );
    }
    #[test]
    fn every_operation_gets_valid_reference_defaults_including_natural_chamfer_depth() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R};
        let (mut app, _) = operation_editor();
        for kind in [
            OpKind::Profile,
            OpKind::Pocket,
            OpKind::Face,
            OpKind::Drill,
            OpKind::Thread,
            OpKind::Chamfer,
            OpKind::Engrave,
            OpKind::Carve,
        ] {
            app.controller.new_operation(kind);
            let id = app
                .controller
                .inspection_setup()
                .operations
                .last()
                .unwrap()
                .id();
            app.begin_operation_draft(id);
            let heights = app
                .resolve_height_editor(id)
                .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert_eq!(
                app.height_draft
                    .as_ref()
                    .unwrap()
                    .definition(P::Clearance)
                    .reference,
                R::Plane(P::Retract)
            );
            assert_eq!(
                app.height_draft
                    .as_ref()
                    .unwrap()
                    .definition(P::Clearance)
                    .offset,
                10.
            );
            if kind == OpKind::Face {
                assert_eq!(
                    heights.bottom,
                    app.controller.document().height_model_bounds().1[2]
                );
                assert!(heights.top >= heights.bottom);
            } else {
                assert!(heights.top > heights.bottom);
            }
            if kind == OpKind::Drill {
                for (plane, reference) in [(P::Top, R::HoleTop), (P::Bottom, R::HoleBottom)] {
                    let options = R::options_for(plane, app.current_operation(id));
                    assert!(options.contains(&R::HoleTop) && options.contains(&R::HoleBottom));
                    assert_eq!(
                        app.height_draft.as_ref().unwrap().definition(plane),
                        &cam_model::HeightDefinition::new(reference, 0.)
                    );
                }
                let _ = app.update(Message::HeightReferenceChanged(P::Top, R::StockTop));
                let _ = app.update(Message::HeightDefaults);
                assert_eq!(
                    app.height_draft
                        .as_ref()
                        .unwrap()
                        .definition(P::Top)
                        .reference,
                    R::HoleTop
                );
            }
            if kind == OpKind::Chamfer {
                assert!((heights.bottom + 1.).abs() < 1e-9);
                let _ = app.update(Message::HeightOffsetChanged(P::Bottom, "0.5".into()));
                let error = app.resolve_height_editor(id).unwrap_err();
                assert!(
                    error.contains("selected chamfer and cutter geometry"),
                    "{error}"
                );
                let _ = app.update(Message::CancelOperationDraft);
            }
        }
    }
    #[test]
    fn height_labels_use_the_operation_datum_while_planes_stay_in_model_space() {
        use cam_model::HeightPlaneKind as P;
        let (mut app, id) = operation_editor();
        app.controller.edit_setup(|setup| {
            setup.origin = [-476.8, 367., 54.];
            setup.extra_origins.push(cam_model::Origin {
                index: 2,
                position: [-476.8, 367., 50.],
            });
            setup.frame.world_from_setup.rotation = [[-1., 0., 0.], [0., 0., 1.], [0., 1., 0.]];
        });
        // The editor may be editing a different datum than the tree selection.
        app.operation_draft.as_mut().unwrap().set_work_offset(2);
        assert_eq!(app.height_display_value(id, P::Top, 74.), "24");
        assert_eq!(app.height_display_value(id, P::Bottom, 55.), "5");
        let camera = OrbitCamera::framed([0., 0., 0.], [100., 100., 100.]);
        let preview = app.height_preview(camera, 0.).unwrap();
        for plane in preview.planes {
            assert!(plane.corners.iter().all(|p| p[1] == plane.z as f32));
            assert_eq!(plane.value_label.parse::<f64>().unwrap(), plane.z - 50.);
        }
        app.operation_draft.as_mut().unwrap().set_work_offset(1);
        assert_eq!(app.height_display_value(id, P::Top, 74.), "20");
        assert_eq!(app.height_display_value(id, P::Bottom, 55.), "1");
    }

    #[test]
    fn height_labels_keep_entered_precision_without_float_tails() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R};
        let (mut app, id) = operation_editor();
        let _ = app.update(Message::HeightReferenceChanged(P::Retract, R::Origin));
        let _ = app.update(Message::HeightOffsetChanged(P::Retract, "0.1".into()));
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "0.20".into()));
        let _ = app.update(Message::HeightReferenceChanged(P::Feed, R::Disabled));
        let z = app.resolve_height_editor(id).unwrap().clearance;
        assert_eq!(app.height_display_value(id, P::Clearance, z), "0.30");
        assert_eq!(app.height_offsets[&P::Clearance], "0.20");
        assert_eq!(app.height_display_value(id, P::Bottom, -0.00000000001), "0");
        let _ = app.update(Message::HeightOffsetChanged(
            P::Clearance,
            "0.12345678".into(),
        ));
        assert_eq!(
            app.height_display_value(id, P::Clearance, 0.22345678),
            "0.22345678"
        );
        let _ = app.update(Message::HeightOffsetChanged(P::Clearance, "2e-4".into()));
        assert_eq!(app.height_display_value(id, P::Clearance, 0.1002), "0.1002");
    }
    #[test]
    fn wrapped_planes_are_cylinders_in_both_rotary_orientations() {
        for axis in [cam_model::RotaryAxis::A, cam_model::RotaryAxis::B] {
            let wrap = cam_model::RotaryWrap {
                axis,
                radius: 100.,
                offset: 1.,
                center: [10., 20., 30.],
                ..Default::default()
            };
            let frame = cam_model::SetupFrame::default();
            let paths = wrapped_plane_outlines(&wrap, 15., [-40.; 3], [40.; 3], &frame);
            assert_eq!(paths.len(), 6);
            assert_eq!(paths[0].first(), paths[0].last());
            for point in paths.iter().take(2).flatten() {
                let radius = match axis {
                    cam_model::RotaryAxis::A => {
                        (point[1] as f64 - 20.).hypot(point[2] as f64 - 30.)
                    }
                    cam_model::RotaryAxis::B => {
                        (point[0] as f64 - 10.).hypot(point[2] as f64 - 30.)
                    }
                };
                assert!((radius - 116.).abs() < 0.0001, "{radius}");
            }
        }
    }
}
