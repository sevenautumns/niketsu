use std::time::Instant;

use iced::widget::scrollable::Id;
use iced::widget::{Button, Column, Container, Row, Scrollable, Text, TextInput};
use iced::{Element, Length, Renderer, Theme};
use iced_futures::core::Widget;
use niketsu_core::file_database::fuzzy::{FuzzyResult, FuzzySearch};

use self::message::{Activate, Click, Close, Input, SearchFinished};
use super::overlay::ElementOverlay;
use crate::message::Message;
use crate::styling::{FileButton, ResultButton};

pub mod message;

pub struct FileSearchWidget<'a> {
    button: Element<'a, Message>,
    base: Element<'a, Message>,
    state: &'a FileSearchWidgetState,
}

impl<'a> FileSearchWidget<'a> {
    pub fn new(state: &'a FileSearchWidgetState) -> Self {
        let search_button = Button::new(
            Text::new("Files")
                .width(Length::Fill)
                .horizontal_alignment(iced::alignment::Horizontal::Center),
        )
        .on_press(Activate.into())
        .width(Length::Fill)
        .style(ResultButton::ready());

        let mut results = vec![];
        for (index, file) in state.results.iter().enumerate() {
            let pressed = index == state.cursor_index;
            // TODO add modified date
            let row = Row::new().push(Text::new(file.entry.file_name()).width(Length::Fill));
            results.push(
                Button::new(Container::new(row).padding(2))
                    .padding(0)
                    .width(Length::Fill)
                    .on_press(Click { index }.into())
                    .style(FileButton::theme(pressed, true))
                    .into(),
            );
        }
        let results = Column::with_children(results).width(Length::Fill);
        let input = TextInput::new("Search Query", &state.query)
            .on_input(|query| Input { query }.into())
            .width(Length::Fill);
        let close_button = Button::new("Close")
            .on_press(Close.into())
            .style(ResultButton::not_ready());
        let top_row = Row::new().push(input).push(close_button).spacing(5);
        let mut base = Column::new().push(top_row).padding(5);
        if !results.children().is_empty() {
            base = base
                .push(Scrollable::new(results).id(Id::new("search")))
                .spacing(5);
        }

        Self {
            button: search_button.into(),
            base: base.into(),
            state,
        }
    }
}

impl<'a> iced::advanced::Widget<Message, Renderer> for FileSearchWidget<'a> {
    fn width(&self) -> iced::Length {
        self.button.as_widget().width()
    }

    fn height(&self) -> iced::Length {
        self.button.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.button.as_widget().layout(renderer, limits)
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.button.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.button
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![
            iced::advanced::widget::Tree::new(&self.button),
            iced::advanced::widget::Tree::new(&self.base),
        ]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(&[&self.button, &self.base]);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.button.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn on_event(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) -> iced::event::Status {
        if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key_code: iced::keyboard::KeyCode::Escape,
            modifiers: _,
        }) = event
        {
            shell.publish(Close.into());
        }

        if let Some(search) = &self.state.search {
            if search.is_finished() {
                shell.publish(SearchFinished.into());
            }
        }

        self.button.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Renderer>> {
        if self.state.active {
            return Some(iced::advanced::overlay::Element::new(
                layout.position(),
                Box::new(ElementOverlay {
                    tree: &mut state.children[1],
                    content: &mut self.base,
                }),
            ));
        }
        None
    }
}

#[derive(Debug, Default)]
pub struct FileSearchWidgetState {
    query: String,
    search: Option<FuzzySearch>,
    results: Vec<FuzzyResult>,
    cursor_index: usize,
    last_click: Option<Instant>,
    active: bool,
}

impl<'a> From<FileSearchWidget<'a>> for Element<'a, Message> {
    fn from(table: FileSearchWidget<'a>) -> Self {
        Self::new(table)
    }
}
