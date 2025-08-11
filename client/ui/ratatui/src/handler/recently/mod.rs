use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use niketsu_core::playlist::Video;
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
                    KeyCode::PageUp => view.app.recently_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.recently_widget_state.jump_previous(5),
                    KeyCode::Home => view.app.recently_widget_state.jump_start(),
                    KeyCode::End => view.app.recently_widget_state.jump_end(),
                    KeyCode::Up => {
                        view.app.recently_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.recently_widget_state.previous();
                    }
                    KeyCode::Enter => {
                        if let Some(videos) = view.app.recently_widget_state.get_selected() {
                            let videos_range: Vec<Video> =
                                videos.iter().map(|v| v.file_name().into()).collect();
                            if let Some(index) = view.app.playlist_widget_state.selected() {
                                view.insert_range(index + 1, videos_range);
                            } else {
                                view.insert_range(0, videos_range);
                            }
                        }
                    }
                    KeyCode::Right | KeyCode::Tab => {
                        view.app.recently_widget_state.previous_frequency()
                    }
                    KeyCode::Left | KeyCode::BackTab => {
                        view.app.recently_widget_state.next_frequency()
                    }
                    KeyCode::Char('x') => {
                        view.app.recently_widget_state.increase_selection_offset();
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
