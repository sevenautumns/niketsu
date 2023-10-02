use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use super::{OverlayWidgetState, TextAreaWrapper};

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
pub struct LoginWidget<'a> {
    current_state: State,
    address_field: TextAreaWrapper<'a>,
    password_field: TextAreaWrapper<'a>,
    username_field: TextAreaWrapper<'a>,
    room_field: TextAreaWrapper<'a>,
    secure: bool,
    style: Style,
}

impl<'a> LoginWidget<'a> {
    pub fn new() -> Self {
        Self {
            current_state: State::default(),
            address_field: TextAreaWrapper::new("Address"),
            password_field: TextAreaWrapper::new("Password"),
            username_field: TextAreaWrapper::new("Username"),
            room_field: TextAreaWrapper::new("Room"),
            secure: true,
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

    pub fn collect_input(&self) -> (String, String, String, bool) {
        let address = self.address_field.lines().join("");
        let room = self.room_field.lines().join("");
        let password = self.password_field.lines().join("");
        (address, room, password, self.secure)
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

impl<'a> OverlayWidgetState for LoginWidget<'a> {
    fn area(&self, r: Rect) -> Rect {
        let vert_width = match r.height {
            0..=50 => 10,
            51..=100 => 25,
            _ => 45,
        };

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(vert_width),
                    Constraint::Percentage(100 - 2 * vert_width),
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
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl<'a> Widget for LoginWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer_block = Block::default().title("Login").borders(Borders::ALL).gray();

        let text_block = Paragraph::new(Text::raw("Press Enter to submit."))
            .block(Block::default().borders(Borders::NONE).gray())
            .wrap(Wrap { trim: false });

        let mut address_field = self.address_field.clone();
        let password_field = self.password_field.clone();
        let mut password_field = password_field.into_masked("Password");
        let mut username_field = self.username_field.clone();
        let mut room_field = self.room_field.clone();

        let mut secure_block = match self.secure {
            true => Paragraph::new(Text::raw("Secure: on"))
                .block(Block::default().borders(Borders::ALL).green()),
            false => Paragraph::new(Text::raw("Secure: off"))
                .block(Block::default().borders(Borders::ALL).red()),
        };

        let style = self.style;
        match self.current_state {
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
