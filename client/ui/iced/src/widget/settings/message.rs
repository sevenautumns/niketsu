use std::path::PathBuf;

use arcstr::ArcStr;
use enum_dispatch::enum_dispatch;
use iced::Task;
use niketsu_core::config::Config;
use niketsu_core::log;
use niketsu_core::room::RoomName;
use niketsu_core::ui::{ServerChange, UiModel};

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
    Reset,
    ConnectApplyClose,
    ApplyClose,
    ConnectApplyCloseSave,
    ApplyCloseSave,
    UsernameInput,
    PathInput,
    DeletePath,
    AddPath,
    RoomInput,
    PasswordInput,
    AutoConnectCheckbox,
}

impl MessageHandler for SettingsWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        SettingsWidgetMessageTrait::handle(self, &mut model.settings_widget_state, &model.model);
        Task::none()
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
pub struct Reset;

impl SettingsWidgetMessageTrait for Reset {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config = Config::load_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct ConnectApplyClose;

impl SettingsWidgetMessageTrait for ConnectApplyClose {
    fn handle(self, state: &mut SettingsWidgetState, model: &UiModel) {
        ApplyClose.handle(state, model);
        let config = state.config();
        model.change_server(ServerChange {
            addr: config.addr(),
            password: config.password.clone(),
            room: config.room.clone(),
        });
    }
}

#[derive(Debug, Clone)]
pub struct ApplyClose;

impl SettingsWidgetMessageTrait for ApplyClose {
    fn handle(self, state: &mut SettingsWidgetState, model: &UiModel) {
        state.active = false;
        let config = state.config();
        let media_dirs: Vec<_> = config.media_dirs.iter().map(PathBuf::from).collect();
        let username = config.username.clone();
        model.change_db_paths(media_dirs);
        model.change_username(username);
    }
}

#[derive(Debug, Clone)]
pub struct ConnectApplyCloseSave;

impl SettingsWidgetMessageTrait for ConnectApplyCloseSave {
    fn handle(self, state: &mut SettingsWidgetState, model: &UiModel) {
        ConnectApplyClose.handle(state, model);
        log!(state.config().save());
    }
}

#[derive(Debug, Clone)]
pub struct ApplyCloseSave;

impl SettingsWidgetMessageTrait for ApplyCloseSave {
    fn handle(self, state: &mut SettingsWidgetState, model: &UiModel) {
        ApplyClose.handle(state, model);
        log!(state.config().save());
    }
}

#[derive(Debug, Clone)]
pub struct UsernameInput(pub ArcStr);

impl SettingsWidgetMessageTrait for UsernameInput {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config.username = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PathInput(pub usize, pub String);

impl SettingsWidgetMessageTrait for PathInput {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        if let Some(d) = state.config.media_dirs.get_mut(self.0) {
            *d = self.1
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeletePath(pub usize);

impl SettingsWidgetMessageTrait for DeletePath {
    fn handle(self, ui: &mut SettingsWidgetState, _: &UiModel) {
        if self.0 < ui.config.media_dirs.len() {
            ui.config.media_dirs.remove(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddPath;

impl SettingsWidgetMessageTrait for AddPath {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config.media_dirs.push(Default::default());
    }
}

#[derive(Debug, Clone)]
pub struct RoomInput(pub RoomName);

impl SettingsWidgetMessageTrait for RoomInput {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config.room = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct PasswordInput(pub String);

impl SettingsWidgetMessageTrait for PasswordInput {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config.password = self.0;
    }
}

#[derive(Debug, Clone)]
pub struct AutoConnectCheckbox(pub bool);

impl SettingsWidgetMessageTrait for AutoConnectCheckbox {
    fn handle(self, state: &mut SettingsWidgetState, _: &UiModel) {
        state.config.auto_connect = self.0;
    }
}
