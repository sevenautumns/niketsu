use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::widget::scrollable::RelativeOffset;
use iced::Command;
use log::{debug, trace, warn};

use super::RunningWindow;
use crate::client::database::FileDatabaseSender;
use crate::client::server::NiketsuUserMessage;
use crate::iced_window::message::IcedMessage;
use crate::iced_window::{MainMessage, MainWindow};
use crate::user::ThisUser;

#[enum_dispatch]
pub trait RunningWindowMessage {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>>;
}

#[enum_dispatch(RunningWindowMessage)]
#[derive(Debug, Clone)]
pub enum UserEvent {
    ReadyButton,
    SendMessage,
    StopDbUpdate,
    StartDbUpdate,
    ScrollMessages,
    MessageInput,
}

impl IcedMessage for UserEvent {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(win) = win.get_running() {
            <UserEvent as RunningWindowMessage>::handle(self, win)
        } else {
            warn!("Got RunningWindow message outside RunningWindow");
            Ok(Command::none())
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReadyButton;

impl RunningWindowMessage for ReadyButton {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        debug!("User: ready press");
        let client = win.client();
        client.user().rcu(|u| {
            let mut user = ThisUser::clone(u);
            user.toggle_ready();
            user
        });
        client.ws().send(client.user().load().status())?;
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct SendMessage;

impl RunningWindowMessage for SendMessage {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        let message = win.message_mut();
        if !message.is_empty() {
            let msg = message.clone();
            *message = Default::default();
            let client = win.client();
            client.ws().send(NiketsuUserMessage {
                message: msg.clone(),
                username: client.user().load().name(),
            })?;
            let name = client.user().load().name();
            client.messages().push_user_chat(msg, name);
        }
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct StopDbUpdate;

impl RunningWindowMessage for StopDbUpdate {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        trace!("Stop database update request received");
        win.client().db().stop_update();
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct StartDbUpdate;

impl RunningWindowMessage for StartDbUpdate {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        trace!("Start database update request received");
        FileDatabaseSender::start_update(&win.client().db());
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct ScrollMessages(pub RelativeOffset);

impl RunningWindowMessage for ScrollMessages {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        *win.messages_scroll_mut() = self.0;
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct MessageInput(pub String);

impl RunningWindowMessage for MessageInput {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        *win.message_mut() = self.0;
        Ok(Command::none())
    }
}
