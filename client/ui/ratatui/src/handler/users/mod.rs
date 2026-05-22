use chrono::Local;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use niketsu_core::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};

use super::chat::Chat;
use super::recently::Recently;
use super::{EventHandler, MainEventHandler, State};
use crate::theme::{ThemeState, ThemedWidget};
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Users;

impl EventHandler for Users {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Esc => {
                    view.app.set_mode(Mode::Normal);
                    view.hover_highlight();
                    view.app.users_widget_state.set_active(false)
                }
                KeyCode::Up => {
                    view.app.users_widget_state.next();
                }
                KeyCode::Down => {
                    view.app.users_widget_state.previous();
                }
                KeyCode::Tab => view.transition_enter(State::from(Recently {})),
                KeyCode::BackTab => view.transition_enter(State::from(Chat {})),
                KeyCode::Char('h') => {
                    if view.model.is_host.get_inner() {
                        if let Some(user) = view.app.users_widget_state.get_current_user() {
                            let current_name = view.model.user.get_inner().name;
                            if user.name == current_name {
                                let msg = PlayerMessage::from(PlayerMessageInner {
                                    message: "Cannot hand over host role to yourself"
                                        .to_string(),
                                    source: MessageSource::Server,
                                    level: MessageLevel::Warn,
                                    timestamp: Local::now(),
                                });
                                view.model.messages.rcu(|msgs| {
                                    let mut msgs = msgs.as_ref().clone();
                                    msgs.push(msg.clone());
                                    msgs
                                });
                            } else {
                                view.model.host_handover(user.name);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl MainEventHandler for Users {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Down | KeyCode::BackTab => view.transition(State::from(Chat {})),
            KeyCode::Right | KeyCode::Tab => view.transition(State::from(Recently {})),
            _ => {}
        }
    }

    fn set_state(&self, view: &mut RatatuiView, state: ThemeState) {
        view.app.users_widget_state.set_state(state);
    }
}
