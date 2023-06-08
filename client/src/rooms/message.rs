use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::Command;
use log::warn;

use super::RoomsWidgetState;
use crate::client::server::NiketsuJoin;
use crate::iced_window::message::IcedMessage;
use crate::iced_window::running::message::RunningWindowMessage;
use crate::iced_window::running::RunningWindow;
use crate::iced_window::{MainMessage, MainWindow};

#[enum_dispatch(RunningWindowMessage)]
#[derive(Debug, Clone)]
pub enum RoomsWidgetMessage {
    ClickRoom,
}

#[derive(Debug, Clone)]
pub struct ClickRoom(pub String);

impl RunningWindowMessage for ClickRoom {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        let mut double_click = false;
        let client = win.client();
        let config = win.config();
        client.rooms().rcu(|r| {
            let mut rooms = RoomsWidgetState::clone(r);
            double_click = rooms.click_room(self.0.clone());
            rooms
        });
        if double_click {
            client.ws().send(NiketsuJoin {
                password: config.password.clone(),
                room: self.0,
                username: config.username.clone(),
            })?;
        }
        Ok(Command::none())
    }
}

impl IcedMessage for RoomsWidgetMessage {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(win) = win.get_running() {
            <RoomsWidgetMessage as RunningWindowMessage>::handle(self, win)
        } else {
            warn!("Got RunningWindow message outside RunningWindow");
            Ok(Command::none())
        }
    }
}
