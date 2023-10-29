use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::EventHandler;
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct FuzzySearch;

impl EventHandler for FuzzySearch {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.fuzzy_search_widget_state.reset_all();
                        view.app.reset_fuzzy_search();
                    }
                    KeyCode::Enter => {
                        if let Some(video) = view.app.fuzzy_search_widget_state.get_selected() {
                            if let Some(index) = view.app.playlist_widget_state.get_current_index()
                            {
                                view.insert(index + 1, &video.file_name().into());
                            } else {
                                view.insert(0, &video.file_name().into());
                            }
                        }
                    }
                    KeyCode::Up => view.app.fuzzy_search_widget_state.next(),
                    KeyCode::Down => view.app.fuzzy_search_widget_state.previous(),
                    _ => {
                        view.app.fuzzy_search_widget_state.input(*key);
                        let query = view.app.fuzzy_search_widget_state.get_input();
                        view.app.fuzzy_search(query);
                    }
                }
            }
        }
    }
}
