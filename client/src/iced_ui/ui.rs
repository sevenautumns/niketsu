use enum_dispatch::enum_dispatch;
use iced::{Application, Element, Renderer};
use iced_native::{Command, Theme};

use super::running::message::RunningWindowMessage;
use super::running::RunningWindow;
use super::startup::message::StartWindowMessage;
use super::startup::StartWindow;
use crate::config::Config;

#[derive(Debug, Clone)]
pub enum IcedUiMessage {
    Start(StartWindowMessage),
    Running(RunningWindowMessage),
}

#[enum_dispatch]
pub trait IcedUITrait {
    fn update(&mut self, message: IcedUiMessage) -> Command<IcedUiMessage>;
    fn view<'a>(&self) -> Element<'a, IcedUiMessage, Renderer<Theme>>;
    fn theme(&self) -> Theme;
}

#[enum_dispatch(IcedUITrait)]
pub(super) enum IcedUiWindow {
    StartWindow,
    RunningWindow,
}

impl Application for IcedUiWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = IcedUiMessage;

    type Theme = Theme;

    type Flags = Config;

    fn new(config: Self::Flags) -> (Self, Command<Self::Message>) {
        (StartWindow::from(config).into(), Command::none())
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        IcedUITrait::update(self, message)
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        IcedUITrait::view(self)
    }
}
