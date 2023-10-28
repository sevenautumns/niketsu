use iced::alignment::Horizontal;
use iced::widget::{
    row, Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput,
};
use iced::{Alignment, Element, Length, Theme};
use niketsu_core::config::Config;
use niketsu_core::ui::UiModel;

use self::message::{
    AddPath, DeletePath, PasswordInput, PathInput, RoomInput, SecureCheckbox, SettingsMessage,
    SettingsMessageTrait, UrlInput, UsernameInput,
};
use super::message::Message;
use super::view::{SubWindowTrait, ViewModel};
use crate::message::CloseSettings;
use crate::styling::{ColorButton, FileButton};
use crate::TEXT_SIZE;

pub(super) mod message;

const SPACING: u16 = 10;

#[derive(Debug, Clone)]
pub struct SettingsView {
    username: String,
    media_dirs: Vec<String>,
    // TODO validate input with url crate
    url: String,
    secure: bool,
    room: String,
    password: String,
    auto_login: bool,
}

impl SubWindowTrait for SettingsView {
    type SubMessage = SettingsMessage;

    fn view(&self, _: &ViewModel) -> Element<Message> {
        let text_size = *TEXT_SIZE.load_full();

        let file_paths: Vec<_> = self
            .media_dirs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                row!(
                    TextInput::new("Filepath", d)
                        .on_input(move |p| SettingsMessage::from(PathInput(i, p)).into()),
                    Button::new(
                        Container::new(Text::new("-"))
                            .center_x()
                            .width(Length::Fill)
                    )
                    .style(ColorButton::theme(Theme::Dark.palette().danger))
                    .on_press(SettingsMessage::from(DeletePath(i)).into())
                    .width(text_size * 2.0),
                )
                .spacing(SPACING)
                .into()
            })
            .collect();

        let column = Column::new()
            .push(
                Text::new("Niketsu")
                    .size(text_size + 75.0)
                    .horizontal_alignment(Horizontal::Center),
            )
            .push(Space::with_height(text_size))
            .push(Text::new("General").size(text_size + 15.0))
            .push(
                Row::new()
                    .push(
                        Column::new()
                            .push(
                                Button::new("Server Address").style(FileButton::theme(false, true)),
                            )
                            .push(Button::new("Password").style(FileButton::theme(false, true)))
                            .push(Button::new("Username").style(FileButton::theme(false, true)))
                            .push(Button::new("Room").style(FileButton::theme(false, true)))
                            .spacing(SPACING)
                            .width(Length::Shrink),
                    )
                    .push(
                        Column::new()
                            .push(
                                Row::new()
                                    .push(
                                        TextInput::new("Server Address", &self.url).on_input(|u| {
                                            SettingsMessage::from(UrlInput(u)).into()
                                        }),
                                    )
                                    .push(
                                        Container::new(
                                            Checkbox::new("Secure", self.secure, |b| {
                                                SettingsMessage::from(SecureCheckbox(b)).into()
                                            })
                                            .spacing(SPACING),
                                        )
                                        .center_y()
                                        .height(text_size + 10.0),
                                    )
                                    .spacing(SPACING),
                            )
                            .push(
                                TextInput::new("Password", &self.password)
                                    .on_input(|u| SettingsMessage::from(PasswordInput(u)).into())
                                    .password(),
                            )
                            .push(
                                TextInput::new("Username", &self.username)
                                    .on_input(|u| SettingsMessage::from(UsernameInput(u)).into()),
                            )
                            .push(
                                TextInput::new("Room", &self.room)
                                    .on_input(|u| SettingsMessage::from(RoomInput(u)).into()),
                            )
                            .spacing(SPACING)
                            .width(Length::Fill),
                    )
                    .spacing(SPACING),
            )
            .push(Space::with_height(text_size))
            .push(Text::new("Directories").size(text_size + 15.0))
            .push(
                Column::new()
                    .push(Column::with_children(file_paths).spacing(SPACING))
                    .push(
                        Button::new(
                            Container::new(Text::new("+"))
                                .center_x()
                                .width(Length::Fill),
                        )
                        .on_press(SettingsMessage::from(AddPath).into())
                        .width(Length::Fill),
                    )
                    .spacing(SPACING),
            )
            .push(Space::with_height(text_size))
            .push(
                Button::new(
                    Text::new("Start")
                        .width(Length::Fill)
                        .horizontal_alignment(iced::alignment::Horizontal::Center),
                )
                .width(Length::Fill)
                .on_press(CloseSettings.into()),
            )
            .align_items(Alignment::Center)
            .width(Length::Fill)
            .max_width(500)
            .spacing(SPACING)
            .padding(SPACING);

        Container::new(Scrollable::new(
            Container::new(column)
                .padding(5)
                .center_x()
                .width(Length::Fill),
        ))
        .height(Length::Fill)
        .padding(SPACING)
        .center_y()
        .into()
    }

    fn update(&mut self, message: SettingsMessage, _: &UiModel) {
        message.handle(self);
    }
}

impl SettingsView {
    pub fn new(config: Config) -> Self {
        Self {
            username: config.username,
            media_dirs: config.media_dirs,
            url: config.url,
            room: config.room,
            password: config.password,
            secure: config.secure,
            auto_login: config.auto_login,
        }
    }
}

impl From<SettingsView> for Config {
    fn from(value: SettingsView) -> Self {
        Config {
            username: value.username,
            media_dirs: value.media_dirs,
            url: value.url,
            room: value.room,
            password: value.password,
            secure: value.secure,
            auto_login: value.auto_login,
        }
    }
}
