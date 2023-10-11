use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
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
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => {
                    view.app.set_mode(Mode::Normal);
                    view.hover_highlight();
                }
                KeyCode::Enter => {
                    if let Some(video) = view.app.playlist_widget.get_current_video() {
                        view.select(video.clone())
                    }
                }
                KeyCode::PageUp => view.app.playlist_widget.jump_next(5),
                KeyCode::PageDown => view.app.playlist_widget.jump_previous(5),
                KeyCode::Up => {
                    view.app.playlist_widget.next();
                }
                KeyCode::Down => {
                    view.app.playlist_widget.previous();
                }
                KeyCode::Char('d') => {
                    if let Some(index) = view.app.playlist_widget.yank_clipboard() {
                        view.remove_range(index);
                        view.app.playlist_widget.reset_offset();
                    }
                }
                KeyCode::Char('x') => {
                    view.app.playlist_widget.increase_selection_offset();
                }
                KeyCode::Char('y') => {
                    view.app.playlist_widget.yank_clipboard();
                }
                KeyCode::Char('p') => {
                    if let Some(index) = view.app.playlist_widget.get_current_index() {
                        if let Some(clipboard) = view.app.playlist_widget.get_clipboard() {
                            view.append_at(index, clipboard);
                        }
                    }
                }
                _ => {}
            },
            Event::Paste(data) => {
                view.add(&Video::from(data.as_str()));
            }
            _ => {}
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
        view.app.playlist_widget.set_style(style);
    }
}
