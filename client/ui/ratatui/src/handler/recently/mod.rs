use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::Style;

use super::playlist::Playlist;
use super::users::Users;
use super::{EventHandler, MainEventHandler, State};
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Recently;

impl EventHandler for Recently {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Up => {
                        view.app.recently_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.recently_widget_state.previous();
                    }
                    KeyCode::Enter => {
                        if let Some(video) = view.app.recently_widget_state.get_selected() {
                            if let Some(index) = view.app.playlist_widget_state.get_current_index()
                            {
                                view.insert(index + 1, &video.file_name().into());
                            } else {
                                view.insert(0, &video.file_name().into());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl MainEventHandler for Recently {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Down => view.transition(State::from(Playlist {})),
            KeyCode::Left => view.transition(State::from(Users {})),
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.recently_widget_state.set_style(style);
    }
}
