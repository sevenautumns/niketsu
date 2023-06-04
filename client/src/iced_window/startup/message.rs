use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::Command;
use iced_native::command::Action;
use log::{error, info, warn};

use super::StartUI;
use crate::client::heartbeat::Changed;
use crate::client::Core;
use crate::config::{Config, RgbWrap};
use crate::iced_window::message::IcedMessage;
use crate::iced_window::{LogResult, MainMessage, MainWindow, RunningWindow};
use crate::TEXT_SIZE;

#[enum_dispatch(StartMessage)]
#[derive(Debug, Clone)]
pub enum StartUIMessage {
    UsernameInput,
    UrlInput,
    PathInput,
    DeletePath,
    AddPath,
    RoomInput,
    PasswordInput,
    TextSizeInput,
    TextColorInput,
    BackgroundColorInput,
    PrimaryColorInput,
    SuccessColorInput,
    DangerColorInput,
    SecureCheckbox,
}

impl IcedMessage for StartUIMessage {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(ui) = win.get_start_ui() {
            StartMessage::handle(self, ui);
        } else {
            warn!("Got StartUI message outside StartUI");
        }
        Ok(Command::none())
    }
}

#[enum_dispatch]
pub trait StartMessage {
    fn handle(self, ui: &mut StartUI);
}

#[derive(Debug, Clone)]
pub struct UsernameInput(pub String);

impl StartMessage for UsernameInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_username(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct UrlInput(pub String);

impl StartMessage for UrlInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_url(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl StartMessage for PathInput {
    fn handle(self, ui: &mut StartUI) {
        if let Some(d) = ui.media_dirs_mut().get_mut(self.0) {
            *d = self.1
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl StartMessage for DeletePath {
    fn handle(self, ui: &mut StartUI) {
        let media_dirs = ui.media_dirs_mut();
        if self.0 < media_dirs.len() {
            media_dirs.remove(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl StartMessage for AddPath {
    fn handle(self, ui: &mut StartUI) {
        ui.media_dirs_mut().push(Default::default());
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub String);

impl StartMessage for RoomInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_room(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl StartMessage for PasswordInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_password(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct TextSizeInput(pub f32);

impl StartMessage for TextSizeInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_text_size(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct TextColorInput(pub String);

impl StartMessage for TextColorInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_text_color_input(self.0.clone());
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.set_text_color(c);
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundColorInput(pub String);

impl StartMessage for BackgroundColorInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_background_color_input(self.0.clone());
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.set_background_color(c);
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrimaryColorInput(pub String);

impl StartMessage for PrimaryColorInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_primary_color_input(self.0.clone());
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.set_primary_color(c);
        }
    }
}

#[derive(Debug, Clone)]
pub struct SuccessColorInput(pub String);

impl StartMessage for SuccessColorInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_success_color_input(self.0.clone());
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.set_success_color(c);
        }
    }
}

#[derive(Debug, Clone)]
pub struct DangerColorInput(pub String);

impl StartMessage for DangerColorInput {
    fn handle(self, ui: &mut StartUI) {
        ui.set_danger_color_input(self.0.clone());
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.set_danger_color(c);
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecureCheckbox(pub bool);

impl StartMessage for SecureCheckbox {
    fn handle(self, ui: &mut StartUI) {
        ui.set_secure(self.0);
    }
}

#[derive(Debug, Clone)]
pub struct StartButton;

impl IcedMessage for StartButton {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(ui) = win.get_start_ui() {
            let config: Config = ui.clone().into();
            TEXT_SIZE.store(Arc::new(config.text_size));
            config.save().log();
            match Core::new(config.clone()) {
                Ok(client) => {
                    let notify = client.changed();
                    *win = RunningWindow::new(client, config).into();
                    info!("Changed Mode to Running");
                    return Ok(Changed::next(notify));
                }

                Err(e) => {
                    error!("Error when creating client: {e:?}");
                    return Ok(Command::single(Action::Window(
                        iced_native::window::Action::Close,
                    )));
                }
            }
        }
        Ok(Command::none())
    }
}
