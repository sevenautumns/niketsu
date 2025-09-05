use crossterm::event::KeyEvent;
use niketsu_core::config::Config;
use ratatui::layout::Flex;
use ratatui::prelude::{Buffer, Constraint, Layout, Rect};
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

#[derive(Debug, Clone)]
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
        let height = match r.height {
            0..=50 => 15,
            51..=100 => 30,
            _ => 60,
        };

        let width = r.width / 2;

        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(r);

        let [popup_layout] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        popup_layout
    }
}

impl StatefulWidget for LoginWidget {
    type State = LoginWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default().title("Login").borders(Borders::ALL).gray();

        let welcome_block = Paragraph::new(Text::raw("Welcome to niketsu."))
            .block(Block::default().borders(Borders::NONE).gray())
            .wrap(Wrap { trim: false });

        let info_block = Paragraph::new(Text::raw("Choose a username and join a room."))
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
