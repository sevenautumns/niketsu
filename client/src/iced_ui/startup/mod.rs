use getset::{MutGetters, Setters};
use iced::widget::{
    column, row, Button, Checkbox, Column, Container, Scrollable, Space, Text, TextInput,
};
use iced::{Element, Renderer};
use iced_native::alignment::Horizontal;
use iced_native::{Alignment, Command, Length, Theme};
use log::warn;

use self::message::{
    AddPath, DeletePath, PasswordInput, PathInput, RoomInput, SecureCheckbox, StartWindowMessage,
    UrlInput, UsernameInput,
};
use super::ui::{IcedUITrait, IcedUiMessage};
use crate::config::{Config, RgbWrap};
use crate::styling::{ColorButton, FileButton};
use crate::TEXT_SIZE;

pub(super) mod message;

const SPACING: u16 = 10;

#[derive(Debug, Clone, Setters, MutGetters)]
#[getset(set, get_mut)]
pub(super) struct StartWindow {
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

impl StartWindow {}

impl IcedUITrait for StartWindow {
    fn view<'a>(&self) -> Element<'a, IcedUiMessage, Renderer<Theme>> {
        // let text_size = *TEXT_SIZE.load_full();

        // let file_paths: Vec<_> = self
        //     .media_dirs
        //     .iter()
        //     .enumerate()
        //     .map(|(i, d)| {
        //         row!(
        //             TextInput::new("Filepath", d)
        //                 .on_input(move |p| StartWindowMessage::from(PathInput(i, p)).into()),
        //             Button::new(
        //                 Container::new(Text::new("-"))
        //                     .center_x()
        //                     .width(Length::Fill)
        //             )
        //             .style(ColorButton::theme(self.theme().palette().danger))
        //             .on_press(StartWindowMessage::from(DeletePath(i)).into())
        //             .width(text_size * 2.0),
        //         )
        //         .spacing(SPACING)
        //         .into()
        //     })
        //     .collect();

        // let column = column!(
        //     Text::new("Niketsu")
        //         .size(text_size + 75.0)
        //         .horizontal_alignment(Horizontal::Center),
        //     Space::with_height(text_size),
        //     Text::new("General").size(text_size + 15.0),
        //     row!(
        //         column!(
        //             Button::new("Server Address").style(FileButton::theme(false, true)),
        //             Button::new("Password").style(FileButton::theme(false, true)),
        //             Button::new("Username").style(FileButton::theme(false, true)),
        //             Button::new("Room").style(FileButton::theme(false, true)),
        //         )
        //         .spacing(SPACING)
        //         .width(Length::Shrink),
        //         column!(
        //             row!(
        //                 TextInput::new("Server Address", &self.url)
        //                     .on_input(|u| StartWindowMessage::from(UrlInput(u)).into()),
        //                 Container::new(
        //                     Checkbox::new("Secure", self.secure, |b| StartWindowMessage::from(
        //                         SecureCheckbox(b)
        //                     )
        //                     .into())
        //                     .spacing(SPACING),
        //                 )
        //                 .center_y()
        //                 .height(text_size + 10.0),
        //             )
        //             .spacing(SPACING),
        //             TextInput::new("Password", &self.password)
        //                 .on_input(|u| StartWindowMessage::from(PasswordInput(u)).into())
        //                 .password(),
        //             TextInput::new("Username", &self.username)
        //                 .on_input(|u| StartWindowMessage::from(UsernameInput(u)).into()),
        //             TextInput::new("Room", &self.room)
        //                 .on_input(|u| StartWindowMessage::from(RoomInput(u)).into()),
        //         )
        //         .spacing(SPACING)
        //         .width(Length::Fill),
        //     )
        //     .spacing(SPACING),
        //     Space::with_height(text_size),
        //     Text::new("Directories").size(text_size + 15.0),
        //     column!(
        //         Column::with_children(file_paths).spacing(SPACING),
        //         Button::new(
        //             Container::new(Text::new("+"))
        //                 .center_x()
        //                 .width(Length::Fill)
        //         )
        //         .on_press(StartWindowMessage::from(AddPath).into())
        //         .width(Length::Fill),
        //     )
        //     .spacing(SPACING),
        //     Space::with_height(text_size),
        //     Text::new("Theme").size(text_size + 15.0),
        //     row!(
        //         self.theme_text_column(),
        //         self.theme_input_column(),
        //         self.theme_reset_column(),
        //     )
        //     .spacing(SPACING),
        //     Space::with_height(text_size),
        //     Button::new(
        //         Text::new("Start")
        //             .width(Length::Fill)
        //             .horizontal_alignment(iced::alignment::Horizontal::Center),
        //     )
        //     .width(Length::Fill)
        //     .on_press(StartButton.into())
        // )
        // .align_items(Alignment::Center)
        // .width(Length::Fill)
        // .max_width(500)
        // .spacing(SPACING)
        // .padding(SPACING);

        // Container::new(Scrollable::new(
        //     Container::new(column)
        //         .padding(5)
        //         .center_x()
        //         .width(Length::Fill),
        // ))
        // .height(Length::Fill)
        // .padding(SPACING)
        // .center_y()
        // .into()
        todo!()
    }

    fn update(&mut self, message: IcedUiMessage) -> Command<IcedUiMessage> {
        let IcedUiMessage::Start(msg) = message else {
            warn!("unexpected message during startup: {message:?}");
            return Command::none();
        };

        todo!()
    }

    fn theme(&self) -> Theme {
        self.config.theme()
    }
}

impl From<Config> for StartWindow {
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

impl From<StartWindow> for Config {
    fn from(ui: StartWindow) -> Self {
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
