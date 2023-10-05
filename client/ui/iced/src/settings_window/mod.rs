use iced::alignment::Horizontal;
use iced::widget::{
    row, Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput,
};
use iced::{Alignment, Element, Length, Theme};
use niketsu_core::config::Config as CoreConfig;

use self::message::{
    AddPath, BackgroundColorInput, DangerColorInput, DeletePath, PasswordInput, PathInput,
    PrimaryColorInput, RoomInput, SecureCheckbox, SettingsMessage, SettingsMessageTrait,
    SuccessColorInput, TextColorInput, UrlInput, UsernameInput,
};
use super::message::Message;
use super::view::{SubWindowTrait, ViewModel};
use super::UiModel;
use crate::config::{
    default_background, default_danger, default_primary, default_success, default_text, Config,
    RgbWrap,
};
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

impl SubWindowTrait for SettingsView {
    type SubMessage = SettingsMessage;

    fn view(&self, model: &ViewModel) -> Element<Message> {
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
                    .style(ColorButton::theme(model.config().theme().palette().danger))
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
            .push(Text::new("Theme").size(text_size + 15.0))
            .push(
                Row::new()
                    .push(self.theme_text_column())
                    .push(self.theme_input_column())
                    .push(self.theme_reset_column())
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
    pub fn new(config: Config, core_config: CoreConfig) -> Self {
        Self {
            username: core_config.username,
            media_dirs: core_config.media_dirs,
            url: core_config.url,
            room: core_config.room,
            password: core_config.password,
            secure: core_config.secure,
            auto_login: core_config.auto_login,
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

    pub fn into_config(self) -> (Config, CoreConfig) {
        let conf = Config {
            text_size: self.text_size,
            background_color: self.background_color,
            text_color: self.text_color,
            primary_color: self.primary_color,
            success_color: self.success_color,
            danger_color: self.danger_color,
        };
        let core = CoreConfig {
            username: self.username,
            media_dirs: self.media_dirs,
            url: self.url,
            room: self.room,
            password: self.password,
            secure: self.secure,
            auto_login: self.auto_login,
        };
        (conf, core)
    }
}

impl SettingsView {
    fn theme_text_column<'a>(&self) -> Element<'a, Message> {
        Column::new()
            .push(Button::new("Text").style(FileButton::theme(false, true)))
            .push(Button::new("Background").style(FileButton::theme(false, true)))
            .push(Button::new("Primary").style(FileButton::theme(false, true)))
            .push(Button::new("Success").style(FileButton::theme(false, true)))
            .push(Button::new("Danger").style(FileButton::theme(false, true)))
            .spacing(SPACING)
            .width(Length::Shrink)
            .into()
    }

    fn theme_input_column<'a>(&self) -> Element<'a, Message, iced::Renderer<Theme>> {
        Column::new()
            .push(
                TextInput::new("Text Color", &self.text_color_input)
                    .on_input(|c| SettingsMessage::from(TextColorInput(c)).into()),
            )
            .push(
                TextInput::new("Background Color", &self.background_color_input)
                    .on_input(|c| SettingsMessage::from(BackgroundColorInput(c)).into()),
            )
            .push(
                TextInput::new("Primary Color", &self.primary_color_input)
                    .on_input(|c| SettingsMessage::from(PrimaryColorInput(c)).into()),
            )
            .push(
                TextInput::new("Success Color", &self.success_color_input)
                    .on_input(|c| SettingsMessage::from(SuccessColorInput(c)).into()),
            )
            .push(
                TextInput::new("Danger Color", &self.danger_color_input)
                    .on_input(|c| SettingsMessage::from(DangerColorInput(c)).into()),
            )
            .spacing(SPACING)
            .width(Length::Fill)
            .into()
    }

    fn theme_reset_column<'a>(&self) -> Element<'a, Message> {
        let text_size = *TEXT_SIZE.load_full();
        Column::new()
            .push(
                Button::new(" ")
                    .style(ColorButton::theme(self.text_color.into()))
                    .on_press(
                        SettingsMessage::from(TextColorInput(default_text().to_string())).into(),
                    )
                    .width(text_size * 2.0),
            )
            .push(
                Button::new(" ")
                    .style(ColorButton::theme(self.background_color.into()))
                    .on_press(
                        SettingsMessage::from(BackgroundColorInput(
                            default_background().to_string(),
                        ))
                        .into(),
                    )
                    .width(text_size * 2.0),
            )
            .push(
                Button::new(" ")
                    .style(ColorButton::theme(self.primary_color.into()))
                    .on_press(
                        SettingsMessage::from(PrimaryColorInput(default_primary().to_string()))
                            .into(),
                    )
                    .width(text_size * 2.0),
            )
            .push(
                Button::new(" ")
                    .style(ColorButton::theme(self.success_color.into()))
                    .on_press(
                        SettingsMessage::from(SuccessColorInput(default_success().to_string()))
                            .into(),
                    )
                    .width(text_size * 2.0),
            )
            .push(
                Button::new(" ")
                    .style(ColorButton::theme(self.danger_color.into()))
                    .on_press(
                        SettingsMessage::from(DangerColorInput(default_danger().to_string()))
                            .into(),
                    )
                    .width(text_size * 2.0),
            )
            .spacing(SPACING)
            .width(Length::Shrink)
            .into()
    }
}
