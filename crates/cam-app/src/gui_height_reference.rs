//! Icon-labelled height references in a grouped, scrollable popup.
use super::*;
use cam_model::{HeightPlaneKind as Plane, HeightReference as Reference};
use iced::advanced::{layout, mouse, overlay, renderer, widget, Clipboard, Layout, Shell, Widget};
use iced::widget::column;
use iced::{Event, Point, Rectangle, Size, Vector};

fn menu_id(plane: Plane) -> widget::Id {
    widget::Id::from(format!("height-reference-menu-{plane:?}"))
}

fn group(reference: Reference) -> u8 {
    match reference {
        Reference::Plane(_) | Reference::Disabled => 0,
        Reference::ModelTop
        | Reference::ModelBottom
        | Reference::StockTop
        | Reference::StockBottom
        | Reference::FixtureTop
        | Reference::FixtureBottom
        | Reference::SelectedContours => 1,
        Reference::Selection | Reference::Origin => 2,
        Reference::Highest | Reference::Lowest => 3,
    }
}

fn icon_bytes(reference: Reference) -> &'static [u8] {
    match reference {
        Reference::Plane(Plane::Clearance) => {
            include_bytes!("../assets/icons/heights/clearance.svg")
        }
        Reference::Plane(Plane::Retract) => include_bytes!("../assets/icons/heights/retract.svg"),
        Reference::Plane(Plane::Feed) => include_bytes!("../assets/icons/heights/feed.svg"),
        Reference::Plane(Plane::Top) => include_bytes!("../assets/icons/heights/top.svg"),
        Reference::Plane(Plane::Bottom) => include_bytes!("../assets/icons/heights/bottom.svg"),
        Reference::Disabled => include_bytes!("../assets/icons/heights/disabled.svg"),
        Reference::ModelTop => include_bytes!("../assets/icons/heights/model_top.svg"),
        Reference::ModelBottom => include_bytes!("../assets/icons/heights/model_bottom.svg"),
        Reference::StockTop => include_bytes!("../assets/icons/heights/stock_top.svg"),
        Reference::StockBottom => include_bytes!("../assets/icons/heights/stock_bottom.svg"),
        Reference::FixtureTop => include_bytes!("../assets/icons/heights/fixture_top.svg"),
        Reference::FixtureBottom => include_bytes!("../assets/icons/heights/fixture_bottom.svg"),
        Reference::SelectedContours => include_bytes!("../assets/icons/heights/contours.svg"),
        Reference::Selection => include_bytes!("../assets/icons/heights/selection.svg"),
        Reference::Origin => include_bytes!("../assets/icons/heights/origin.svg"),
        Reference::Highest => include_bytes!("../assets/icons/heights/highest.svg"),
        Reference::Lowest => include_bytes!("../assets/icons/heights/lowest.svg"),
    }
}

fn label<M: 'static>(reference: Reference) -> Element<'static, M> {
    row![
        iced::widget::svg(iced::widget::svg::Handle::from_memory(icon_bytes(
            reference
        )))
        .width(22)
        .height(22),
        text(reference.to_string()).size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn picker(plane: Plane, selected: Reference) -> Element<'static, Message> {
    let trigger = container(
        row![label(selected), text("▾").size(12).color(INSPECTOR_MUTED),]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([5, 9])
    .width(Length::Fill)
    .style(|_| {
        let style = inspector_picker(&iced::Theme::Dark, pick_list::Status::Active);
        container::Style {
            background: Some(style.background),
            text_color: Some(style.text_color),
            border: style.border,
            ..Default::default()
        }
    });
    Element::new(HeightPicker {
        plane,
        selected,
        trigger: trigger.into(),
    })
}

struct HeightPicker {
    plane: Plane,
    selected: Reference,
    trigger: Element<'static, Message>,
}

#[derive(Default)]
struct State {
    open: bool,
    active: usize,
    reveal_active: bool,
}

impl Widget<Message, iced::Theme, iced::Renderer> for HeightPicker {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.trigger), widget::Tree::empty()]
    }
    fn diff(&self, tree: &mut widget::Tree) {
        tree.children[0].diff(&self.trigger);
    }
    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && cursor.is_over(layout.bounds())
        {
            state.open = !state.open;
            state.reveal_active = true;
            state.active = Reference::options(self.plane)
                .iter()
                .position(|r| *r == self.selected)
                .unwrap_or(0);
            shell.capture_event();
            shell.invalidate_layout();
        }
    }
    fn mouse_interaction(
        &self,
        _: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Rectangle,
        _: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout<'a>,
        _: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, iced::Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.open {
            return None;
        }
        let options = Reference::options(self.plane);
        let mut rows = column![].spacing(2);
        let mut previous = None;
        for (index, reference) in options.iter().copied().enumerate() {
            let current = group(reference);
            if previous.is_some_and(|g| g != current) {
                rows = rows.push(popup_separator());
            }
            previous = Some(current);
            let active = index == state.active;
            rows = rows.push(
                button(label(reference))
                    .on_press(reference)
                    .width(Length::Fill)
                    .padding([3, 7])
                    .style(move |theme, status| popup_item_style(theme, status, active)),
            );
        }
        let content: Element<'static, Reference> =
            container(scrollable(rows.padding(4)).id(menu_id(self.plane)))
                .width(Length::Fill)
                .style(popup_surface)
                .into();
        tree.children[1].diff(&content);
        Some(overlay::Element::new(Box::new(Menu {
            plane: self.plane,
            options,
            state,
            content,
            tree: &mut tree.children[1],
            anchor: layout.bounds() + translation,
            viewport: *viewport,
        })))
    }
}

