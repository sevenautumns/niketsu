use crossterm::event::{Event, KeyCode, KeyEvent};
use enum_dispatch::enum_dispatch;
use playlist::video_overlay::VideoName;
use playlist_browser::PlaylistBrowserOverlay;
use ratatui::style::Style;
use recently::Recently;

use self::chat::Chat;
use self::chat_input::ChatInput;
use self::command::Command;
use self::help::Help;
use self::login::Login;
use self::media::MediaDir;
use self::options::Options;
use self::playlist::Playlist;
use self::search::Search;
use self::users::Users;
use crate::view::{Mode, RatatuiView};

pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod command;
pub(crate) mod help;
pub(crate) mod login;
pub(crate) mod media;
pub(crate) mod options;
pub(crate) mod playlist;
pub(crate) mod playlist_browser;
pub(crate) mod recently;
pub(crate) mod search;
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
                _ => match key.code {
                    KeyCode::Char(' ') => {
                        view.app.set_mode(Mode::Overlay);
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(Options {})));
                    }
                    KeyCode::Char(':') => {
                        view.app.set_mode(Mode::Overlay);
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(Command {})));
                        view.app.command_input_widget_state.set_active(true);
                    }
                    _ => {}
                },
            }
        }
        self.handle(view, event);
    }

    //TODO should probably be split into a select and deselect function for better
    // control of transitions
    fn set_style(&self, view: &mut RatatuiView, style: Style);
}

#[enum_dispatch(MainEventHandler, EventHandler)]
#[derive(Debug, Clone, Copy)]
pub enum State {
    Chat(Chat),
    ChatInput(ChatInput),
    Users(Users),
    Playlist(Playlist),
    Recently(Recently),
}

impl Default for State {
    fn default() -> Self {
        Self::from(Chat {})
    }
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum OverlayState {
    Login(Login),
    Search(Search),
    Option(Options),
    MediaDir(MediaDir),
    PlaylistBrowser(PlaylistBrowserOverlay),
    VideoName(VideoName),
    Command(Command),
    Help(Help),
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::from(Login {})
    }
}
