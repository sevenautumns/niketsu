use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::widget::scrollable::{Id, RelativeOffset};
use iced::Command;
use log::{trace, warn};

use super::running::message::RunningWindowMessage;
use super::{MainMessage, MainWindow};
use crate::heartbeat::Changed;

#[enum_dispatch]
pub trait IcedMessage {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>>;
}

#[derive(Debug, Clone)]
pub struct PlayerChanged;

impl IcedMessage for PlayerChanged {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(win) = win.get_running() {
            <PlayerChanged as RunningWindowMessage>::handle(self, win)
        } else {
            warn!("Got RunningWindow message outside RunningWindow");
            Ok(Command::none())
        }
    }
}

impl RunningWindowMessage for PlayerChanged {
    fn handle(self, win: &mut super::running::RunningWindow) -> Result<Command<MainMessage>> {
        trace!("Player Changed");
        let messages_scroll = win.messages_scroll();
        let snap = if RelativeOffset::END.y.eq(&messages_scroll.y) {
            iced_native::widget::scrollable::snap_to(Id::new("messages"), *messages_scroll)
        } else {
            Command::none()
        };
        Ok(Command::batch([
            snap,
            Changed::next(win.client().changed()),
        ]))
    }
}
