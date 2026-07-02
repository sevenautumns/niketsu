use iced::widget::{
    Button, Column, Container, Scrollable, Space, Text, button, checkbox, column, pick_list, row,
    text, text_input,
};
use iced::{Element, Length, Theme};
use message::ThemeChange;
use niketsu_core::config::Config;

use self::message::{
    Abort, Activate, AddPath, ApplyClose, ApplyCloseSave, AutoConnectCheckbox, ConnectApplyClose,
    ConnectApplyCloseSave, DeletePath, PasswordInput, PathInput, Reset, RoomInput,
    SettingsWidgetMessage, UsernameInput,
};
use crate::TEXT_SIZE;
use crate::config::IcedConfig;
use crate::message::Message;
use crate::styling::{FileButton, ModalContainer};

pub mod message;

const SPACING: f32 = 10.0;
const MAX_WIDTH: f32 = 600.0;

pub fn open_button() -> Element<'static, Message> {
    Element::from(
        Button::new(
            Text::new("Settings")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press(SettingsWidgetMessage::from(Activate).into())
        .width(Length::Fill)
        .style(iced::widget::button::success),
    )
}

pub fn view(state: &SettingsWidgetState) -> Element<'_, Message> {
    let text_size = *TEXT_SIZE.load_full();

    let file_paths: Vec<_> = state
        .config
        .media_dirs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            row!(
                text_input("Filepath", d).on_input(move |p| PathInput(i, p).into()),
                button(Container::new("-").center_x(Length::Fill))
                    .style(iced::widget::button::danger)
                    .on_press(DeletePath(i).into())
                    .width(text_size * 2.0),
            )
            .spacing(SPACING)
            .into()
        })
        .collect();

    let column = column![
        row![
            text("Settings").size(text_size + 25.0).width(Length::Fill),
            button("Reset").on_press(Reset.into()),
            button("Close")
                .on_press(Abort.into())
                .style(iced::widget::button::danger),
        ]
        .spacing(SPACING),
        Space::new().height(text_size),
        text("General").size(text_size + 15.0).width(Length::Fill),
        row![
            column![
                button("Room").style(FileButton::theme(false, true)),
                button("Password").style(FileButton::theme(false, true)),
                button("Username").style(FileButton::theme(false, true)),
                button("Auto Connect").style(FileButton::theme(false, true)),
            ]
            .spacing(SPACING)
            .width(Length::Shrink),
            column![
                text_input("Room", &state.config.room).on_input(|u| RoomInput(u.into()).into()),
                text_input("Password", &state.config.password)
                    .on_input(|u| PasswordInput(u).into())
                    .secure(true),
                text_input("Username", &state.config.username)
                    .on_input(|u| UsernameInput(u.into()).into(),),
                Container::new(
                    checkbox(state.config.auto_connect)
                        .on_toggle(|b| AutoConnectCheckbox(b).into())
                        .spacing(SPACING),
                )
                .center_y(text_size + 15.0),
            ]
            .spacing(SPACING)
            .width(Length::Fill),
        ]
        .spacing(SPACING),
        Space::new().height(text_size),
        row![
            text("Theme").size(text_size + 15.0).width(Length::Fill),
            pick_list(Theme::ALL, Some(state.iced_config.theme.clone()), |theme| {
                ThemeChange(theme).into()
            },)
        ],
        Space::new().height(text_size),
        text("Directories")
            .size(text_size + 15.0)
            .width(Length::Fill),
        column![
            Column::with_children(file_paths).spacing(SPACING),
            button(Container::new("+").center_x(Length::Fill))
                .on_press(AddPath.into())
                .width(Length::Fill),
        ]
        .spacing(SPACING),
        Space::new().height(text_size),
        row![
            button(
                text("Apply")
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .on_press(ApplyClose.into()),
            button(
                text("Connect")
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .on_press(ConnectApplyClose.into()),
        ]
        .spacing(SPACING),
        row![
            button(
                text("Apply & Save")
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .on_press(ApplyCloseSave.into()),
            button(
                text("Connect & Save")
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .on_press(ConnectApplyCloseSave.into()),
        ]
        .spacing(SPACING),
    ]
    .align_x(iced::alignment::Horizontal::Center)
    .width(Length::Fill)
    .max_width(MAX_WIDTH)
    .spacing(SPACING)
    .padding(SPACING);

    let base: Element<'_, SettingsWidgetMessage> = Container::new(Scrollable::new(
        Container::new(column).padding(10).center_x(Length::Fill),
    ))
    .style(ModalContainer::theme)
    .padding(SPACING)
    .max_width(MAX_WIDTH)
    .center_y(Length::Shrink)
    .into();
    base.map(Message::from)
}

#[derive(Debug, Clone)]
pub struct SettingsWidgetState {
    iced_config: IcedConfig,
    config: Config,
    active: bool,
}

impl SettingsWidgetState {
    pub fn new(config: Config, iced_config: IcedConfig) -> Self {
        Self {
            iced_config,
            config,
            active: false,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn iced_config(&self) -> &IcedConfig {
        &self.iced_config
    }
}
