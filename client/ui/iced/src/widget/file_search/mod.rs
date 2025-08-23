use std::time::Instant;

use iced::advanced::Widget;
use iced::advanced::widget::Operation;
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::widget::scrollable::Id;
use iced::widget::{Button, Column, Container, Row, Scrollable, Text, TextInput, rich_text, span};
use iced::{Element, Event, Length, Renderer, Theme, Vector};
use itertools::Itertools;
use niketsu_core::file_database::FileEntry;
use niketsu_core::fuzzy::FuzzySearch;
use niketsu_core::util::FuzzyResult;

use self::message::{
    Activate, Click, Close, FileSearchWidgetMessage, Input, Insert, SearchFinished, Select,
};
use super::overlay::{ElementOverlay, ElementOverlayConfig};
use crate::message::Message;
use crate::styling::FileButton;

pub mod message;

pub struct FileSearchWidget<'a> {
    button: Element<'a, FileSearchWidgetMessage>,
    base: Element<'a, FileSearchWidgetMessage>,
    state: &'a FileSearchWidgetState,
}

impl<'a> FileSearchWidget<'a> {
    pub fn new(state: &'a FileSearchWidgetState) -> Self {
        let search_button = Button::new(
            Text::new("Files")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press(Activate.into())
        .width(Length::Fill)
        .style(iced::widget::button::success);

        let mut results = vec![];
        for (index, file) in state.results.iter().enumerate() {
            let pressed = index == state.cursor_index;
            // TODO add modified date
            let text = file
                .entry
                .file_name()
                .chars()
                .enumerate()
                .chunk_by(|(i, _)| file.hits.contains(i))
                .into_iter()
                .map(|(bold, chars)| {
                    let mut span = span(String::from_iter(chars.map(|(_, c)| c)));
                    if bold {
                        span = span.underline(true);
                    }
                    span
                })
                .collect::<Vec<_>>();
            let row = Row::new().push(rich_text(text).width(Length::Fill));
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
            .on_submit(
                Insert {
                    index: state.cursor_index,
                }
                .into(),
            )
            .id(iced::widget::text_input::Id::new("file_search_query"))
            .width(Length::Fill);
        let close_button = Button::new("Close")
            .on_press(Close.into())
            .style(iced::widget::button::danger);
        let top_row = Row::new().push(input).push(close_button).spacing(5);
        let mut base = Column::new().push(top_row).padding(5);
        if !results.children().is_empty() {
            base = base
                // TODO scroll with selection
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

impl iced::advanced::Widget<FileSearchWidgetMessage, Theme, Renderer> for FileSearchWidget<'_> {
    fn size(&self) -> iced::Size<Length> {
        self.button.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.button
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
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
        operation: &mut dyn Operation,
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
        shell: &mut iced::advanced::Shell<'_, FileSearchWidgetMessage>,
        viewport: &iced::Rectangle,
    ) -> iced::event::Status {
        if self.state.active {
            if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            )) = event
            {
                if matches!(cursor, iced::mouse::Cursor::Available(_)) {
                    shell.publish(Close.into());
                }
            }
            if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(named),
                ..
            }) = event
            {
                match named {
                    Named::ArrowUp => {
                        let index = (self.state.cursor_index + self.state.results.len() - 1)
                            .checked_rem(self.state.results.len())
                            .unwrap_or_default();
                        shell.publish(Select { index }.into());
                    }
                    Named::ArrowDown => {
                        let index = (self.state.cursor_index + 1)
                            .checked_rem(self.state.results.len())
                            .unwrap_or_default();
                        shell.publish(Select { index }.into());
                    }
                    Named::Enter => {
                        shell.publish(
                            Insert {
                                index: self.state.cursor_index,
                            }
                            .into(),
                        );
                    }
                    Named::Escape => {
                        shell.publish(Close.into());
                    }
                    _ => {}
                }
            }

            if let Some(search) = &self.state.search {
                if search.is_finished() {
                    shell.publish(SearchFinished.into());
                }
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
        _layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
        _translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, FileSearchWidgetMessage, Theme, Renderer>>
    {
        // Ignore Captures if the `Enter` key was pressed
        let event_status = Box::new(|event, status| {
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(named),
                ..
            }) = event
            {
                if matches!(named, Named::Enter) || matches!(named, Named::Escape) {
                    return iced::event::Status::Ignored;
                }
            }
            status
        });
        if self.state.active {
            return Some(iced::advanced::overlay::Element::new(Box::new(
                ElementOverlay {
                    tree: &mut state.children[1],
                    content: &mut self.base,
                    config: ElementOverlayConfig {
                        event_status,
                        ..Default::default()
                    },
                },
            )));
        }
        None
    }
}

#[derive(Debug, Default)]
pub struct FileSearchWidgetState {
    query: String,
    search: Option<FuzzySearch<FileEntry>>,
    results: Vec<FuzzyResult<FileEntry>>,
    cursor_index: usize,
    last_click: Option<Instant>,
    active: bool,
}

impl<'a> From<FileSearchWidget<'a>> for Element<'a, Message> {
    fn from(table: FileSearchWidget<'a>) -> Self {
        Element::new(table).map(Message::from)
    }
}
