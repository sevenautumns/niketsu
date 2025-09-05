use crossterm::event::{KeyCode, KeyEvent};
use niketsu_core::config::Config;
use ratatui::layout::Flex;
use ratatui::prelude::{Buffer, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget};

use super::{OverlayWidgetState, TextAreaWrapper};

pub struct SettingsWidget;

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Relay,
    Port,
    AutoConnect,
    AutoShare,
}

#[derive(Debug, Clone)]
pub struct SettingsWidgetState {
    current_state: State,
    relay: TextAreaWrapper,
    port: TextAreaWrapper,
    auto_connect: bool,
    auto_share: bool,
    style: Style,
}

impl SettingsWidgetState {
    pub fn new(config: &Config) -> Self {
        Self {
            current_state: State::default(),
            relay: TextAreaWrapper::new("Relay".into(), config.relay.clone()),
            port: TextAreaWrapper::new("Port".into(), config.port.clone().to_string()),
            auto_connect: config.auto_connect,
            auto_share: config.auto_share,
            style: Style::default().cyan(),
        }
    }

    pub fn previous_state(&mut self) {
        match self.current_state {
            State::Relay => self.current_state = State::AutoShare,
            State::Port => self.current_state = State::Relay,
            State::AutoConnect => self.current_state = State::Port,
            State::AutoShare => self.current_state = State::AutoConnect,
        }
    }

    pub fn next_state(&mut self) {
        match self.current_state {
            State::Relay => self.current_state = State::Port,
            State::Port => self.current_state = State::AutoConnect,
            State::AutoConnect => self.current_state = State::AutoShare,
            State::AutoShare => self.current_state = State::Relay,
        }
    }

    pub fn handle_input(&mut self, key: KeyEvent) {
        match &self.current_state {
            State::Relay => {
                self.relay.input(key);
            }
            State::Port => {
                self.port.input(key);
            }
            s => if let KeyCode::Char(' ') = key.code { match s {
                State::AutoConnect => self.auto_connect = !self.auto_connect,
                State::AutoShare => self.auto_share = !self.auto_share,
                _ => {}
            } },
        }
    }

    pub fn collect_input(&self) -> (String, u16, bool, bool) {
        let relay = self.relay.get_input();
        let port = self.port.get_input().parse::<u16>().unwrap();
        (relay, port, self.auto_connect, self.auto_share)
    }
}

impl OverlayWidgetState for SettingsWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let height = match r.height {
            0..=50 => 20,
            51..=100 => 30,
            _ => 40,
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

impl StatefulWidget for SettingsWidget {
    type State = SettingsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let options_block = Block::default().title("Settings").borders(Borders::ALL);
        let info_text = Paragraph::new(Text::raw("Save settings by pressing Enter."));

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(2),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(area);

        let auto_connect = create_button(
            state.auto_connect,
            matches!(state.current_state, State::AutoConnect),
            "Auto connect",
        );
        let auto_share = create_button(
            state.auto_share,
            matches!(state.current_state, State::AutoShare),
            "Auto share",
        );
        let relay_field = state.relay.with_default_style().with_placeholder("Relay");
        let port_field = state.port.with_default_style().with_placeholder("Port");

        let style = state.style;
        match state.current_state {
            State::Relay => relay_field.highlight(style, style.black().on_cyan()),
            State::Port => port_field.highlight(style, style.black().on_cyan()),
            _ => {}
        }

        options_block.render(area, buf);
        info_text.render(layout[0], buf);
        relay_field.render(layout[1], buf);
        port_field.render(layout[2], buf);
        auto_connect.render(layout[3], buf);
        auto_share.render(layout[4], buf);
    }
}

fn create_button(condition: bool, highlight: bool, title: &str) -> Paragraph {
    let mut block = Block::default().title(title).borders(Borders::ALL);
    if highlight {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }

    

    match condition {
        true => {
            block = block.style(Style::default().green());
            Paragraph::new(Text::raw("On")).block(block)
        }
        false => {
            block = block.style(Style::default().red());
            Paragraph::new(Text::raw("Off")).block(block)
        }
    }
}
