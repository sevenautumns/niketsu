use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use super::EventHandler;
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct MediaDir;

impl EventHandler for MediaDir {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.media_widget_state.reset_all();
                    }
                    KeyCode::Enter => {
                        view.app.media_widget_state.push_path();
                        let media_paths = view.app.media_widget_state.get_paths();
                        view.change_media_dirs(
                            media_paths.clone().iter().map(|m| m.into()).collect(),
                        );
                        view.save_media_dir(media_paths)
                    }
                    KeyCode::Up => view.app.media_widget_state.next(),
                    KeyCode::Down => view.app.media_widget_state.previous(),
                    KeyCode::Char('d') => {
                        if key.modifiers == KeyModifiers::CONTROL {
                            view.app.media_widget_state.remove_path();
                            let media_paths = view.app.media_widget_state.get_paths();
                            view.change_media_dirs(
                                media_paths.clone().iter().map(|m| m.into()).collect(),
                            );
                            view.save_media_dir(media_paths)
                        } else {
                            view.app.media_widget_state.input(*key);
                        }
                    }
                    _ => view.app.media_widget_state.input(*key),
                }
            }
        }
    }
}
