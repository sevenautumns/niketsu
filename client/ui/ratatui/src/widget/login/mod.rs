use crossterm::event::{KeyCode, KeyEvent};
use niketsu_core::config::Config;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget, Wrap};

use super::{OverlayWidgetState, TextAreaWrapper};

pub struct LoginWidget;

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Address,
    Password,
    Username,
    Room,
    Secure,
}

#[derive(Debug, Default, Clone)]
pub struct LoginWidgetState {
    current_state: State,
    address_field: TextAreaWrapper,
    password_field: TextAreaWrapper,
    username_field: TextAreaWrapper,
    room_field: TextAreaWrapper,
    secure: bool,
    style: Style,
}

impl LoginWidgetState {
    pub fn new(config: &Config) -> Self {
        Self {
            current_state: State::default(),
            address_field: TextAreaWrapper::new("Address", config.url.clone()),
            password_field: TextAreaWrapper::new("Password", config.password.clone()),
            username_field: TextAreaWrapper::new("Username", config.username.clone()),
            room_field: TextAreaWrapper::new("Room", config.room.clone()),
            secure: config.secure,
            style: Style::default().fg(Color::Cyan),
        }
    }

    pub fn previous_state(&mut self) {
        match self.current_state {
            State::Address => self.current_state = State::Secure,
            State::Password => self.current_state = State::Address,
            State::Username => self.current_state = State::Password,
            State::Room => self.current_state = State::Username,
            State::Secure => self.current_state = State::Room,
        }
    }

    pub fn next_state(&mut self) {
        match self.current_state {
            State::Address => self.current_state = State::Password,
            State::Password => self.current_state = State::Username,
            State::Username => self.current_state = State::Room,
            State::Room => self.current_state = State::Secure,
            State::Secure => self.current_state = State::Address,
        }
    }

    pub fn collect_input(&self) -> (String, bool, String, String, String) {
        let address = self.address_field.get_input();
        let room = self.room_field.get_input();
        let password = self.password_field.get_input();
        let username = self.username_field.get_input();
        (address, self.secure, password, room, username)
    }

    pub fn input(&mut self, key: KeyEvent) {
        match self.current_state {
            State::Address => {
                self.address_field.input(key);
            }
            State::Username => {
                self.username_field.input(key);
            }
            State::Room => {
                self.room_field.input(key);
            }
            State::Password => {
                self.password_field.input(key);
            }
            State::Secure => {
                if key.code == KeyCode::Char(' ') {
                    self.secure = !self.secure;
                }
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

        let mut address_field = state.address_field.clone().set_default_stye();
        let password_field = state.password_field.clone().set_default_stye();
        let mut password_field = password_field.into_masked("Password");
        let mut username_field = state.username_field.clone().set_default_stye();
        let mut room_field = state.room_field.clone().set_default_stye();

        let mut secure_block = match state.secure {
            true => Paragraph::new(Text::raw("Secure: on"))
                .block(Block::default().borders(Borders::ALL).green()),
            false => Paragraph::new(Text::raw("Secure: off"))
                .block(Block::default().borders(Borders::ALL).red()),
        };

        let style = state.style;
        match state.current_state {
            State::Address => address_field.set_textarea_style(style, style.dark_gray().on_cyan()),
            State::Password => {
                password_field.set_textarea_style(style, style.dark_gray().on_cyan())
            }
            State::Username => {
                username_field.set_textarea_style(style, style.dark_gray().on_cyan())
            }
            State::Room => room_field.set_textarea_style(style, style.dark_gray().on_cyan()),
            State::Secure => secure_block = secure_block.style(style),
        };

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(1),
                    Constraint::Length(3),
                    Constraint::Length(3),
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
        address_field.widget().render(layout[1], buf);
        password_field.widget().render(layout[2], buf);
        username_field.widget().render(layout[3], buf);
        room_field.widget().render(layout[4], buf);
        secure_block.render(layout[5], buf);
    }
}
