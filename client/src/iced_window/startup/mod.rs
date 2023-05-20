use getset::{MutGetters, Setters};
use iced::alignment::Horizontal;
use iced::widget::{
    column, row, Button, Checkbox, Column, Container, Scrollable, Space, Text, TextInput,
};
use iced::{Alignment, Element, Length, Theme};
use message::StartUIMessage;

use self::message::*;
use super::InnerApplication;
use crate::config::{
    default_background, default_danger, default_primary, default_success, default_text, Config,
    RgbWrap,
};
use crate::iced_window::MainMessage;
use crate::styling::{ColorButton, FileButton};
use crate::TEXT_SIZE;

pub mod message;

#[derive(Debug, Clone, Setters, MutGetters)]
#[getset(set, get_mut)]
pub struct StartUI {
    #[getset(skip)]
    config: Config,

    username: String,
    media_dirs: Vec<String>,
    url: String,
    secure: bool,
    room: String,
    password: String,

    text_size: f32,
    background_color: RgbWrap,
    background_color_input: String,
    text_color: RgbWrap,
    text_color_input: String,
    primary_color: RgbWrap,
    primary_color_input: String,
    success_color: RgbWrap,
    success_color_input: String,
    danger_color: RgbWrap,
    danger_color_input: String,
}

impl From<Config> for StartUI {
    fn from(config: Config) -> Self {
        Self {
            config: config.clone(),
            username: config.username,
            media_dirs: config.media_dirs,
            url: config.url,
            room: config.room,
            password: config.password,
            text_size: config.text_size,
            background_color: config.background_color,
            background_color_input: config.background_color.to_string(),
            text_color: config.text_color,
            text_color_input: config.text_color.to_string(),
            primary_color: config.primary_color,
            primary_color_input: config.primary_color.to_string(),
            success_color: config.success_color,
            success_color_input: config.success_color.to_string(),
            danger_color: config.danger_color,
            danger_color_input: config.danger_color.to_string(),
            secure: config.secure,
        }
    }
}

impl From<StartUI> for Config {
    fn from(ui: StartUI) -> Self {
        Self {
            username: ui.username,
            media_dirs: ui.media_dirs,
            url: ui.url,
            room: ui.room,
            password: ui.password,
            text_size: ui.text_size,
            background_color: ui.background_color,
            text_color: ui.text_color,
            primary_color: ui.primary_color,
            success_color: ui.success_color,
            danger_color: ui.danger_color,
            secure: ui.secure,
        }
    }
}

