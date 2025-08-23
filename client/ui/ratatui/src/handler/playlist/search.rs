use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use niketsu_core::playlist::Video;
use ratatui::widgets::Clear;

use super::EventHandler;
use crate::handler::RenderHandler;
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::search::SearchWidget;

#[derive(Debug, Clone, Copy)]
pub struct PlaylistSearch;

impl EventHandler for PlaylistSearch {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.playlist_search_widget_state.reset_all();
                        view.app.reset_playlist_search();
                    }
                    KeyCode::Enter => {
                        if let Some(videos) = view.app.playlist_search_widget_state.get_selected() {
                            let videos_range: Vec<Video> =
                                videos.iter().map(|v| v.as_str().into()).collect();

                            for video in videos_range.iter() {
                                view.remove(video);
                            }

                            if let Some(index) = view.app.playlist_widget_state.selected() {
                                view.insert_range(index + 1, videos_range);
                            } else {
                                view.insert_range(0, videos_range);
                            }
                        }
                        view.app.reset_overlay();
                        view.app.playlist_search_widget_state.reset_all();
                        view.app.reset_playlist_search();
                    }
                    KeyCode::PageUp => view.app.playlist_search_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.playlist_search_widget_state.jump_previous(5),
                    KeyCode::Home => view.app.playlist_search_widget_state.jump_start(),
                    KeyCode::End => view.app.playlist_search_widget_state.jump_end(),
                    KeyCode::Up => {
                        view.app.playlist_search_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.playlist_search_widget_state.previous();
                    }
                    k => match (k, key.modifiers) {
                        (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                            view.app
                                .playlist_search_widget_state
                                .increase_selection_offset();
                        }
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            if let Some(videos) =
                                view.app.playlist_search_widget_state.get_selected()
                            {
                                let videos_range: Vec<Video> =
                                    videos.iter().map(|v| v.as_str().into()).collect();

                                for video in videos_range.iter() {
                                    view.remove(video);
                                }
                            }
                            view.app.reset_overlay();
                            view.app.playlist_search_widget_state.reset_all();
                            view.app.reset_playlist_search();
                        }
                        (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                            if let Some(videos) =
                                view.app.playlist_search_widget_state.get_selected()
                            {
                                if let Some(video) = videos.first() {
                                    view.app.playlist_widget_state.jump_to_video(video);
                                }
                            }
                            view.app.reset_overlay();
                            view.app.playlist_search_widget_state.reset_all();
                            view.app.reset_playlist_search();
                        }
                        (_, _) => {
                            view.app.playlist_search_widget_state.select(Some(0));
                            view.app.playlist_search_widget_state.reset_offset();
                            view.app.playlist_search_widget_state.input(*key);
                            let query = view.app.playlist_search_widget_state.get_input();
                            view.app.search_playlist(query);
                        }
                    },
                }
            }
        }
    }
}

impl RenderHandler for PlaylistSearch {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.playlist_search_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(
            SearchWidget::default(),
            area,
            &mut app.playlist_search_widget_state,
        );
    }
}
