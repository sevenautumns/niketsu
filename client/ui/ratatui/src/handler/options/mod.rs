use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::widgets::Clear;

use super::help::Help;
use super::login::Login;
use super::media::MediaDir;
use super::playlist_browser::PlaylistBrowserOverlay;
use super::search::Search;
use super::{EventHandler, OverlayState, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::options::OptionsWidget;

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
                    KeyCode::Char('/') => {
                        view.app
                            .set_current_overlay_state(Some(OverlayState::from(Search {})));
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
                        view.app.users_widget_state.toggle_ready();
                        view.app.reset_overlay();
                    }
                    KeyCode::Char('f') => {
                        view.model.video_share_toggle();
                        view.app.reset_overlay();
                    }
                    KeyCode::Char('x') => {
                        view.model.video_file_request();
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

impl RenderHandler for Options {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.options_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(OptionsWidget {}, area, &mut app.options_widget_state);
    }
}
