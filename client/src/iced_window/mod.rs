use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::{Application, Command, Element, Renderer, Theme};
use startup::message::StartButton;

use self::startup::message::StartUIMessage;
use crate::client::LogResult;
use crate::config::Config;
use crate::iced_window::message::{IcedMessage, PlayerChanged};
use crate::iced_window::running::message::UserEvent;
use crate::iced_window::running::RunningWindow;
use crate::iced_window::startup::StartUI;
use crate::playlist::message::PlaylistMessage;
use crate::rooms::message::RoomsWidgetMessage;

pub mod message;
pub mod running;
pub mod startup;

#[enum_dispatch]
pub trait InnerApplication {
    fn view<'a>(&self) -> Element<'a, MainMessage, Renderer<Theme>>;
    fn config(&self) -> &Config;
    fn theme(&self) -> Theme {
        self.config().theme()
    }
}

#[enum_dispatch(InnerApplication)]
#[derive(Debug)]
pub enum MainWindow {
    StartUI(Box<StartUI>),
    RunningWindow,
}

impl MainWindow {
    pub fn get_start_ui(&mut self) -> Option<&mut StartUI> {
        if let MainWindow::StartUI(ui) = self {
            return Some(ui);
        }
        None
    }

    pub fn get_running(&mut self) -> Option<&mut RunningWindow> {
        if let MainWindow::RunningWindow(win) = self {
            return Some(win);
        }
        None
    }
}

#[enum_dispatch(IcedMessage)]
#[derive(Debug, Clone)]
pub enum MainMessage {
    StartUIMessage,
    StartButton,
    PlaylistMessage,
    UserEvent,
    RoomsWidgetMessage,
    PlayerChanged,
}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = MainMessage;

    type Theme = Theme;

    type Flags = Config;

    fn new(config: Self::Flags) -> (Self, Command<Self::Message>) {
        (Box::new(StartUI::from(config)).into(), Command::none())
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn theme(&self) -> Self::Theme {
        InnerApplication::theme(self)
    }

    fn update(&mut self, msg: Self::Message) -> Command<Self::Message> {
        match msg.handle(self) {
            Ok(cmd) => cmd,
            msg @ Err(_) => {
                msg.log();
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        InnerApplication::view(self)
    }
}
