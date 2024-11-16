use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::search::SearchWidget;
use crate::widget::OverlayWidgetState;

#[derive(Debug, Clone, Copy)]
pub struct Search;

impl EventHandler for Search {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.search_widget_state.reset_all();
                        view.app.reset_fuzzy_search();
                    }
                    KeyCode::Enter => {
                        if let Some(video) = view.app.search_widget_state.get_selected() {
                            if let Some(index) = view.app.playlist_widget_state.get_current_index()
                            {
                                view.insert(index + 1, &video.file_name().into());
                            } else {
                                view.insert(0, &video.file_name().into());
                            }
                        }
                    }
                    KeyCode::Up => view.app.search_widget_state.next(),
                    KeyCode::Down => view.app.search_widget_state.previous(),
                    KeyCode::PageUp => view.app.search_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.search_widget_state.jump_previous(5),
                    _ => {
                        view.app.search_widget_state.input(*key);
                        let query = view.app.search_widget_state.get_input();
                        view.app.fuzzy_search(query);
                    }
                }
            }
        }
    }
}

impl RenderHandler for Search {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.search_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(SearchWidget {}, area, &mut app.search_widget_state);
    }
}
