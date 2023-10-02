use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::fuzzy_search::FuzzySearch;
use super::login::Login;
use super::{EventHandler, OverlayState};
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct Options;

impl EventHandler for Options {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    // KeyCode::Char('h') => view
                    //     .app
                    //     .set_current_overlay_state(Some(OverlayState::from(Help {}))),
                    KeyCode::Char('l') => view
                        .app
                        .set_current_overlay_state(Some(OverlayState::from(Login {}))),
                    KeyCode::Char('f') => {
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(FuzzySearch {})));
                        view.app.fuzzy_search("".to_string());
                    }
                    _ => {
                        view.app.reset_overlay();
                    }
                }
            }
        }
    }
}
