use std::str::FromStr;

use dyn_clone::DynClone;

use super::SettingsView;
use crate::config::RgbWrap;
use crate::message::Message;

pub trait SettingsMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, ui: &mut SettingsView);
}

dyn_clone::clone_trait_object!(SettingsMessage);

#[derive(Debug, Clone)]
pub struct UsernameInput(pub String);

impl SettingsMessage for UsernameInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.username = self.0;
    }
}

impl From<UsernameInput> for Message {
    fn from(value: UsernameInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct UrlInput(pub String);

impl SettingsMessage for UrlInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.url = self.0;
    }
}

impl From<UrlInput> for Message {
    fn from(value: UrlInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl SettingsMessage for PathInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        if let Some(d) = ui.media_dirs.get_mut(self.0) {
            *d = self.1
        }
    }
}

impl From<PathInput> for Message {
    fn from(value: PathInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl SettingsMessage for DeletePath {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        if self.0 < ui.media_dirs.len() {
            ui.media_dirs.remove(self.0);
        }
    }
}

impl From<DeletePath> for Message {
    fn from(value: DeletePath) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl SettingsMessage for AddPath {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.media_dirs.push(Default::default());
    }
}

impl From<AddPath> for Message {
    fn from(value: AddPath) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub String);

impl SettingsMessage for RoomInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.room = self.0;
    }
}

impl From<RoomInput> for Message {
    fn from(value: RoomInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl SettingsMessage for PasswordInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.password = self.0;
    }
}

impl From<PasswordInput> for Message {
    fn from(value: PasswordInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct TextSizeInput(pub f32);

impl SettingsMessage for TextSizeInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.text_size = self.0;
    }
}

impl From<TextSizeInput> for Message {
    fn from(value: TextSizeInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct TextColorInput(pub String);

impl SettingsMessage for TextColorInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.text_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.text_color = c;
        }
    }
}

impl From<TextColorInput> for Message {
    fn from(value: TextColorInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundColorInput(pub String);

impl SettingsMessage for BackgroundColorInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.background_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.background_color = c;
        }
    }
}

impl From<BackgroundColorInput> for Message {
    fn from(value: BackgroundColorInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct PrimaryColorInput(pub String);

impl SettingsMessage for PrimaryColorInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.primary_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.primary_color = c;
        }
    }
}

impl From<PrimaryColorInput> for Message {
    fn from(value: PrimaryColorInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct SuccessColorInput(pub String);

impl SettingsMessage for SuccessColorInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.success_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.success_color = c;
        }
    }
}

impl From<SuccessColorInput> for Message {
    fn from(value: SuccessColorInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct DangerColorInput(pub String);

impl SettingsMessage for DangerColorInput {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.danger_color_input = self.0.clone();
        if let Ok(c) = RgbWrap::from_str(&self.0) {
            ui.danger_color = c;
        }
    }
}

impl From<DangerColorInput> for Message {
    fn from(value: DangerColorInput) -> Self {
        Message::Settings(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct SecureCheckbox(pub bool);

impl SettingsMessage for SecureCheckbox {
    fn handle(self: Box<Self>, ui: &mut SettingsView) {
        ui.secure = self.0;
    }
}

impl From<SecureCheckbox> for Message {
    fn from(value: SecureCheckbox) -> Self {
        Message::Settings(Box::new(value))
    }
}
