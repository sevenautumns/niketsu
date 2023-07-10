use iced::{Element, Renderer};
use iced_native::{Command, Theme};
use log::warn;

use super::ui::{IcedUITrait, IcedUiMessage};
use crate::config::Config;

pub(super) mod message;

pub(super) struct RunningWindow {}

impl IcedUITrait for RunningWindow {
    fn view<'a>(&self) -> Element<'a, IcedUiMessage, Renderer<Theme>> {
        todo!()
    }

    fn update(&mut self, message: IcedUiMessage) -> Command<IcedUiMessage> {
        let IcedUiMessage::Running(msg) = message else {
            warn!("unexpected message during \"running\" state: {message:?}");
            return Command::none();
        };

        todo!()
    }

    fn theme(&self) -> Theme {
        todo!()
    }
}