struct Menu<'a> {
    plane: Plane,
    options: Vec<Reference>,
    state: &'a mut State,
    content: Element<'static, Reference>,
    tree: &'a mut widget::Tree,
    anchor: Rectangle,
    viewport: Rectangle,
}

impl overlay::Overlay<Message, iced::Theme, iced::Renderer> for Menu<'_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let below = (bounds.height - self.anchor.y - self.anchor.height - 4.).max(0.);
        let above = (self.anchor.y - 4.).max(0.);
        let width = self.anchor.width.max(218.).min(bounds.width);
        let limits =
            layout::Limits::new(Size::ZERO, Size::new(width, below.max(above))).width(width);
        let node = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let node = node.move_to(Point::new(
            self.anchor.x.min(bounds.width - size.width).max(0.),
            if size.height <= below {
                self.anchor.y + self.anchor.height + 4.
            } else {
                (self.anchor.y - size.height - 4.).max(0.)
            },
        ));
        if self.state.reveal_active {
            // Keep the initial selection and keyboard navigation visible even in
            // a short popup. Pointer scrolling leaves the offset untouched.
            let mut reveal = widget::operation::scrollable::snap_to::<()>(
                menu_id(self.plane),
                widget::operation::scrollable::RelativeOffset {
                    x: None,
                    y: Some(self.state.active as f32 / (self.options.len() - 1) as f32),
                },
            );
            self.content.as_widget_mut().operate(
                self.tree,
                Layout::new(&node),
                renderer,
                &mut reveal,
            );
            self.state.reveal_active = false;
        }
        node
    }
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        use iced::keyboard::{key::Named, Key};
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(key),
            ..
        }) = event
        {
            match key {
                Named::Escape | Named::Tab => self.state.open = false,
                Named::ArrowUp => {
                    self.state.active =
                        (self.state.active + self.options.len() - 1) % self.options.len()
                }
                Named::ArrowDown => {
                    self.state.active = (self.state.active + 1) % self.options.len()
                }
                Named::Home => self.state.active = 0,
                Named::End => self.state.active = self.options.len() - 1,
                Named::Enter | Named::Space => {
                    shell.publish(Message::HeightReferenceChanged(
                        self.plane,
                        self.options[self.state.active],
                    ));
                    self.state.open = false;
                }
                _ => return,
            }
            self.state.reveal_active = true;
            if *key != Named::Tab {
                shell.capture_event();
            }
            shell.invalidate_layout();
            return;
        }
        if matches!(event, Event::Window(iced::window::Event::Unfocused))
            || matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
                && !cursor.is_over(layout.bounds())
        {
            self.state.open = false;
            shell.capture_event();
            shell.invalidate_layout();
            return;
        }
        let mut messages = Vec::new();
        let mut local = Shell::new(&mut messages);
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            &layout.bounds(),
        );
        if !local.is_empty() {
            self.state.open = false;
            local.invalidate_layout();
        }
        shell.merge(local, |reference| {
            Message::HeightReferenceChanged(self.plane, reference)
        });
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &self.viewport,
            renderer,
        )
    }
    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_height_reference_icons_are_valid_square_svgs() {
        let references = Reference::options(Plane::Feed)
            .into_iter()
            .chain([Reference::Plane(Plane::Feed)]);
        for reference in references {
            let source = std::str::from_utf8(icon_bytes(reference)).unwrap();
            let document = roxmltree::Document::parse(source).unwrap();
            let root = document.root_element();
            assert_eq!(root.tag_name().name(), "svg", "{reference}");
            assert_eq!(root.attribute("viewBox"), Some("0 0 24 24"), "{reference}");
            assert!(root.children().any(|node| node.is_element()), "{reference}");
        }
    }
}
