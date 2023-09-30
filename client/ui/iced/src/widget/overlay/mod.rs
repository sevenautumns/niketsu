use iced::{BorderRadius, Color, Element, Point, Renderer, Size, Theme};

use crate::message::Message;

pub struct ElementOverlay<'a, 'b> {
    pub tree: &'b mut iced::advanced::widget::Tree,
    pub content: &'b mut Element<'a, Message, Renderer>,
}

impl<'a, 'b> iced::advanced::Overlay<Message, Renderer> for ElementOverlay<'a, 'b> {
    fn layout(
        &self,
        renderer: &Renderer,
        bounds: Size,
        _position: Point,
    ) -> iced_futures::core::layout::Node {
        let limits =
            iced::advanced::layout::Limits::new(Size::ZERO, bounds).shrink(Size::new(40.0, 40.0));
        let mut child = self.content.as_widget().layout(renderer, &limits);
        child.align(
            iced::Alignment::Center,
            iced::Alignment::Center,
            limits.max(),
        );
        child.translate(iced::Vector::new(20.0, 20.0))
    }

    fn is_over(
        &self,
        layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
        cursor_position: Point,
    ) -> bool {
        layout.bounds().expand(5.0).contains(cursor_position)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
    ) {
        <Renderer as iced::advanced::Renderer>::fill_quad(
            renderer,
            iced::advanced::renderer::Quad {
                bounds: layout.bounds().expand(5.0),
                border_radius: BorderRadius::from(5.0),
                border_width: 2.0,
                border_color: theme.palette().text,
            },
            Color {
                a: 0.98,
                ..theme.palette().background
            },
        );

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

    fn operate(
        &mut self,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.content
            .as_widget()
            .operate(self.tree, layout, renderer, operation)
    }

    fn on_event(
        &mut self,
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
    ) -> iced::event::Status {
        self.content.as_widget_mut().on_event(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        )
    }

    fn mouse_interaction(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(self.tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'c>(
        &'c mut self,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
    ) -> Option<iced::advanced::overlay::Element<'c, Message, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(self.tree, layout, renderer)
    }
}
