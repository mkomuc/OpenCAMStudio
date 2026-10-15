//! Dimensioned world-XY grid shared by the GPU line pass and canvas labels.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct GridLayout {
    min: [f32; 2],
    max: [f32; 2],
    pub step: f32,
}
impl GridLayout {
    pub(super) fn new(camera: OrbitCamera, size: iced::Size) -> Self {
        let aspect = size.width / size.height.max(1.0);
        let points: Vec<_> = [[-1., -1.], [-1., 1.], [1., -1.], [1., 1.]]
            .into_iter()
            .filter_map(|[u, v]| camera.pick_plane(u, v, aspect, 0.))
            .collect();
        let span = camera.half_height() * 2.;
        let center = [
            camera.target[0] + camera.pan[0],
            camera.target[1] + camera.pan[1],
        ];
        let min = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .reduce(f32::min)
                .unwrap_or(center[axis] - span)
        });
        let max = std::array::from_fn(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .reduce(f32::max)
                .unwrap_or(center[axis] + span)
        });
        let raw = camera.world_per_pixel(size.height) * 80.;
        let step = nice_step(raw.max((max[0] - min[0]).max(max[1] - min[1]) / 100.));
        Self { min, max, step }
    }
    fn ticks(&self, axis: usize) -> Vec<f32> {
        if !self.step.is_finite() || !self.min[axis].is_finite() || !self.max[axis].is_finite() {
            return vec![];
        }
        let start = (self.min[axis] / self.step).ceil();
        let count = ((self.max[axis] / self.step).floor() - start + 1.).clamp(0., 101.) as usize;
        (0..count)
            .map(|index| (start + index as f32) * self.step)
            .collect()
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
                let mut a = [self.min[0], self.min[1], 0.];
                let mut b = [self.max[0], self.max[1], 0.];
                a[axis] = tick;
                b[axis] = tick;
                scene.add_strip(vec![a, b], color);
            }
        }
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
#[derive(Clone, Copy)]
pub(super) struct GridLabels {
    pub camera: OrbitCamera,
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
        let layout = GridLayout::new(self.camera, size);
        let mut frame = canvas::Frame::new(renderer, size);
        let label = |content: String, position: iced::Point| canvas::Text {
            content,
            position,
            color: Color::from_rgb(0.63, 0.68, 0.73),
            size: 11.0.into(),
            ..Default::default()
        };
        frame.fill_text(label(
            format!("XY grid · {} mm", fmt_num(layout.step as f64)),
            iced::Point::new((size.width - 155.).max(8.), (size.height - 17.).max(8.)),
        ));
        for axis in 0..2 {
            for tick in layout.ticks(axis) {
                let mut a = [layout.min[0], layout.min[1], 0.];
                let mut b = [layout.max[0], layout.max[1], 0.];
                a[axis] = tick;
                b[axis] = tick;
                let a = project(self.camera, a, size);
                let b = project(self.camera, b, size);
                let (denominator, numerator) = if axis == 0 {
                    (b.y - a.y, size.height - 34. - a.y)
                } else {
                    (b.x - a.x, size.width - 62. - a.x)
                };
                if denominator.abs() < 0.01 {
                    continue;
                }
                let t = numerator / denominator;
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let point = iced::Point::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
                if point.x < 8.
                    || point.y < 12.
                    || point.x > size.width - 20.
                    || point.y > size.height - 20.
                {
                    continue;
                }
                frame.fill_text(label(
                    format!(
                        "{} {}",
                        if axis == 0 { "X" } else { "Y" },
                        fmt_num(tick as f64)
                    ),
                    point,
                ));
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_stays_on_xy_has_units_and_bounded_adaptive_ticks() {
        let mut camera = OrbitCamera::framed([-50., -50., 0.], [50., 50., 0.]);
        let size = iced::Size::new(800., 600.);
        let grid = GridLayout::new(camera, size);
        assert!(grid.step > 0.);
        assert!(grid.vertices().iter().all(|v| v.position[2] == 0.));
        assert!(grid.ticks(0).contains(&0.));
        camera.zoom = 3.;
        assert!(GridLayout::new(camera, size).step < grid.step);
        camera.orient = cam_render::orientation(0.7, 1.2);
        let tilted = GridLayout::new(camera, size);
        assert!(tilted.ticks(0).len() <= 101 && tilted.ticks(1).len() <= 101);
        assert!(tilted
            .vertices()
            .iter()
            .all(|v| v.position.iter().all(|n| n.is_finite())));
    }
}
