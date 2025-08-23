use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use niketsu_core::playlist::Video;
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::search::SearchWidget;

#[derive(Debug, Clone, Copy)]
pub struct BrowserSearch;

impl EventHandler for BrowserSearch {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.browser_search_widget_state.reset_all();
                        view.app.reset_browser_search();
                    }
                    KeyCode::Enter => {
                        if let Some(videos) = view.app.browser_search_widget_state.get_selected() {
                            let videos_range: Vec<Video> =
                                videos.iter().map(|v| v.file_name().into()).collect();
                            if let Some(index) = view.app.playlist_widget_state.selected() {
                                view.insert_range(index + 1, videos_range);
                            } else {
                                view.insert_range(0, videos_range);
                            }
                        }
                        view.app.reset_overlay();
                        view.app.browser_search_widget_state.reset_all();
                        view.app.reset_browser_search();
                    }
                    KeyCode::PageUp => view.app.browser_search_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.browser_search_widget_state.jump_previous(5),
                    KeyCode::Home => view.app.browser_search_widget_state.jump_start(),
                    KeyCode::End => view.app.browser_search_widget_state.jump_end(),
                    KeyCode::Up => {
                        view.app.browser_search_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.browser_search_widget_state.previous();
                    }
                    k => {
                        if matches!(k, KeyCode::Char('x')) && key.modifiers == KeyModifiers::CONTROL
                        {
                            view.app
                                .browser_search_widget_state
                                .increase_selection_offset();
                        } else {
                            view.app.browser_search_widget_state.select(Some(0));
                            view.app.browser_search_widget_state.reset_offset();
                            view.app.browser_search_widget_state.input(*key);
                            let query = view.app.browser_search_widget_state.get_input();
                            view.app.search_browser(query);
                        }
                    }
                }
            }
        }
    }
}

impl RenderHandler for BrowserSearch {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.browser_search_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(
            SearchWidget::default(),
            area,
            &mut app.browser_search_widget_state,
        );
    }
}
