use std::str::FromStr;

use iced::alignment::Horizontal;
use iced::widget::{column, row, Button, Container, Scrollable, Space, Text, TextInput};
use iced::{Alignment, Element, Length};

use crate::config::{
    default_background, default_danger, default_primary, default_success, default_text, Config,
    RgbWrap,
};
use crate::styling::{ColorButton, FileButton};
use crate::window::MainMessage;
use crate::TEXT_SIZE;

#[derive(Debug, Clone)]
pub enum StartUIMessage {
    UsernameInput(String),
    UrlInput(String),
    PathInput(String),
    RoomInput(String),
    PasswordInput(String),
    TextSizeInput(f32),
    TextColorInput(String),
    BackgroundColorInput(String),
    PrimaryColorInput(String),
    SuccessColorInput(String),
    DangerColorInput(String),
    StartButton,
}

#[derive(Debug, Clone)]
pub struct StartUI {
    pub username: String,
    pub media_dir: String,
    pub url: String,
    pub room: String,
    pub password: String,

    pub text_size: f32,
    pub background_color: RgbWrap,
    pub background_color_input: String,
    pub text_color: RgbWrap,
    pub text_color_input: String,
    pub primary_color: RgbWrap,
    pub primary_color_input: String,
    pub success_color: RgbWrap,
    pub success_color_input: String,
    pub danger_color: RgbWrap,
    pub danger_color_input: String,
}

impl StartUI {
    pub fn msg(&mut self, msg: StartUIMessage) {
        match msg {
            StartUIMessage::UsernameInput(u) => self.username = u,
            StartUIMessage::UrlInput(u) => self.url = u,
            StartUIMessage::PathInput(p) => self.media_dir = p,
            StartUIMessage::RoomInput(r) => self.room = r,
            StartUIMessage::PasswordInput(p) => self.password = p,
            StartUIMessage::StartButton => {}
            StartUIMessage::TextColorInput(c) => {
                self.text_color_input = c;
                if let Ok(c) = RgbWrap::from_str(&self.text_color_input) {
                    self.text_color = c;
                }
            }
            StartUIMessage::BackgroundColorInput(c) => {
                self.background_color_input = c;
                if let Ok(c) = RgbWrap::from_str(&self.background_color_input) {
                    self.background_color = c;
                }
            }
            StartUIMessage::PrimaryColorInput(c) => {
                self.primary_color_input = c;
                if let Ok(c) = RgbWrap::from_str(&self.primary_color_input) {
                    self.primary_color = c;
                }
            }
            StartUIMessage::SuccessColorInput(c) => {
                self.success_color_input = c;
                if let Ok(c) = RgbWrap::from_str(&self.success_color_input) {
                    self.success_color = c;
                }
            }
            StartUIMessage::DangerColorInput(c) => {
                self.danger_color_input = c;
                if let Ok(c) = RgbWrap::from_str(&self.danger_color_input) {
                    self.danger_color = c;
                }
            }
            StartUIMessage::TextSizeInput(t) => self.text_size = t,
        }
    }

