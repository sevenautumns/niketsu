use crossterm::event::KeyEvent;
use niketsu_core::config::Config;
use ratatui::prelude::{Buffer, Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Text;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget, Wrap};

use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

use super::{OverlayWidgetState, TextAreaWrapper};

pub struct LoginWidget;

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Password,
    Username,
    Room,
}

#[derive(Debug, Clone)]
pub struct LoginWidgetState {
    current_state: State,
    password_field: TextAreaWrapper,
    username_field: TextAreaWrapper,
    room_field: TextAreaWrapper,
    theme: ThemeWrapper,
}

impl ThemedWidget for LoginWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl LoginWidgetState {
    pub fn new(config: &Config, theme: Theme) -> Self {
        Self {
            current_state: State::default(),
            password_field: TextAreaWrapper::new(
                Some("Password".to_string()),
                Some(config.password.clone()),
                theme,
                true,
            ),
            username_field: TextAreaWrapper::new(
                Some("Username".to_string()),
                Some(config.username.to_string()),
                theme,
                true,
            ),
            room_field: TextAreaWrapper::new(
                Some("Room".to_string()),
                Some(config.room.to_string()),
                theme,
                true,
            ),
            theme: ThemeWrapper::new(theme),
        }
    }

    pub fn previous_state(&mut self) {
        match self.current_state {
            State::Username => self.current_state = State::Password,
            State::Room => self.current_state = State::Username,
            State::Password => self.current_state = State::Room,
        }
    }

    pub fn next_state(&mut self) {
        match self.current_state {
            State::Username => self.current_state = State::Room,
            State::Room => self.current_state = State::Password,
            State::Password => self.current_state = State::Username,
        }
    }

    pub fn collect_input(&self) -> (String, String, String) {
        let room = self.room_field.get_input();
        let password = self.password_field.get_input();
        let username = self.username_field.get_input();
        (room, password, username)
    }

    pub fn input(&mut self, key: KeyEvent) {
        match self.current_state {
            State::Username => {
                self.username_field.input(key);
            }
            State::Room => {
                self.room_field.input(key);
            }
            State::Password => {
                self.password_field.input(key);
            }
        }
    }
}

impl OverlayWidgetState for LoginWidgetState {
    fn area(&self, r: Rect) -> Rect {
        self.default_area(r)
    }
}

impl StatefulWidget for LoginWidget {
    type State = LoginWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default()
            .title("Login")
            .borders(Borders::ALL)
            .style(state.theme.style());

        let welcome_block = Paragraph::new(Text::raw("Welcome to niketsu."))
            .block(Block::default().borders(Borders::NONE).gray())
            .wrap(Wrap { trim: false })
            .style(state.theme.style());

        let info_block = Paragraph::new(Text::raw("Choose a username and join a room."))
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: false })
            .style(state.theme.style());

        let room_field = state
            .room_field
            .with_style(state.theme.inner())
            .with_placeholder("Enter the room");
        let password_field = state
            .password_field
            .with_style(state.theme.inner())
            .with_mask("Enter your password");
        let username_field = state
            .username_field
            .with_style(state.theme.inner())
            .with_placeholder("Enter your username");

        let block_style = state.theme.highlight_fg();
        let cursor_style = state.theme.highlight();
        match state.current_state {
            State::Username => username_field.highlight(block_style, cursor_style),
            State::Room => room_field.highlight(block_style, cursor_style),
            State::Password => password_field.highlight(block_style, cursor_style),
        };

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(area);

        outer_block.render(area, buf);
        welcome_block.render(layout[0], buf);
        info_block.render(layout[1], buf);
        username_field.render(layout[2], buf);
        room_field.render(layout[3], buf);
        password_field.render(layout[4], buf);
    }
}
