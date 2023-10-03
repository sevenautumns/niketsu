use std::str::FromStr;

use enum_dispatch::enum_dispatch;
use iced::Command;

use super::SettingsView;
use crate::config::RgbWrap;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait SettingsMessageTrait {
    fn handle(self, ui: &mut SettingsView);
}

#[enum_dispatch(SettingsMessageTrait)]
#[derive(Debug, Clone)]
pub enum SettingsMessage {
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

impl MessageHandler for SettingsMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        if let Some(settings) = &mut model.settings {
            SettingsMessageTrait::handle(self, settings);
        }
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct UsernameInput(pub String);

impl SettingsMessageTrait for UsernameInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.username = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct UrlInput(pub String);

impl SettingsMessageTrait for UrlInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.url = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl SettingsMessageTrait for PathInput {
    fn handle(self, ui: &mut SettingsView) {
        if let Some(d) = ui.media_dirs.get_mut(self.0) {
            *d = self.1
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl SettingsMessageTrait for DeletePath {
    fn handle(self, ui: &mut SettingsView) {
        if self.0 < ui.media_dirs.len() {
            ui.media_dirs.remove(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl SettingsMessageTrait for AddPath {
    fn handle(self, ui: &mut SettingsView) {
        ui.media_dirs.push(Default::default());
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub String);

impl SettingsMessageTrait for RoomInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.room = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl SettingsMessageTrait for PasswordInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.password = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct TextSizeInput(pub f32);

impl SettingsMessageTrait for TextSizeInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.text_size = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct TextColorInput(pub String);

impl SettingsMessageTrait for TextColorInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.text_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.text_color = c;
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundColorInput(pub String);

impl SettingsMessageTrait for BackgroundColorInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.background_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.background_color = c;
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrimaryColorInput(pub String);

impl SettingsMessageTrait for PrimaryColorInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.primary_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.primary_color = c;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SuccessColorInput(pub String);

impl SettingsMessageTrait for SuccessColorInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.success_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.success_color = c;
        }
    }
}

#[derive(Debug, Clone)]
pub struct DangerColorInput(pub String);

impl SettingsMessageTrait for DangerColorInput {
    fn handle(self, ui: &mut SettingsView) {
        ui.danger_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.danger_color = c;
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecureCheckbox(pub bool);

impl SettingsMessageTrait for SecureCheckbox {
    fn handle(self, ui: &mut SettingsView) {
        ui.secure = self.0;
    }
}
