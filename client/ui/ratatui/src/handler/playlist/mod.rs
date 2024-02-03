use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use niketsu_core::playlist::Video;
use ratatui::style::Style;

use super::chat_input::ChatInput;
use super::room::Rooms;
use super::{MainEventHandler, State};
use crate::handler::EventHandler;
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Playlist;

impl EventHandler for Playlist {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Enter => {
                        if let Some(video) = view.app.playlist_widget_state.get_current_video() {
                            view.select(video.clone())
                        }
                    }
                    KeyCode::PageUp => view.app.playlist_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.playlist_widget_state.jump_previous(5),
                    KeyCode::Home => view.app.playlist_widget_state.jump_start(),
                    KeyCode::End => view.app.playlist_widget_state.jump_end(),
                    KeyCode::Up => {
                        view.app.playlist_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.playlist_widget_state.previous();
                    }
                    KeyCode::Char('d') => {
                        if let Some(index) = view.app.playlist_widget_state.yank_clipboard() {
                            view.remove_range(index);
                            view.app.playlist_widget_state.reset_offset();
                        }
                    }
                    KeyCode::Char('x') => {
                        view.app.playlist_widget_state.increase_selection_offset();
                    }
                    KeyCode::Char('y') => {
                        view.app.playlist_widget_state.yank_clipboard();
                    }
                    KeyCode::Char('p') => {
                        if let Some(index) = view.app.playlist_widget_state.get_current_index() {
                            if let Some(clipboard) = view.app.playlist_widget_state.get_clipboard()
                            {
                                view.append_at(index + 1, clipboard);
                            }
                        }
                    }
                    KeyCode::Char('v') => {
                        if key.modifiers == KeyModifiers::CONTROL {
                            let content = view.app.get_clipboard();
                            if let Ok(c) = content {
                                if let Some(index) =
                                    view.app.playlist_widget_state.get_current_index()
                                {
                                    view.insert(index + 1, &Video::from(c.as_str()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl MainEventHandler for Playlist {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Left => view.transition(State::from(ChatInput {})),
            KeyCode::Up => view.transition(State::from(Rooms {})),
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.playlist_widget_state.set_style(style);
    }
}
