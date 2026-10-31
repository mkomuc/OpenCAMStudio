//! Screen-space force scale; no input handler, so orbiting/picking passes through.
use super::*;

pub(super) struct ForceLegend {
    pub maximum: Option<f64>,
    pub power_w: Option<f64>,
    pub reserved_left: f32,
    pub gizmo_size: Option<f32>,
}

fn card_bounds(size: iced::Size, reserved_left: f32, gizmo_size: Option<f32>) -> iced::Rectangle {
    let right = gizmo_size
        .map(|cube| gizmo_rect(size.width, size.height, cube, GIZMO_MARGIN).0 - 12.)
        .unwrap_or(size.width - 12.)
        .max(0.);
    let width = 440_f32.min((right - reserved_left - 12.).max(0.));
    let height = (if width < 340. { 90_f32 } else { 72_f32 }).min((size.height - 16.).max(0.));
    iced::Rectangle {
        x: (right - width).max(0.),
        y: GIZMO_MARGIN.min((size.height - height).max(0.)),
        width,
        height,
    }
}

fn label(value: f64) -> String {
    if value > 0. && value < 0.1 {
        format!("{value:.1e}")
    } else {
        format!("{value:.1}")
    }
}

impl canvas::Program<Message> for ForceLegend {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let card = card_bounds(bounds.size(), self.reserved_left, self.gizmo_size);
        if card.width < 180. || card.height < 68. {
            return Vec::new();
        }
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let stacked = card.width < 340.;
        let origin = card.position()
            + if stacked {
                iced::Vector::new(14., 28.)
            } else {
                iced::Vector::new(164., 10.)
            };
        let text_color = Color::from_rgb(0.92, 0.94, 0.97);
        frame.fill_text(canvas::Text {
            content: self
                .power_w
                .map_or_else(|| "Power: —".into(), |power| format!("Power: {power:.3} W")),
            position: card.position() + iced::Vector::new(14., if stacked { 8. } else { 30. }),
            color: text_color,
            size: 12.0.into(),
            ..Default::default()
        });
        frame.fill_text(canvas::Text {
            content: "Cutting force (N)".into(),
            position: origin,
            color: text_color,
            size: 13.0.into(),
            ..Default::default()
        });
        let bar = origin + iced::Vector::new(0., 25.);
        let width = card.x + card.width - origin.x - 14.;
        let scale = self.maximum.unwrap_or(1.);
        for i in 0..128 {
            let fraction = i as f64 / 127.;
            frame.fill_rectangle(
                bar + iced::Vector::new(width * i as f32 / 128., 0.),
                iced::Size::new(width / 128. + 0.5, 13.),
                Color::from(force_color(fraction * scale, scale)),
            );
        }
        for i in 0..=4 {
            let fraction = i as f32 / 4.;
            let tick = bar + iced::Vector::new(width * fraction, 13.);
            frame.stroke(
                &canvas::Path::line(tick, tick + iced::Vector::new(0., 4.)),
                canvas::Stroke::default()
                    .with_color(text_color)
                    .with_width(1.),
            );
            frame.fill_text(canvas::Text {
                content: self
                    .maximum
                    .map_or("—".into(), |max| label(max * f64::from(fraction))),
                position: tick + iced::Vector::new(0., 6.),
                size: 10.0.into(),
                color: text_color,
                align_x: (if i == 0 {
                    iced::alignment::Horizontal::Left
                } else if i == 4 {
                    iced::alignment::Horizontal::Right
                } else {
                    iced::alignment::Horizontal::Center
                })
                .into(),
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legend_stays_in_viewport_and_preserves_small_force_labels() {
        for size in [
            iced::Size::new(1200., 700.),
            iced::Size::new(260., 180.),
            iced::Size::new(80., 60.),
        ] {
            let card = card_bounds(size, 0., Some(110.));
            assert!(card.x >= 0. && card.y >= 0.);
            assert!(card.x + card.width <= size.width && card.y + card.height <= size.height);
        }
        assert_ne!(label(0.00001), label(0.));
        let card = card_bounds(iced::Size::new(600., 400.), 300., None);
        assert!(card.x >= 312. && card.x + card.width <= 600.);
        for cube in [60., 110., 260.] {
            let size = iced::Size::new(1200., 700.);
            let card = card_bounds(size, 300., Some(cube));
            let (cube_x, cube_y, _) = gizmo_rect(size.width, size.height, cube, GIZMO_MARGIN);
            assert_eq!(card.y, cube_y);
            assert!(card.x + card.width <= cube_x - 12.);
        }
    }
    #[test]
    fn legend_does_not_capture_orbit_or_picking_input() {
        use iced::widget::canvas::Program;
        let legend = ForceLegend {
            maximum: Some(100.),
            power_w: Some(50.),
            reserved_left: 0.,
            gizmo_size: Some(110.),
        };
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800., 600.));
        let event =
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        assert!(legend
            .update(
                &mut (),
                &event,
                bounds,
                iced::mouse::Cursor::Available(iced::Point::new(700., 550.))
            )
            .is_none());
    }
}
