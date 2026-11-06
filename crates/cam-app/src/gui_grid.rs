//! Dimensioned world grid shared by the GPU line pass and canvas labels.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridPlane {
    Xy,
    Xz,
    Yz,
}
impl GridPlane {
    fn for_camera(camera: OrbitCamera) -> Self {
        // Only cardinal side views switch planes; even a small orbit restores XY.
        let normal = [
            camera.orient[0][2],
            camera.orient[1][2],
            camera.orient[2][2],
        ];
        const EPSILON: f32 = 1e-5;
        if normal[2].abs() < EPSILON && normal[0].abs() < EPSILON {
            Self::Xz
        } else if normal[2].abs() < EPSILON && normal[1].abs() < EPSILON {
            Self::Yz
        } else {
            Self::Xy
        }
    }
    fn axes(self) -> [usize; 2] {
        match self {
            Self::Xy => [0, 1],
            Self::Xz => [0, 2],
            Self::Yz => [1, 2],
        }
    }
    fn normal_axis(self) -> usize {
        match self {
            Self::Xy => 2,
            Self::Xz => 1,
            Self::Yz => 0,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Xy => "XY",
            Self::Xz => "XZ",
            Self::Yz => "YZ",
        }
    }
    fn point(self, coordinates: [f32; 2]) -> [f32; 3] {
        let mut point = [0.; 3];
        let [a, b] = self.axes();
        point[a] = coordinates[0];
        point[b] = coordinates[1];
        point
    }
    fn pick(self, camera: OrbitCamera, u: f32, v: f32, aspect: f32) -> Option<[f32; 2]> {
        let ray = camera.pick_ray(u, v, aspect);
        let normal = self.normal_axis();
        let direction = ray.direction[normal];
        if direction.abs() < 1e-8 {
            return None;
        }
        let t = -ray.origin[normal] / direction;
        let [a, b] = self.axes();
        Some([
            (ray.origin[a] + t * ray.direction[a]) as f32,
            (ray.origin[b] + t * ray.direction[b]) as f32,
        ])
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GridLayout {
    plane: GridPlane,
    min: [f32; 2],
    max: [f32; 2],
    visible_min: [f32; 2],
    visible_max: [f32; 2],
    pub step: f32,
}
impl GridLayout {
    pub(super) fn new(camera: OrbitCamera, size: iced::Size, machine_size: [f32; 3]) -> Self {
        // The machine gives a travel, not an absolute work origin. Center its
        // footprint on the world origin, independently of the camera and job.
        let plane = GridPlane::for_camera(camera);
        let axes = plane.axes();
        let half = axes.map(|axis| {
            let v = machine_size[axis];
            if v.is_finite() && v > 0. {
                v / 2.
            } else {
                50.
            }
        });
        let min = half.map(|v| -v);
        let max = half;
        let aspect = size.width / size.height.max(1.);
        let points: Vec<_> = [[-1., -1.], [-1., 1.], [1., -1.], [1., 1.]]
            .into_iter()
            .filter_map(|[x, y]| plane.pick(camera, x, y, aspect))
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .collect();
        let visible_min = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .reduce(f32::min)
                .unwrap_or(min[axis])
                .max(min[axis])
        });
        let visible_max = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .reduce(f32::max)
                .unwrap_or(max[axis])
                .min(max[axis])
        });
        let raw = camera.world_per_pixel(size.height.max(1.)) * 80.;
        let visible_span = (visible_max[0] - visible_min[0]).max(visible_max[1] - visible_min[1]);
        let step = nice_step(raw.max(visible_span / 100.));
        Self {
            plane,
            min,
            max,
            visible_min,
            visible_max,
            step,
        }
    }
    fn ticks(&self, axis: usize) -> Vec<f32> {
        let start = (self.visible_min[axis] / self.step).ceil();
        let count =
            ((self.visible_max[axis] / self.step).floor() - start + 1.).clamp(0., 101.) as usize;
        (0..count).map(|i| (start + i as f32) * self.step).collect()
    }
    pub(super) fn vertices(&self) -> Vec<Vertex> {
        let mut scene = Scene::new();
        for axis in 0..2 {
            for tick in self.ticks(axis) {
                let color = if tick.abs() < self.step * 0.01 {
                    [0.36, 0.43, 0.49, 1.]
                } else {
                    [0.20, 0.24, 0.28, 1.]
                };
                let mut a = self.min;
                let mut b = self.max;
                a[axis] = tick;
                b[axis] = tick;
                scene.add_strip(vec![self.plane.point(a), self.plane.point(b)], color);
            }
        }
        let [x, y] = self.max;
        scene.add_strip(
            [[-x, -y], [x, -y], [x, y], [-x, y], [-x, -y]]
                .into_iter()
                .map(|point| self.plane.point(point))
                .collect(),
            [0.30, 0.34, 0.38, 1.],
        );
        scene.line_vertices()
    }
}
fn nice_step(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= 0. {
        return 10.;
    }
    let power = 10_f32.powf(raw.log10().floor());
    let scaled = raw / power;
    let nice = if scaled <= 1. {
        1.
    } else if scaled <= 2. {
        2.
    } else if scaled <= 5. {
        5.
    } else {
        10.
    };
    (nice * power).max(0.000001)
}
fn tick_text(value: f32, step: f32) -> String {
    let decimals = (-step.log10().floor()).clamp(0., 6.) as usize;
    let value = if value.abs() < step * 0.001 {
        0.
    } else {
        value
    };
    format!("{value:.decimals$}")
}
fn project(camera: OrbitCamera, point: [f32; 3], size: iced::Size) -> iced::Point {
    let matrix = camera.view_proj(size.width / size.height.max(1.));
    let clip: [f32; 4] = std::array::from_fn(|row| {
        matrix[0][row] * point[0]
            + matrix[1][row] * point[1]
            + matrix[2][row] * point[2]
            + matrix[3][row]
    });
    iced::Point::new(
        (clip[0] / clip[3] + 1.) * size.width / 2.,
        (1. - clip[1] / clip[3]) * size.height / 2.,
    )
}
// Clip a projected grid line to the viewport so labels remain visible when
// zooming into the plane, even when its physical boundary is off screen.
fn clipped_line(
    a: iced::Point,
    b: iced::Point,
    size: iced::Size,
) -> Option<(iced::Point, iced::Point)> {
    if ![a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite()) || size.width < 50. || size.height < 50.
    {
        return None;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut lo, mut hi) = (0_f32, 1_f32);
    for (p, q) in [
        (-dx, a.x - 18.),
        (dx, size.width - 18. - a.x),
        (-dy, a.y - 18.),
        (dy, size.height - 30. - a.y),
    ] {
        if p.abs() < 1e-6 {
            if q < 0. {
                return None;
            }
        } else if p < 0. {
            lo = lo.max(q / p);
        } else {
            hi = hi.min(q / p);
        }
    }
    if lo > hi {
        return None;
    }
    Some((
        iced::Point::new(a.x + lo * dx, a.y + lo * dy),
        iced::Point::new(a.x + hi * dx, a.y + hi * dy),
    ))
}
#[derive(Clone, Copy)]
pub(super) struct GridLabels {
    pub camera: OrbitCamera,
    pub grid_size: [f32; 3],
}
impl canvas::Program<Message> for GridLabels {
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
        let layout = GridLayout::new(self.camera, size, self.grid_size);
        let mut frame = canvas::Frame::new(renderer, size);
        for axis in 0..2 {
            let mut previous: Option<iced::Point> = None;
            for tick in layout.ticks(axis) {
                let mut a = layout.min;
                let mut b = layout.max;
                a[axis] = tick;
                b[axis] = tick;
                let Some((a, b)) = clipped_line(
                    project(self.camera, layout.plane.point(a), size),
                    project(self.camera, layout.plane.point(b), size),
                    size,
                ) else {
                    continue;
                };
                let point = if a.y > b.y { a } else { b };
                // The text baseline follows the projected grid axis.
                let mut along = [0., 0., 0.];
                along[layout.plane.axes()[axis]] = layout.step;
                let origin = project(self.camera, [0., 0., 0.], size);
                let direction = project(self.camera, along, size);
                let mut angle = (direction.y - origin.y).atan2(direction.x - origin.x);
                if angle > std::f32::consts::FRAC_PI_2 {
                    angle -= std::f32::consts::PI;
                }
                if angle < -std::f32::consts::FRAC_PI_2 {
                    angle += std::f32::consts::PI;
                }
                if previous.is_some_and(|p| (p.x - point.x).hypot(p.y - point.y) < 32.) {
                    continue;
                }
                previous = Some(point);
                frame.with_save(|frame| {
                    frame.translate(iced::Vector::new(point.x, point.y));
                    frame.rotate(angle);
                    frame.scale_nonuniform(iced::Vector::new(1., 0.8));
                    frame.fill_text(canvas::Text {
                        content: tick_text(tick, layout.step),
                        position: iced::Point::new(2., -12.),
                        color: Color::from_rgb(0.63, 0.68, 0.73),
                        size: 11.0.into(),
                        ..Default::default()
                    });
                });
            }
        }
        frame.fill_text(canvas::Text {
            content: format!(
                "{} grid · {} mm",
                layout.plane.name(),
                tick_text(layout.step, layout.step)
            ),
            position: iced::Point::new((size.width - 155.).max(8.), (size.height - 17.).max(8.)),
            color: Color::from_rgb(0.63, 0.68, 0.73),
            size: 11.0.into(),
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_uses_machine_travel_and_adapts_to_zoom() {
        let mut camera = OrbitCamera::framed([-50., -50., 0.], [50., 50., 0.]);
        let size = iced::Size::new(800., 600.);
        let grid = GridLayout::new(camera, size, [400., 200., 150.]);
        assert_eq!(grid.min, [-200., -100.]);
        assert_eq!(grid.max, [200., 100.]);
        assert!(grid.ticks(0).contains(&0.));
        assert!(grid.vertices().iter().all(|v| v.position[2] == 0.));
        camera.zoom = 3.;
        let zoomed = GridLayout::new(camera, size, [400., 200., 150.]);
        assert!(zoomed.step < grid.step);
        assert_eq!(zoomed.min, grid.min);
        assert!(zoomed.ticks(0).len() <= 101);
        camera.zoom = 100.;
        let close = GridLayout::new(camera, size, [400., 200., 150.]);
        assert!(close.step < zoomed.step);
        assert!(close.ticks(0).len() <= 101);
        assert_eq!(tick_text(-0.2, 0.1), "-0.2");
    }
    #[test]
    fn cardinal_side_views_use_the_matching_vertical_grid() {
        use std::f32::consts::{FRAC_PI_2, PI};
        let mut camera = OrbitCamera::framed([-50., -50., 0.], [50., 50., 0.]);
        let size = iced::Size::new(800., 600.);
        for (yaw, expected, normal, half) in [
            (0., GridPlane::Xz, 1, [200., 75.]),
            (PI, GridPlane::Xz, 1, [200., 75.]),
            (FRAC_PI_2, GridPlane::Yz, 0, [100., 75.]),
            (-FRAC_PI_2, GridPlane::Yz, 0, [100., 75.]),
        ] {
            camera.orient = cam_render::orientation(yaw, -FRAC_PI_2);
            let grid = GridLayout::new(camera, size, [400., 200., 150.]);
            assert_eq!(grid.plane, expected);
            assert_eq!(grid.max, half);
            assert!(grid.ticks(1).contains(&0.));
            assert!(grid.vertices().iter().all(|v| v.position[normal] == 0.));
            camera.zoom = 3.;
            assert!(GridLayout::new(camera, size, [400., 200., 150.]).step < grid.step);
            camera.zoom = 0.;
        }
        camera.orient = cam_render::orientation(0., -FRAC_PI_2 + 0.01);
        assert_eq!(GridPlane::for_camera(camera), GridPlane::Xy);
        camera.orient = cam_render::orientation(0.3, 0.8);
        assert_eq!(GridPlane::for_camera(camera), GridPlane::Xy);
        camera.orient = cam_render::orientation(0., 0.);
        assert_eq!(GridPlane::for_camera(camera), GridPlane::Xy);
        camera.orient = cam_render::orientation(0., PI);
        assert_eq!(GridPlane::for_camera(camera), GridPlane::Xy);
    }
    #[test]
    fn clipping_keeps_labels_inside_the_viewport() {
        let (a, b) = clipped_line(
            iced::Point::new(-50., 100.),
            iced::Point::new(900., 100.),
            iced::Size::new(800., 600.),
        )
        .unwrap();
        assert_eq!(a.x, 18.);
        assert_eq!(b.x, 782.);
    }
}