impl InnerApplication for Box<StartUI> {
    fn view<'a>(&self) -> Element<'a, MainMessage, iced::Renderer<Theme>> {
        let text_size = *TEXT_SIZE.load_full();

        let file_paths: Vec<_> = self
            .media_dirs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                row!(
                    TextInput::new("Filepath", d)
                        .on_input(move |p| StartUIMessage::from(PathInput(i, p)).into()),
                    Button::new(
                        Container::new(Text::new("-"))
                            .center_x()
                            .width(Length::Fill)
                    )
                    .style(ColorButton::theme(self.theme().palette().danger))
                    .on_press(StartUIMessage::from(DeletePath(i)).into())
                    .width(text_size * 2.0),
                )
                .spacing(10)
                .into()
            })
            .collect();

        let column = column!(
            Text::new("Niketsu")
                .size(text_size + 75.0)
                .horizontal_alignment(Horizontal::Center),
            Space::with_height(text_size),
            Text::new("General").size(text_size + 15.0),
            row!(
                column!(
                    Button::new("Server Address").style(FileButton::theme(false, true)),
                    Button::new("Password").style(FileButton::theme(false, true)),
                    Button::new("Username").style(FileButton::theme(false, true)),
                    Button::new("Room").style(FileButton::theme(false, true)),
                )
                .spacing(10)
                .width(Length::Shrink),
                column!(
                    row!(
                        TextInput::new("Server Address", &self.url)
                            .on_input(|u| StartUIMessage::from(UrlInput(u)).into()),
                        Container::new(
                            Checkbox::new("Secure", self.secure, |b| StartUIMessage::from(
                                SecureCheckbox(b)
                            )
                            .into())
                            .spacing(10),
                        )
                        .center_y()
                        .height(text_size + 10.0),
                    )
                    .spacing(10),
                    TextInput::new("Password", &self.password)
                        .on_input(|u| StartUIMessage::from(PasswordInput(u)).into())
                        .password(),
                    TextInput::new("Username", &self.username)
                        .on_input(|u| StartUIMessage::from(UsernameInput(u)).into()),
                    TextInput::new("Room", &self.room)
                        .on_input(|u| StartUIMessage::from(RoomInput(u)).into()),
                )
                .spacing(10)
                .width(Length::Fill),
            )
            .spacing(10),
            Space::with_height(text_size),
            Text::new("Directories").size(text_size + 15.0),
            column!(
                Column::with_children(file_paths).spacing(10),
                Button::new(
                    Container::new(Text::new("+"))
                        .center_x()
                        .width(Length::Fill)
                )
                .on_press(StartUIMessage::from(AddPath).into())
                .width(Length::Fill),
            )
            .spacing(10),
            Space::with_height(text_size),
            Text::new("Theme").size(text_size + 15.0),
            row!(
                column!(
                    Button::new("Text").style(FileButton::theme(false, true)),
                    Button::new("Background").style(FileButton::theme(false, true)),
                    Button::new("Primary").style(FileButton::theme(false, true)),
                    Button::new("Success").style(FileButton::theme(false, true)),
                    Button::new("Danger").style(FileButton::theme(false, true)),
                )
                .spacing(10)
                .width(Length::Shrink),
                column!(
                    TextInput::new("Text Color", &self.text_color_input)
                        .on_input(|c| StartUIMessage::from(TextColorInput(c)).into()),
                    TextInput::new("Background Color", &self.background_color_input)
                        .on_input(|c| StartUIMessage::from(BackgroundColorInput(c)).into()),
                    TextInput::new("Primary Color", &self.primary_color_input)
                        .on_input(|c| { StartUIMessage::from(PrimaryColorInput(c)).into() }),
                    TextInput::new("Success Color", &self.success_color_input)
                        .on_input(|c| { StartUIMessage::from(SuccessColorInput(c)).into() }),
                    TextInput::new("Danger Color", &self.danger_color_input)
                        .on_input(|c| { StartUIMessage::from(DangerColorInput(c)).into() }),
                )
                .spacing(10)
                .width(Length::Fill),
                column!(
                    Button::new(" ")
                        .style(ColorButton::theme(self.text_color.into()))
                        .on_press(
                            StartUIMessage::from(TextColorInput(default_text().to_string())).into()
                        )
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.background_color.into()))
                        .on_press(
                            StartUIMessage::from(BackgroundColorInput(
                                default_background().to_string()
                            ))
                            .into()
                        )
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.primary_color.into()))
                        .on_press(
                            StartUIMessage::from(PrimaryColorInput(default_primary().to_string()))
                                .into()
                        )
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.success_color.into()))
                        .on_press(
                            StartUIMessage::from(SuccessColorInput(default_success().to_string()))
                                .into()
                        )
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.danger_color.into()))
                        .on_press(
                            StartUIMessage::from(DangerColorInput(default_danger().to_string()))
                                .into()
                        )
                        .width(text_size * 2.0),
                )
                .spacing(10)
                .width(Length::Shrink)
            )
            .spacing(10),
            Space::with_height(text_size),
            Button::new(
                Text::new("Start")
                    .width(Length::Fill)
                    .horizontal_alignment(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .on_press(StartButton.into())
        )
        .align_items(Alignment::Center)
        .width(Length::Fill)
        .max_width(500)
        .spacing(10)
        .padding(10);

        Container::new(Scrollable::new(
            Container::new(column)
                .padding(5)
                .center_x()
                .width(Length::Fill),
        ))
        .height(Length::Fill)
        .padding(10)
        .center_y()
        .into()
    }

    fn config(&self) -> &Config {
        &self.config
    }
}
