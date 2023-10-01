use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::view::RatatuiView;

pub fn handle_fuzzy_search(view: &mut RatatuiView, event: &Event) {
    if let Event::Key(key) = event {
        if key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Esc => {
                    view.app.reset_overlay();
                    view.app.fuzzy_search_widget.reset_all();
                }
                KeyCode::Enter => todo!(),
                KeyCode::Up => todo!(),
                KeyCode::Down => todo!(),
                _ => {
                    view.app.fuzzy_search_widget.input_field.input(*key);
                    let query = view.app.fuzzy_search_widget.get_input();
                    let file_database = view.app.fuzzy_search_widget.get_file_database();
                    view.app.search(query, file_database);
                }
            }
        }
    }
}
