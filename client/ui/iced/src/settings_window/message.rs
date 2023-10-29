use std::path::PathBuf;

use enum_dispatch::enum_dispatch;
use iced::Command;
use niketsu_core::config::Config;
use niketsu_core::log;
use niketsu_core::ui::{RoomChange, ServerChange, UiModel};

use super::SettingsViewState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait SettingsMessageTrait {
    fn handle(self, ui: &mut SettingsViewState, model: &UiModel);
}

#[enum_dispatch(SettingsMessageTrait)]
#[derive(Debug, Clone)]
pub enum SettingsMessage {
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

impl MessageHandler for SettingsMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        SettingsMessageTrait::handle(self, &mut model.settings, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct Activate;

impl SettingsMessageTrait for Activate {
    fn handle(self, state: &mut SettingsViewState, _: &UiModel) {
        state.active = true
    }
}

#[derive(Debug, Clone)]
pub struct Abort;

impl SettingsMessageTrait for Abort {
    fn handle(self, state: &mut SettingsViewState, _: &UiModel) {
        state.active = false
    }
}

#[derive(Debug, Clone)]
pub struct Close;

impl SettingsMessageTrait for Close {
    fn handle(self, state: &mut SettingsViewState, model: &UiModel) {
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

impl SettingsMessageTrait for UsernameInput {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.username = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct UrlInput(pub String);

impl SettingsMessageTrait for UrlInput {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.url = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl SettingsMessageTrait for PathInput {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        if let Some(d) = ui.media_dirs.get_mut(self.0) {
            *d = self.1
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl SettingsMessageTrait for DeletePath {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        if self.0 < ui.media_dirs.len() {
            ui.media_dirs.remove(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl SettingsMessageTrait for AddPath {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.media_dirs.push(Default::default());
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub String);

impl SettingsMessageTrait for RoomInput {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.room = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl SettingsMessageTrait for PasswordInput {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.password = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct SecureCheckbox(pub bool);

impl SettingsMessageTrait for SecureCheckbox {
    fn handle(self, ui: &mut SettingsViewState, _: &UiModel) {
        ui.secure = self.0;
    }
}
