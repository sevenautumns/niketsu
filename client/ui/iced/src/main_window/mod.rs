use iced::advanced::widget::Operation;
use iced::event::Status;
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::mouse::Cursor;
use iced::widget::scrollable::Id;
use iced::widget::{Button, Column, Container, Row, Scrollable, Text};
use iced::{Element, Event, Length, Rectangle, Renderer, Theme};

use self::message::{MainMessage, ReadyButton};
use super::message::Message;
use super::view::ViewModel;
use super::widget::chat::ChatWidget;
use super::widget::database::DatabaseWidget;
use super::widget::playlist::PlaylistWidget;
use super::widget::rooms::RoomsWidget;
use crate::message::ToggleReady;
use crate::styling::ContainerBorder;
use crate::widget::file_search::FileSearchWidget;
use crate::widget::settings::SettingsWidget;

pub(super) mod message;

const SPACING: u16 = 5;

pub struct MainView<'a> {
    base: Element<'a, Message>,
}

impl<'a> MainView<'a> {
    pub fn new(view_model: &'a ViewModel) -> Self {
        let mut btn: Button<Message>;
        match view_model.user().ready {
            true => {
                btn = Button::new(
                    Text::new("Ready")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .style(iced::widget::button::success)
            }
            false => {
                btn = Button::new(
                    Text::new("Not Ready")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .style(iced::widget::button::danger)
            }
        }
        btn = btn.on_press(MainMessage::from(ReadyButton).into());

        let base = Row::new()
            .push(
                Column::new()
                    .push(
                        Row::new()
                            .push(SettingsWidget::new(view_model.get_settings_widget_state()))
                            .push(FileSearchWidget::new(
                                view_model.get_file_search_widget_state(),
                            ))
                            .spacing(SPACING),
                    )
                    .push(ChatWidget::new(view_model.get_chat_widget_state()))
                    .spacing(SPACING)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .push(
                Column::new()
                    .push(DatabaseWidget::new(view_model.get_database_widget_state()))
                    .push(
                        Container::new(RoomsWidget::new(
                            view_model.get_rooms_widget_state(),
                            &view_model.user(),
                        ))
                        .style(ContainerBorder::theme)
                        .padding(SPACING)
                        .width(Length::Fill)
                        .height(Length::Fill),
                    )
                    .push(
                        Container::new(
                            Scrollable::new(PlaylistWidget::new(
                                view_model.get_playlist_widget_state().clone(),
                                view_model.playing_video(),
                            ))
                            .width(Length::Fill)
                            .id(Id::new("playlist")),
                        )
                        .style(ContainerBorder::theme)
                        .padding(SPACING)
                        .height(Length::Fill),
                    )
                    .push(btn.width(Length::Fill))
                    .width(Length::Fill)
                    .spacing(SPACING),
            )
            .spacing(SPACING)
            .padding(SPACING)
            .into();
        Self { base }
    }
}

impl iced::advanced::Widget<Message, Theme, Renderer> for MainView<'_> {
    fn size(&self) -> iced::Size<Length> {
        self.base.as_widget().size()
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
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

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
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
    ) -> iced::advanced::mouse::Interaction {
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
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        let mut status = self.base.as_widget_mut().on_event(
            &mut state.children[0],
            event.clone(),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        if let Status::Ignored = status {
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: Key::Named(Named::Space),
                ..
            }) = event
            {
                shell.publish(ToggleReady.into());
                status = Status::Captured;
            }
        }

        status
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.base
            .as_widget_mut()
            .overlay(&mut state.children[0], layout, renderer, translation)
    }
}

impl<'a> From<MainView<'a>> for Element<'a, Message> {
    fn from(msgs: MainView<'a>) -> Self {
        Self::new(msgs)
    }
}
