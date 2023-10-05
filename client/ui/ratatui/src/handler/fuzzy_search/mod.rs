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
                        view.app.fuzzy_search_widget.reset_all();
                    }
                    KeyCode::Enter => {
                        if let Some(video) = view.app.fuzzy_search_widget.get_selected() {
                            view.add(&video.file_name().into());
                        }
                    }
                    KeyCode::Up => view.app.fuzzy_search_widget.next(),
                    KeyCode::Down => view.app.fuzzy_search_widget.previous(),
                    _ => {
                        view.app.fuzzy_search_widget.input(*key);
                        let query = view.app.fuzzy_search_widget.get_input();
                        view.app.fuzzy_search(query);
                    }
                }
            }
        }
    }
}
