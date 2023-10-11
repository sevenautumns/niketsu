use crossterm::event::{Event, KeyEvent};
use enum_dispatch::enum_dispatch;
use ratatui::style::Style;

use self::chat::Chat;
use self::chat_input::ChatInput;
use self::database::Database;
use self::fuzzy_search::FuzzySearch;
use self::login::Login;
use self::options::Options;
use self::playlist::Playlist;
use self::room::Rooms;
use crate::view::RatatuiView;

pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod fuzzy_search;
pub(crate) mod help;
pub(crate) mod login;
pub(crate) mod options;
pub(crate) mod playlist;
pub(crate) mod room;

#[enum_dispatch]
pub trait EventHandler {
    fn handle(&self, view: &mut RatatuiView, event: &Event);
}

#[enum_dispatch]
pub trait MainEventHandler: EventHandler {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent);
    fn set_style(&self, view: &mut RatatuiView, style: Style);
}

#[enum_dispatch(MainEventHandler, EventHandler)]
#[derive(Debug, Clone, Copy)]
pub enum State {
    Chat(Chat),
    ChatInput(ChatInput),
    Database(Database),
    Rooms(Rooms),
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
    // Help,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::from(Login {})
    }
}