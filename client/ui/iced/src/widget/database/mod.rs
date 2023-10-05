use iced::event::Status;
use iced::mouse::Cursor;
use iced::widget::{Button, Container, ProgressBar, Row, Text, Tooltip};
use iced::{Element, Event, Length, Rectangle, Renderer, Theme};
use niketsu_core::file_database::FileStore;

use self::message::{DatabaseWidgetMessage, StartDbUpdate, StopDbUpdate};
use crate::message::Message;
use crate::styling::{ContainerBorder, FileButton, FileProgressBar, ResultButton};
use crate::TEXT_SIZE;

pub mod message;

pub struct DatabaseWidget<'a> {
    base: Element<'a, Message>,
}

impl<'a> DatabaseWidget<'a> {
    pub fn new(state: &DatabaseWidgetState) -> Self {
        let finished = 1.0 == state.ratio;
        let main: Element<_, _> = match finished {
            true => {
                let len = state.database.len();
                Container::new(
                    Button::new(Text::new(format!("{len} files in database")))
                        .style(FileButton::theme(false, true)),
                )
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(ContainerBorder::basic())
                .width(Length::Fill)
                .into()
            }
            false => ProgressBar::new(0.0..=1.0, state.ratio)
                .style(FileProgressBar::theme(finished))
                // Text size + 2 times default button padding
                .height(Length::Fixed(*TEXT_SIZE.load_full() + 16.0))
                .into(),
        };

        let update_msg = match finished {
            true => DatabaseWidgetMessage::from(StartDbUpdate).into(),
            false => DatabaseWidgetMessage::from(StopDbUpdate).into(),
        };
        let update_btn = match finished {
            true => Button::new("Update"),
            false => Button::new("Stop"),
        }
        .on_press(update_msg)
        .style(ResultButton::theme(finished));

        let update_text = match finished {
            true => "Update file database",
            false => "Stop update of file database",
        };
        let update_tooltip: Element<_, _> = Tooltip::new(
            update_btn,
            update_text,
            iced::widget::tooltip::Position::Bottom,
        )
        .into();
        let base = Row::new()
            .push(main)
            .push(update_tooltip)
            .spacing(5.0)
            .into();
        Self { base }
    }
}

impl<'a> iced::advanced::Widget<Message, Renderer> for DatabaseWidget<'a> {
    fn width(&self) -> Length {
        self.base.as_widget().width()
    }

    fn height(&self) -> Length {
        self.base.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base.as_widget().layout(renderer, limits)
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> iced::mouse::Interaction {
        self.base.as_widget().mouse_interaction(
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
        event: Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        self.base.as_widget_mut().on_event(
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
}

impl<'a> From<DatabaseWidget<'a>> for Element<'a, Message> {
    fn from(db: DatabaseWidget<'a>) -> Self {
        Self::new(db)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DatabaseWidgetState {
    database: FileStore,
    ratio: f32,
}

impl DatabaseWidgetState {
    pub fn update_file_store(&mut self, store: FileStore) {
        self.database = store;
    }

    pub fn update_progress(&mut self, ratio: f32) {
        self.ratio = ratio;
    }
}
