use std::borrow::BorrowMut;
use std::ops::Deref;
use std::sync::Arc;

use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{column, row, Button, Container, Scrollable, Text, TextInput};
use iced::{Application, Command, Element, Length, Padding, Renderer, Theme};
use iced_native::command::Action;
use log::*;

use crate::client::{Client, UiMessage};
use crate::config::Config;
use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::fs::FileDatabase;
use crate::heartbeat::Changed;
use crate::rooms::{RoomsWidget, RoomsWidgetMessage, RoomsWidgetState};
use crate::start_ui::{StartUI, StartUIMessage};
use crate::styling::{ContainerBorder, ResultButton};
use crate::user::ThisUser;
use crate::ws::ServerMessage;
use crate::TEXT_SIZE;

#[derive(Debug)]
pub enum MainWindow {
    Startup {
        config: Config,
        ui: Box<StartUI>,
    },
    Running {
        client: Client,
        config: Config,
        message: String,
        messages_scroll: RelativeOffset,
    },
}

#[derive(Debug, Clone)]
pub enum MainMessage {
    StartUi(StartUIMessage),
    User(UserMessage),
    FileTable(PlaylistWidgetMessage),
    PlayerChanged,
    Rooms(RoomsWidgetMessage),
}

#[derive(Debug, Clone)]
pub enum UserMessage {
    ReadyButton,
    SendMessage,
    StopDbUpdate,
    StartDbUpdate,
    ScrollMessages(RelativeOffset),
    MessageInput(String),
}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = MainMessage;

    type Theme = Theme;

    type Flags = Config;

    fn new(config: Self::Flags) -> (Self, Command<Self::Message>) {
        (
            Self::Startup {
                config: config.clone(),
                ui: Box::new(StartUI::from(config)),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn theme(&self) -> Self::Theme {
        match self {
            MainWindow::Startup { config, .. } => config.theme(),
            MainWindow::Running { config, .. } => config.theme(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> Command<Self::Message> {
        match self.borrow_mut() {
            MainWindow::Startup { ui, .. } => match msg {
                MainMessage::StartUi(StartUIMessage::StartButton) => {
                    let config: Config = ui.clone().deref().clone().into();
                    TEXT_SIZE.store(Arc::new(config.text_size));
                    config.save().log();
                    match Client::new(config.clone()) {
                        Ok(client) => {
                            let notify = client.changed();
                            *self = MainWindow::Running {
                                client,
                                message: Default::default(),
                                config,
                                messages_scroll: RelativeOffset::END,
                            };
                            info!("Changed Mode to Running");
                            return Changed::next(notify);
                        }

                        Err(e) => {
                            error!("Error when creating client: {e:?}");
                            return Command::single(Action::Window(
                                iced_native::window::Action::Close,
                            ));
                        }
                    }
                }
                MainMessage::StartUi(msg) => ui.msg(msg),
                _ => todo!(),
            },
            MainWindow::Running {
                message,
                config,
                messages_scroll,
                client,
            } => match msg {
                MainMessage::FileTable(event) => match event {
                    PlaylistWidgetMessage::DoubleClick(video) => {
                        debug!("FileTable doubleclick: {video:?}");
                        client
                            .ws()
                            .send(ServerMessage::Select {
                                filename: video.as_str().to_string().into(),
                                username: client.user().load().name(),
                            })
                            .log();
                        client.send_ui_message(UiMessage::MpvSelect(video));
                    }
                    PlaylistWidgetMessage::Move(f, i) => {
                        debug!("FileTable move file: {f:?}, {i}");
                        client.playlist().rcu(|p| {
                            let mut playlist = PlaylistWidgetState::clone(p);
                            playlist.move_video(f.clone(), i);
                            playlist
                        });
                        let playlist = client
                            .playlist()
                            .load()
                            .videos()
                            .drain(..)
                            .map(|v| v.as_str().to_string())
                            .collect();
                        client
                            .ws()
                            .send(ServerMessage::Playlist {
                                playlist,
                                username: client.user().load().name(),
                            })
                            .log();
                    }
                    PlaylistWidgetMessage::Delete(f) => {
                        debug!("FileTable delete file: {f:?}");
                        let playlist = client
                            .playlist()
                            .rcu(|p| {
                                let mut playlist = PlaylistWidgetState::clone(p);
                                playlist.delete_video(&f);
                                playlist
                            })
                            .videos()
                            .drain(..)
                            .map(|v| v.as_str().to_string())
                            .collect();
                        client
                            .ws()
                            .send(ServerMessage::Playlist {
                                playlist,
                                username: client.user().load().name(),
                            })
                            .log();
                    }
                    PlaylistWidgetMessage::Interaction(f, i) => {
                        debug!("FileTable file interaction: {f:?}, {i:?}");
                        client.playlist().rcu(|p| {
                            let mut playlist = PlaylistWidgetState::clone(p);
                            playlist.file_interaction(f.clone(), i.clone());
                            playlist
                        });
                    }
                },
                MainMessage::User(event) => match event {
                    UserMessage::ReadyButton => {
                        debug!("User: ready press");
                        client.user().rcu(|u| {
                            let mut user = ThisUser::clone(u);
                            user.toggle_ready();
                            user
                        });
                        client.ws().send(client.user().load().status()).log();
                    }
                    UserMessage::SendMessage => {
                        if !message.is_empty() {
                            let msg = message.clone();
                            *message = Default::default();
                            client
                                .ws()
                                .send(ServerMessage::UserMessage {
                                    message: msg.clone(),
                                    username: client.user().load().name(),
                                })
                                .log();
                            let name = client.user().load().name();
                            client.messages().push_user_chat(msg, name);
                        }
                    }
                    UserMessage::MessageInput(msg) => *message = msg,
                    UserMessage::StartDbUpdate => {
                        trace!("Start database update request received");
                        FileDatabase::start_update(&client.db());
                        return Command::none();
                    }
                    UserMessage::StopDbUpdate => {
                        trace!("Stop database update request received");
                        client.db().stop_update()
                    }
                    UserMessage::ScrollMessages(off) => {
                        *messages_scroll = off;
                    }
                },
                MainMessage::Rooms(RoomsWidgetMessage::ClickRoom(room)) => {
                    let mut double_click = false;
                    client.rooms().rcu(|r| {
                        let mut rooms = RoomsWidgetState::clone(r);
                        double_click = rooms.click_room(room.clone());
                        rooms
                    });
                    if double_click {
                        client
                            .ws()
                            .send(ServerMessage::Join {
                                password: config.password.clone(),
                                room,
                                username: config.username.clone(),
                            })
                            .log();
                    }
                }
                MainMessage::StartUi(msg) => warn!("{msg:?}"),
                MainMessage::PlayerChanged => {
                    trace!("Player Changed");
                    let snap = if RelativeOffset::END.eq(messages_scroll) {
                        iced_native::widget::scrollable::snap_to(
                            Id::new("messages"),
                            *messages_scroll,
                        )
                    } else {
                        Command::none()
                    };
                    return Command::batch([snap, Changed::next(client.changed())]);
                }
            },
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        match self {
            MainWindow::Startup { ui, .. } => ui.view(self.theme()),
            MainWindow::Running {
                message, client, ..
            } => {
                let mut btn;
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
                btn = btn.on_press(MainMessage::User(UserMessage::ReadyButton));

                row!(
                    column!(
                        client.messages().view(self.theme()),
                        row!(
                            TextInput::new("Message", message)
                                .width(Length::Fill)
                                .on_input(|m| MainMessage::User(UserMessage::MessageInput(m)))
                                .on_submit(MainMessage::User(UserMessage::SendMessage)),
                            Button::new("Send")
                                .on_press(MainMessage::User(UserMessage::SendMessage))
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
                                client.playing(),
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
        }
    }
}

pub trait LogResult {
    fn log(&self);
}

impl<T> LogResult for anyhow::Result<T> {
    fn log(&self) {
        if let Err(e) = self {
            error!("{e:?}")
        }
    }
}
