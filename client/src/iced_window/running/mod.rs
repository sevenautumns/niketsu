use std::ops::Deref;

use getset::{Getters, MutGetters};
use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{column, row, Button, Container, Scrollable, Text, TextInput};
use iced::{Element, Length, Padding, Renderer, Theme};

use self::message::{ReadyButton, UserEvent};
use super::{InnerApplication, MainMessage};
use crate::client::Core;
use crate::config::Config;
use crate::iced_window::running::message::{MessageInput, SendMessage};
use crate::playlist::PlaylistWidget;
use crate::rooms::RoomsWidget;
use crate::styling::{ContainerBorder, ResultButton};

pub mod message;

#[derive(Debug, Getters, MutGetters)]
#[getset(get = "pub")]
pub struct RunningWindow {
    client: Core,
    config: Config,
    #[getset(get_mut = "pub")]
    message: String,
    #[getset(get_mut = "pub")]
    messages_scroll: RelativeOffset,
}

impl RunningWindow {
    pub fn new(client: Core, config: Config) -> Self {
        RunningWindow {
            client,
            message: Default::default(),
            config,
            messages_scroll: RelativeOffset::END,
        }
    }
}

impl InnerApplication for RunningWindow {
    fn view<'a>(&self) -> Element<'a, MainMessage, Renderer<Theme>> {
        let mut btn;
        let client = self.client();
        match client.user().load().ready() {
            true => {
                btn = Button::new(
                    Text::new("Ready")
                        .width(Length::Fill)
                        .horizontal_alignment(iced::alignment::Horizontal::Center),
                )
                .style(ResultButton::ready())
            }
            false => {
                btn = Button::new(
                    Text::new("Not Ready")
                        .width(Length::Fill)
                        .horizontal_alignment(iced::alignment::Horizontal::Center),
                )
                .style(ResultButton::not_ready())
            }
        }
        btn = btn.on_press(UserEvent::from(ReadyButton).into());

        row!(
            column!(
                client.messages().view(self.theme()),
                row!(
                    TextInput::new("Message", &self.message)
                        .width(Length::Fill)
                        .on_input(|m| UserEvent::from(MessageInput(m)).into())
                        .on_submit(UserEvent::from(SendMessage).into()),
                    Button::new("Send").on_press(UserEvent::from(SendMessage).into())
                )
                .spacing(5.0)
            )
            .spacing(5.0)
            .width(Length::Fill)
            .height(Length::Fill),
            column!(
                client.db().view(),
                Container::new(RoomsWidget::new(
                    &client.rooms().load(),
                    &client.user().load(),
                    &self.theme()
                ))
                .style(ContainerBorder::basic())
                .padding(5.0)
                .width(Length::Fill)
                .height(Length::Fill),
                Container::new(
                    Scrollable::new(PlaylistWidget::new(
                        client.playlist().load().deref().deref().clone(),
                        client.playing_file(),
                        &client.db()
                    ))
                    .width(Length::Fill)
                    .id(Id::new("playlist"))
                )
                .style(ContainerBorder::basic())
                .padding(5.0)
                .height(Length::Fill),
                btn.width(Length::Fill)
            )
            .width(Length::Fill)
            .spacing(5.0)
        )
        .spacing(5.0)
        .padding(Padding::new(5.0))
        .into()
    }

    fn config(&self) -> &Config {
        &self.config
    }
}
