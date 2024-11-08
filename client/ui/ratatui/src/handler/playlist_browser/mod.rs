use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::EventHandler;
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct PlaylistBrowserOverlay;

impl EventHandler for PlaylistBrowserOverlay {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.playlist_browser_widget_state.reset_all();
                    }
                    KeyCode::Enter => {
                        let playlist = view.app.playlist_browser_widget_state.get_playlist();
                        if let Some(pl) = playlist {
                            view.model.change_playlist(pl);
                        }
                        view.app.reset_overlay();
                        view.app.playlist_browser_widget_state.reset_all();
                    }
                    KeyCode::Up => view.app.playlist_browser_widget_state.next(),
                    KeyCode::Down => view.app.playlist_browser_widget_state.previous(),
                    _ => view.app.playlist_browser_widget_state.input(*key),
                }
            }
        }
    }
}
