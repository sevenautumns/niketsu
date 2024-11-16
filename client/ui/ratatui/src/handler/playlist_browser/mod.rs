use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::playlist_browser::PlaylistBrowserWidget;
use crate::widget::OverlayWidgetState;

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

impl RenderHandler for PlaylistBrowserOverlay {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.playlist_browser_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(
            PlaylistBrowserWidget {},
            area,
            &mut app.playlist_browser_widget_state,
        );
    }
}
