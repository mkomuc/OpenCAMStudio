//! Setup picking uses screen distances so a corner catches consistently in every
//! camera view. Hover and click run the same query against the same geometry.
use super::*;

type Point3 = [f64; 3];
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SetupFeature {
    Stock(usize),
    Model(EntityRef),
    Drawing(LoopRef),
}
#[derive(Clone, Debug)]
struct PickEdge {
    source: SetupFeature,
    points: Vec<Point3>,
    straight: bool,
}
#[derive(Clone, Debug)]
struct PickPoint {
    source: SetupFeature,
    point: Point3,
    kind: SnapKind,
}
#[derive(Clone, Debug)]
pub(super) struct SetupSnap {
    pub point: Point3,
    pub kind: SnapKind,
    pub source: SetupFeature,
}
#[derive(Clone, Debug, Default)]
pub(super) struct SetupHover {
    pub snap: Option<SetupSnap>,
    pub edge: Option<SetupFeature>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct SetupPickGeometry {
    edges: Vec<PickEdge>,
    points: Vec<PickPoint>,
    frame: cam_model::SetupFrame,
    top: f64,
}

impl SetupPickGeometry {
    pub fn new(
        controller: &AppController,
        setup: &cam_model::Setup,
        hidden: &BTreeSet<InstanceId>,
    ) -> Self {
        let mut result = Self {
            frame: setup.frame.clone(),
            top: setup.heights.top_of_stock,
            ..Self::default()
        };
        if !controller.has_geometry() {
            return result;
        }
        let (min, max) = controller.pending_setup_model_bounds(setup);
        let (min, max) = setup.stock.resolve_bounds(min, max);
        if (0..3).all(|i| min[i].is_finite() && max[i].is_finite() && min[i] <= max[i]) {
            let corners: Vec<_> = (0..8)
                .map(|index| {
                    setup.frame.to_world_vector(std::array::from_fn(|i| {
                        if index & (1 << i) == 0 {
                            min[i]
                        } else {
                            max[i]
                        }
                    }))
                })
                .collect();
            for index in 0..8 {
                for axis in 0..3 {
                    let other = index ^ (1 << axis);
                    if index < other {
                        let id = result.edges.len();
                        result.add_line(SetupFeature::Stock(id), corners[index], corners[other]);
                    }
                }
            }
        }
        for instance in &controller.geometry_state().instances {
            if !instance.visible || hidden.contains(&instance.id) {
                continue;
            }
            let Some(asset) = controller.geometry_state().asset(instance.asset_id) else {
                continue;
            };
            for edge in asset
                .topology
                .edges
                .iter()
                .filter(|e| e.body_id == instance.body_id)
            {
                let source = SetupFeature::Model(EntityRef {
                    asset_id: asset.id,
                    revision: asset.revision,
                    instance_id: instance.id,
                    body_id: instance.body_id,
                    kind: EntityKind::Edge,
                    id: edge.id,
                });
                match &edge.curve {
                    cam_model::CadCurve::Line { start, end } => result.add_line(
                        source,
                        instance.placement.transform_point(*start),
                        instance.placement.transform_point(*end),
                    ),
                    cam_model::CadCurve::CircleArc {
                        center,
                        normal,
                        x_axis,
                        radius,
                        start_angle,
                        end_angle,
                    } => {
                        let y = [
                            normal[1] * x_axis[2] - normal[2] * x_axis[1],
                            normal[2] * x_axis[0] - normal[0] * x_axis[2],
                            normal[0] * x_axis[1] - normal[1] * x_axis[0],
                        ];
                        let at = |angle: f64| {
                            instance.placement.transform_point(std::array::from_fn(|i| {
                                center[i] + radius * (x_axis[i] * angle.cos() + y[i] * angle.sin())
                            }))
                        };
                        let span = end_angle - start_angle;
                        let steps = ((span.abs() * radius.abs() / 0.05).sqrt().ceil() as usize)
                            .clamp(8, 2048);
                        let points: Vec<_> = (0..=steps)
                            .map(|i| at(start_angle + span * i as f64 / steps as f64))
                            .collect();
                        if span.abs() < std::f64::consts::TAU - 1e-8 {
                            for angle in [*start_angle, *end_angle] {
                                result.points.push(PickPoint {
                                    source: source.clone(),
                                    point: at(angle),
                                    kind: SnapKind::End,
                                });
                            }
                        }
                        result.points.push(PickPoint {
                            source: source.clone(),
                            point: at((start_angle + end_angle) / 2.),
                            kind: SnapKind::Mid,
                        });
                        let quarter = std::f64::consts::FRAC_PI_2;
                        let lo = start_angle.min(*end_angle);
                        let hi = start_angle.max(*end_angle);
                        for n in (lo / quarter).ceil() as i64..=(hi / quarter).floor() as i64 {
                            result.points.push(PickPoint {
                                source: source.clone(),
                                point: at(n as f64 * quarter),
                                kind: SnapKind::Quadrant,
                            });
                        }
                        result.edges.push(PickEdge {
                            source,
                            points,
                            straight: false,
                        });
                    }
                    _ => {}
                }
            }
            for vertex in asset
                .topology
                .vertices
                .iter()
                .filter(|v| v.body_id == instance.body_id)
            {
                result.points.push(PickPoint {
                    source: SetupFeature::Model(EntityRef {
                        asset_id: asset.id,
                        revision: asset.revision,
                        instance_id: instance.id,
                        body_id: instance.body_id,
                        kind: EntityKind::Vertex,
                        id: vertex.id,
                    }),
                    point: instance.placement.transform_point(vertex.point),
                    kind: SnapKind::End,
                });
            }
        }
        // Reuse the drawing controller's corner/mid/quadrant semantics instead of
        // treating every tessellation vertex of a circle as an endpoint.
        let references = controller
            .regions()
            .iter()
            .enumerate()
            .flat_map(|(i, r)| {
                std::iter::once(LoopRef::outer(i))
                    .chain((0..r.holes().len()).map(move |h| LoopRef::hole(i, h)))
            })
            .chain((0..controller.open_paths().len()).map(LoopRef::open));
        for reference in references {
            let Some((points, closed)) = controller.loop_points(reference) else {
                continue;
            };
            if points.len() < 2 {
                continue;
            }
            let corners = crate::controller::loop_corners(&points);
            let mut add = |kind, p: [f64; 2]| {
                result.points.push(PickPoint {
                    source: SetupFeature::Drawing(reference),
                    point: [p[0], p[1], 0.],
                    kind,
                })
            };
            if closed {
                for &i in &corners {
                    add(SnapKind::End, [points[i].x, points[i].y]);
                }
                for p in crate::controller::loop_mids(&points, &corners) {
                    add(SnapKind::Mid, p);
                }
                if corners.is_empty() {
                    if let Some(([x, y], r)) = crate::controller::fit_circle(&points) {
                        for (dx, dy) in [(1., 0.), (0., 1.), (-1., 0.), (0., -1.)] {
                            add(SnapKind::Quadrant, [x + dx * r, y + dy * r]);
                        }
                    }
                }
            } else {
                for p in [points[0], points[points.len() - 1]] {
                    add(SnapKind::End, [p.x, p.y]);
                }
            }
            let mut path: Vec<_> = points.iter().map(|p| [p.x, p.y, 0.]).collect();
            if closed && !path.is_empty() {
                path.push(path[0]);
            }
            result.edges.push(PickEdge {
                source: SetupFeature::Drawing(reference),
                points: path,
                straight: false,
            });
        }
        result
    }
    fn add_line(&mut self, source: SetupFeature, a: Point3, b: Point3) {
        for point in [a, b] {
            self.points.push(PickPoint {
                source: source.clone(),
                point,
                kind: SnapKind::End,
            });
        }
        self.points.push(PickPoint {
            source: source.clone(),
            point: std::array::from_fn(|i| (a[i] + b[i]) / 2.),
            kind: SnapKind::Mid,
        });
        self.edges.push(PickEdge {
            source,
            points: vec![a, b],
            straight: true,
        });
    }
    pub fn stock_lines(&self) -> impl Iterator<Item = (usize, [Point3; 2])> + '_ {
        self.edges.iter().filter_map(|e| {
            if let SetupFeature::Stock(id) = e.source {
                Some((id, [e.points[0], e.points[1]]))
            } else {
                None
            }
        })
    }
    pub fn stock_bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut points = self.stock_lines().flat_map(|(_, p)| p);
        let first = points.next()?.map(|v| v as f32);
        let (mut min, mut max) = (first, first);
        for p in points {
            for i in 0..3 {
                min[i] = min[i].min(p[i] as f32);
                max[i] = max[i].max(p[i] as f32);
            }
        }
        Some((min, max))
    }
    pub fn direction(&self, source: &SetupFeature) -> Option<Point3> {
        let e = self
            .edges
            .iter()
            .find(|e| &e.source == source && e.straight)?;
        Some(std::array::from_fn(|i| e.points[1][i] - e.points[0][i]))
    }
    pub fn fallback(&self, ray: cam_render::Ray) -> Option<Point3> {
        let origin = self.frame.to_setup_vector(ray.origin);
        let direction = self.frame.to_setup_vector(ray.direction);
        if direction[2].abs() < 1e-10 {
            return None;
        }
        let t = (self.top - origin[2]) / direction[2];
        (t >= 0.).then(|| std::array::from_fn(|i| ray.origin[i] + t * ray.direction[i]))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn query(
        &self,
        camera: OrbitCamera,
        bounds: iced::Rectangle,
        cursor: iced::Point,
        enabled: &[SnapKind],
        axis: bool,
        scene: &CadScene,
        through: bool,
        aperture: f32,
    ) -> SetupHover {
        let project = |p| project(camera, bounds, p);
        let aspect = bounds.width / bounds.height.max(1.);
        let ray = camera.pick_ray(
            2. * (cursor.x - bounds.x) / bounds.width - 1.,
            1. - 2. * (cursor.y - bounds.y) / bounds.height,
            aspect,
        );
        let visible = |point| through || scene.point_visible(ray, point);
        let mut best: Option<(u8, f32, f32, SetupSnap)> = None;
        if !axis {
            for candidate in &self.points {
                if !enabled.contains(&candidate.kind) {
                    continue;
                }
                let Some((p, depth)) = project(candidate.point) else {
                    continue;
                };
                let distance = p.distance(cursor);
                if distance > aperture || !visible(candidate.point) {
                    continue;
                }
                let key = (
                    candidate.kind.priority(),
                    (distance * 10.).round() / 10.,
                    depth,
                );
                if best.as_ref().is_none_or(|(k, d, z, _)| key < (*k, *d, *z)) {
                    best = Some((
                        key.0,
                        key.1,
                        key.2,
                        SetupSnap {
                            point: candidate.point,
                            kind: candidate.kind,
                            source: candidate.source.clone(),
                        },
                    ));
                }
            }
        }
        let mut edge_best: Option<(f32, f32, SetupFeature)> = None;
        for edge in &self.edges {
            if axis && !edge.straight {
                continue;
            }
            for segment in edge.points.windows(2) {
                let (Some((a, za)), Some((b, zb))) = (project(segment[0]), project(segment[1]))
                else {
                    continue;
                };
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let den = dx * dx + dy * dy;
                let t = if den > 1e-8 {
                    ((cursor.x - a.x) * dx + (cursor.y - a.y) * dy) / den
                } else {
                    0.
                }
                .clamp(0., 1.);
                let p = iced::Point::new(a.x + t * dx, a.y + t * dy);
                let point = std::array::from_fn(|i| {
                    segment[0][i] + t as f64 * (segment[1][i] - segment[0][i])
                });
                let distance = p.distance(cursor);
                let depth = za + t * (zb - za);
                if distance > aperture || !visible(point) {
                    continue;
                }
                if edge_best
                    .as_ref()
                    .is_none_or(|(d, z, _)| (distance, depth) < (*d, *z))
                {
                    edge_best = Some((distance, depth, edge.source.clone()));
                }
                if !axis
                    && enabled.contains(&SnapKind::Nearest)
                    && best
                        .as_ref()
                        .is_none_or(|(k, d, z, _)| (3, distance, depth) < (*k, *d, *z))
                {
                    best = Some((
                        3,
                        distance,
                        depth,
                        SetupSnap {
                            point,
                            kind: SnapKind::Nearest,
                            source: edge.source.clone(),
                        },
                    ));
                }
            }
        }
        SetupHover {
            snap: best.map(|(_, _, _, hit)| hit),
            edge: edge_best.map(|(_, _, source)| source),
        }
    }
    #[allow(clippy::too_many_arguments)] // Cohesive per-frame camera and picking preferences.
    pub fn overlays(
        &self,
        scene: &mut Scene,
        camera: OrbitCamera,
        bounds: iced::Rectangle,
        hover: &SetupHover,
        enabled: &[SnapKind],
        highlight: bool,
        show_points: bool,
        marker_scale: f32,
        cad_scene: &CadScene,
        through: bool,
    ) {
        let source = hover
            .snap
            .as_ref()
            .map(|hit| &hit.source)
            .or(hover.edge.as_ref());
        if let Some(edge) = self
            .edges
            .iter()
            .find(|e| Some(&e.source) == source)
            .filter(|_| highlight)
        {
            scene.add_strip(
                edge.points.iter().map(|p| p.map(|v| v as f32)).collect(),
                PICK_HOVER,
            );
        }
        let wpp =
            camera.world_per_pixel(bounds.height) / (bounds.width / bounds.height.max(1.)).min(1.);
        let (r, u) = (camera.right(), camera.up());
        let mut mark = |point: Point3, size: f32, color| {
            let at = |x: f32, y: f32| {
                std::array::from_fn(|i| point[i] as f32 + size * (x * r[i] + y * u[i]))
            };
            scene.add_strip(
                vec![
                    at(-1., -1.),
                    at(1., -1.),
                    at(1., 1.),
                    at(-1., 1.),
                    at(-1., -1.),
                ],
                color,
            );
        };
        if highlight && show_points {
            let aspect = bounds.width / bounds.height.max(1.);
            let ray = camera.pick_ray(0., 0., aspect);
            for p in &self.points {
                if enabled.contains(&p.kind) && (through || cad_scene.point_visible(ray, p.point)) {
                    mark(p.point, 2.5 * wpp, SNAP_MARK);
                }
            }
        }
        if let Some(hit) = &hover.snap {
            mark(hit.point, 7. * marker_scale * wpp, SNAP_MARK);
        }
    }
}
// Orthographic projection matches the viewport's actual camera, including pan,
// zoom and portrait aspect correction. Depth breaks coincident screen ties.
pub(super) fn project(
    camera: OrbitCamera,
    bounds: iced::Rectangle,
    point: Point3,
) -> Option<(iced::Point, f32)> {
    let m = camera.view_proj(bounds.width / bounds.height.max(1.));
    let q: [f32; 4] = std::array::from_fn(|row| {
        (0..3).map(|i| m[i][row] * point[i] as f32).sum::<f32>() + m[3][row]
    });
    if q[3].abs() < 1e-10 {
        return None;
    }
    Some((
        iced::Point::new(
            bounds.x + (q[0] / q[3] + 1.) * bounds.width / 2.,
            bounds.y + (1. - q[1] / q[3]) * bounds.height / 2.,
        ),
        q[2] / q[3],
    ))
}
