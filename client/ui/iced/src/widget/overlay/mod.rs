use iced::advanced::widget::Operation;
use iced::{Border, Color, Element, Renderer, Size, Theme};

pub struct ElementOverlay<'a, 'b, M> {
    pub tree: &'b mut iced::advanced::widget::Tree,
    pub content: &'b mut Element<'a, M>,
    pub config: ElementOverlayConfig,
}

pub struct ElementOverlayConfig {
    pub max_height: Option<f32>,
    pub max_width: Option<f32>,
    pub min_padding: f32,
    // pub event_status: Box<dyn Fn(iced::Event, iced::event::Status) -> iced::event::Status>,
}

impl Default for ElementOverlayConfig {
    fn default() -> Self {
        Self {
            max_height: None,
            max_width: None,
            min_padding: 20.0,
            // event_status: Box::new(|_, status| status),
        }
    }
}

impl std::fmt::Debug for ElementOverlayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementOverlayConfig")
            .field("max_height", &self.max_height)
            .field("max_width", &self.max_width)
            .field("min_padding", &self.min_padding)
            .finish()
    }
}

impl<M> iced::advanced::Overlay<M, Theme, Renderer> for ElementOverlay<'_, '_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> iced::advanced::layout::Node {
        let padding = self.config.min_padding * 2.0;
        let limits = iced::advanced::layout::Limits::new(Size::ZERO, bounds)
            .max_width(self.config.max_width.unwrap_or(f32::INFINITY))
            .max_height(self.config.max_height.unwrap_or(f32::INFINITY))
            .shrink(Size::new(padding, padding));
        let mut child = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        child = child.align(
            iced::Alignment::Center,
            iced::Alignment::Center,
            limits.max(),
        );
        let offset_x = (bounds.width - limits.max().width) / 2.0;
        let offset_y = (bounds.height - limits.max().height) / 2.0;
        child.translate(iced::Vector::new(offset_x, offset_y))
    }

    // fn is_over(
    //     &self,
    //     layout: iced::advanced::Layout<'_>,
    //     _renderer: &Renderer,
    //     cursor_position: Point,
    // ) -> bool {
    //     layout.bounds().expand(5.0).contains(cursor_position)
    // }

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
                border: Border {
                    color: theme.palette().text,
                    width: 2.0,
                    radius: 5.0.into(),
                },
                shadow: Default::default(),
                snap: false,
            },
            Color {
                a: 0.99,
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
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation)
    }

    fn mouse_interaction(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let interaction = self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        );
        if interaction == iced::advanced::mouse::Interaction::None
            && cursor.is_over(layout.bounds())
        {
            iced::advanced::mouse::Interaction::Idle
        } else {
            interaction
        }
    }

    fn overlay<'a>(
        &'a mut self,
        layout: iced::advanced::Layout<'a>,
        renderer: &Renderer,
    ) -> Option<iced::advanced::overlay::Element<'a, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            self.tree,
            layout,
            renderer,
            &layout.bounds(),
            iced::Vector::ZERO,
        )
    }

    fn update(
        &mut self,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, M>,
    ) {
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        shell.request_redraw();
    }
}
