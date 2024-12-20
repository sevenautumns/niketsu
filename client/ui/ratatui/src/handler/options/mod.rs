use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::fuzzy_search::FuzzySearch;
use super::help::Help;
use super::login::Login;
use super::media::MediaDir;
use super::playlist_browser::PlaylistBrowserOverlay;
use super::{EventHandler, OverlayState};
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct Options;

impl EventHandler for Options {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('h') => view
                        .app
                        .set_current_overlay_state(Some(OverlayState::from(Help {}))),
                    KeyCode::Char('l') => view
                        .app
                        .set_current_overlay_state(Some(OverlayState::from(Login {}))),
                    KeyCode::Char('f') => {
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(FuzzySearch {})));
                        view.app.fuzzy_search("".to_string());
                    }
                    KeyCode::Char('m') => view
                        .app
                        .set_current_overlay_state(Some(OverlayState::from(MediaDir {}))),
                    KeyCode::Char('b') => view.app.set_current_overlay_state(Some(
                        OverlayState::from(PlaylistBrowserOverlay {}),
                    )),
                    KeyCode::Char('r') => {
                        view.model.user_ready_toggle();
                        //TODO needs to be changed once ready toggling works better
                        view.app.users_widget_state.toggle_ready();
                        view.app.reset_overlay();
                    }
                    KeyCode::Char('s') => {
                        view.model.start_db_update();
                        view.app.reset_overlay();
                    }
                    KeyCode::Char('p') => {
                        view.model.stop_db_update();
                        view.app.reset_overlay();
                    }
                    _ => {
                        view.app.reset_overlay();
                    }
                }
            }
        }
    }
}