    pub fn view<'a>(&self) -> Element<'a, MainMessage> {
        let text_size = *TEXT_SIZE.load_full().unwrap();
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
                    Button::new("Filepath").style(FileButton::theme(false, true)),
                )
                .spacing(10)
                .width(Length::Shrink),
                column!(
                    TextInput::new("Server Address", &self.url)
                        .on_input(|u| MainMessage::StartUi(StartUIMessage::UrlInput(u))),
                    TextInput::new("Password", &self.password)
                        .on_input(|u| MainMessage::StartUi(StartUIMessage::PasswordInput(u)))
                        .password(),
                    TextInput::new("Username", &self.username)
                        .on_input(|u| MainMessage::StartUi(StartUIMessage::UsernameInput(u))),
                    TextInput::new("Room", &self.room)
                        .on_input(|u| MainMessage::StartUi(StartUIMessage::RoomInput(u))),
                    // TODO more filepaths
                    TextInput::new("Filepath", &self.media_dir)
                        .on_input(|p| MainMessage::StartUi(StartUIMessage::PathInput(p))),
                )
                .spacing(10)
                .width(Length::Fill),
            )
            .spacing(10),
            Space::with_height(text_size),
            Text::new("Theme").size(text_size + 15.0),
            row!(
                column!(
                    // Button::new("Text Size").style(FileButton::new(false, true)),
                    Button::new("Text").style(FileButton::theme(false, true)),
                    Button::new("Background").style(FileButton::theme(false, true)),
                    Button::new("Primary").style(FileButton::theme(false, true)),
                    Button::new("Success").style(FileButton::theme(false, true)),
                    Button::new("Danger").style(FileButton::theme(false, true)),
                )
                .spacing(10)
                .width(Length::Shrink),
                column!(
                    // Slider::new(1.0..=100.0, self.text_size, |t| MainMessage::StartUi(
                    //     StartUIMessage::TextSizeInput(t)
                    // ))
                    // .height(text_size + 10.0),
                    TextInput::new("Text Color", &self.text_color_input)
                        .on_input(|c| MainMessage::StartUi(StartUIMessage::TextColorInput(c))),
                    TextInput::new("Background Color", &self.background_color_input).on_input(
                        |c| MainMessage::StartUi(StartUIMessage::BackgroundColorInput(c))
                    ),
                    TextInput::new("Primary Color", &self.primary_color_input)
                        .on_input(|c| MainMessage::StartUi(StartUIMessage::PrimaryColorInput(c))),
                    TextInput::new("Success Color", &self.success_color_input)
                        .on_input(|c| MainMessage::StartUi(StartUIMessage::SuccessColorInput(c))),
                    TextInput::new("Danger Color", &self.danger_color_input)
                        .on_input(|c| MainMessage::StartUi(StartUIMessage::DangerColorInput(c))),
                )
                .spacing(10)
                .width(Length::Fill),
                column!(
                    // Button::new(
                    //     Text::new(self.text_size.to_string())
                    //         .horizontal_alignment(Horizontal::Center)
                    // )
                    // .width(text_size * 2.0)
                    // .style(FileButton::new(false, true)),
                    Button::new(" ")
                        .style(ColorButton::theme(self.text_color.into()))
                        .on_press(MainMessage::StartUi(StartUIMessage::TextColorInput(
                            default_text().to_string()
                        )))
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.background_color.into()))
                        .on_press(MainMessage::StartUi(StartUIMessage::BackgroundColorInput(
                            default_background().to_string()
                        )))
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.primary_color.into()))
                        .on_press(MainMessage::StartUi(StartUIMessage::PrimaryColorInput(
                            default_primary().to_string()
                        )))
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.success_color.into()))
                        .on_press(MainMessage::StartUi(StartUIMessage::SuccessColorInput(
                            default_success().to_string()
                        )))
                        .width(text_size * 2.0),
                    Button::new(" ")
                        .style(ColorButton::theme(self.danger_color.into()))
                        .on_press(MainMessage::StartUi(StartUIMessage::DangerColorInput(
                            default_danger().to_string()
                        )))
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
            .on_press(MainMessage::StartUi(StartUIMessage::StartButton))
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
}

impl From<Config> for StartUI {
    fn from(config: Config) -> Self {
        Self {
            username: config.username,
            media_dir: config.media_dir,
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
        }
    }
}

impl From<StartUI> for Config {
    fn from(ui: StartUI) -> Self {
        Self {
            username: ui.username,
            media_dir: ui.media_dir,
            url: ui.url,
            room: ui.room,
            password: ui.password,
            text_size: ui.text_size,
            background_color: ui.background_color,
            text_color: ui.text_color,
            primary_color: ui.primary_color,
            success_color: ui.success_color,
            danger_color: ui.danger_color,
        }
    }
}
