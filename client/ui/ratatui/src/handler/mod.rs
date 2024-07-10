use crossterm::event::{Event, KeyCode, KeyEvent};
use enum_dispatch::enum_dispatch;
use ratatui::style::Style;

use self::chat::Chat;
use self::chat_input::ChatInput;
use self::command::Command;
use self::database::Database;
use self::fuzzy_search::FuzzySearch;
use self::help::Help;
use self::login::Login;
use self::media::MediaDir;
use self::options::Options;
use self::playlist::Playlist;
use self::users::Users;
use crate::view::{Mode, RatatuiView};

pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod fuzzy_search;
pub(crate) mod help;
pub(crate) mod login;
pub(crate) mod media;
pub(crate) mod options;
pub(crate) mod playlist;
pub(crate) mod users;

#[enum_dispatch]
pub trait EventHandler {
    fn handle(&self, view: &mut RatatuiView, event: &Event);
}

#[enum_dispatch]
pub trait MainEventHandler: EventHandler {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent);

    fn handle_with_overlay(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            match view.app.current_state() {
                State::ChatInput(_) => {}
                _ => {
                    if key.code == KeyCode::Char(' ') {
                        view.app.set_mode(Mode::Overlay);
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(Options {})));
                    }
                }
            }
        }
        self.handle(view, event);
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style);
}

#[enum_dispatch(MainEventHandler, EventHandler)]
#[derive(Debug, Clone, Copy)]
pub enum State {
    Chat(Chat),
    ChatInput(ChatInput),
    Database(Database),
    Users(Users),
    Playlist(Playlist),
}

impl Default for State {
    fn default() -> Self {
        Self::from(Chat {})
    }
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone, Copy)]
pub enum OverlayState {
    Login(Login),
    FuzzySearch(FuzzySearch),
    Option(Options),
    MediaDir(MediaDir),
    Command(Command),
    Help(Help),
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::from(Login {})
    }
}
