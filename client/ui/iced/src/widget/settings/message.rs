use std::path::PathBuf;

use enum_dispatch::enum_dispatch;
use iced::Command;
use niketsu_core::config::Config;
use niketsu_core::log;
use niketsu_core::ui::{RoomChange, ServerChange, UiModel};

use super::SettingsWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait SettingsWidgetMessageTrait {
    fn handle(self, ui: &mut SettingsWidgetState, model: &UiModel);
}

#[enum_dispatch(SettingsWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum SettingsWidgetMessage {
    Activate,
    Abort,
    Close,
    UsernameInput,
    UrlInput,
    PathInput,
    DeletePath,
    AddPath,
    RoomInput,
    PasswordInput,
    SecureCheckbox,
}

impl MessageHandler for SettingsWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        SettingsWidgetMessageTrait::handle(self, &mut model.settings_widget_state, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct Activate;

impl SettingsWidgetMessageTrait for Activate {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.active = true
    }
}

#[derive(Debug, Clone)]
pub struct Abort;

impl SettingsWidgetMessageTrait for Abort {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.active = false
    }
}

#[derive(Debug, Clone)]
pub struct Close;

impl SettingsWidgetMessageTrait for Close {
    fn handle(self, state: &mut SettingsWidgetState, model: &UiModel) {
        state.active = false;
        let config: Config = state.clone().into();
        let media_dirs: Vec<_> = config.media_dirs.iter().map(PathBuf::from).collect();
        let username = config.username.clone();
        model.change_db_paths(media_dirs);
        model.change_username(username);
        model.change_server(ServerChange {
            addr: config.url.clone(),
            secure: config.secure,
            password: Some(config.password.clone()),
            room: RoomChange {
                room: config.room.clone(),
            },
        });
        log!(config.save());
    }
}

#[derive(Debug, Clone)]
pub struct UsernameInput(pub String);

impl SettingsWidgetMessageTrait for UsernameInput {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.username = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct UrlInput(pub String);

impl SettingsWidgetMessageTrait for UrlInput {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.url = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl SettingsWidgetMessageTrait for PathInput {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        if let Some(d) = ui.media_dirs.get_mut(self.0) {
            *d = self.1
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl SettingsWidgetMessageTrait for DeletePath {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        if self.0 < ui.media_dirs.len() {
            ui.media_dirs.remove(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl SettingsWidgetMessageTrait for AddPath {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.media_dirs.push(Default::default());
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub String);

impl SettingsWidgetMessageTrait for RoomInput {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.room = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl SettingsWidgetMessageTrait for PasswordInput {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.password = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct SecureCheckbox(pub bool);

impl SettingsWidgetMessageTrait for SecureCheckbox {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        ui.secure = self.0;
    }
}
