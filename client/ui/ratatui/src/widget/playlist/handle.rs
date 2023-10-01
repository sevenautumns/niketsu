use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::view::{Mode, RatatuiView};

pub fn handle_playlist(view: &mut RatatuiView, event: &Event) {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Esc => {
                view.app.set_mode(Mode::Normal);
                view.app.unhighlight_selection();
                view.app.playlist_widget.unselect();
            }
            KeyCode::Enter => match view.app.playlist_widget.get_current_video() {
                Some(video) => view.model.change_video(video.clone()),
                None => {}
            },
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
            KeyCode::Char('y') => view.app.playlist_widget.yank_clipboard_range(),
            KeyCode::Char('p') => {
                if let Some(index) = view.app.playlist_widget.get_current_index() {
                    if let Some(clipboard) = view.app.playlist_widget.get_clipboard() {
                        view.append_at(index, clipboard);
                    } else if let Some(clipboard_range) =
                        view.app.playlist_widget.get_clipboard_range()
                    {
                        view.move_range(clipboard_range, index);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}
