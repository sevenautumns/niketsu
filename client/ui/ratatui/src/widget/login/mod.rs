use crossterm::event::KeyEvent;
use niketsu_core::config::Config;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget, Wrap};

use super::{OverlayWidgetState, TextAreaWrapper};

pub struct LoginWidget;

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Password,
    Username,
    Room,
}

#[derive(Debug, Default, Clone)]
pub struct LoginWidgetState {
    current_state: State,
    password_field: TextAreaWrapper,
    username_field: TextAreaWrapper,
    room_field: TextAreaWrapper,
    style: Style,
}

impl LoginWidgetState {
    pub fn new(config: &Config) -> Self {
        Self {
            current_state: State::default(),
            password_field: TextAreaWrapper::new("Password".into(), config.password.clone()),
            username_field: TextAreaWrapper::new("Username".into(), config.username.to_string()),
            room_field: TextAreaWrapper::new("Room".into(), config.room.to_string()),
            style: Style::default().cyan(),
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
        let vert_width = match r.height {
            0..=50 => 5,
            51..=100 => 20,
            _ => 40,
        };

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(vert_width),
                    Constraint::Min(30),
                    Constraint::Percentage(vert_width),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(30),
                    Constraint::Min(30),
                    Constraint::Percentage(30),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for LoginWidget {
    type State = LoginWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default().title("Login").borders(Borders::ALL).gray();

        let text_block = Paragraph::new(Text::raw("Press Enter to submit."))
            .block(Block::default().borders(Borders::NONE).gray())
            .wrap(Wrap { trim: false });

        let room_field = state
            .room_field
            .with_default_style()
            .with_placeholder("Enter the room");
        let password_field = state
            .password_field
            .with_default_style()
            .with_mask("Enter your password");
        let username_field = state
            .username_field
            .with_default_style()
            .with_placeholder("Enter your username");

        let style = state.style;
        match state.current_state {
            State::Username => username_field.highlight(style, style.black().on_cyan()),
            State::Room => room_field.highlight(style, style.black().on_cyan()),
            State::Password => password_field.highlight(style, style.black().on_cyan()),
        };

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(1),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(area);

        outer_block.render(area, buf);
        text_block.render(layout[0], buf);
        username_field.render(layout[1], buf);
        room_field.render(layout[2], buf);
        password_field.render(layout[3], buf);
    }
}
