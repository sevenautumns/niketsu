use crossterm::event::{KeyCode, KeyEvent};
use niketsu_core::config::Config;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, StatefulWidget, Widget};

use super::{OverlayWidgetState, TextAreaWrapper};
use crate::theme::{Theme, ThemeSelection, ThemeWrapper, ThemedWidget};

pub struct SettingsWidget;

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Relay,
    Port,
    AutoConnect,
    AutoShare,
    ColorScheme,
}

#[derive(Debug, Clone)]
pub struct SettingsWidgetState {
    current_state: State,
    relay: TextAreaWrapper,
    port: TextAreaWrapper,
    auto_connect: bool,
    auto_share: bool,
    theme: ThemeWrapper,
    theme_selection: ThemeSelection,
}

impl ThemedWidget for SettingsWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl SettingsWidgetState {
    pub fn new(config: &Config, theme_selection: &ThemeSelection) -> Self {
        let theme = theme_selection.theme();
        Self {
            current_state: State::default(),
            relay: TextAreaWrapper::new(
                Some("Relay".into()),
                Some(config.relay.clone()),
                theme,
                true,
            ),
            port: TextAreaWrapper::new(
                Some("Port".into()),
                Some(config.port.clone().to_string()),
                theme,
                true,
            ),
            auto_connect: config.auto_connect,
            auto_share: config.auto_share,
            theme: ThemeWrapper::new(theme),
            theme_selection: theme_selection.clone(),
        }
    }

    pub fn previous_state(&mut self) {
        match self.current_state {
            State::Relay => self.current_state = State::ColorScheme,
            State::Port => self.current_state = State::Relay,
            State::AutoConnect => self.current_state = State::Port,
            State::AutoShare => self.current_state = State::AutoConnect,
            State::ColorScheme => self.current_state = State::AutoShare,
        }
    }

    pub fn next_state(&mut self) {
        match self.current_state {
            State::Relay => self.current_state = State::Port,
            State::Port => self.current_state = State::AutoConnect,
            State::AutoConnect => self.current_state = State::AutoShare,
            State::AutoShare => self.current_state = State::ColorScheme,
            State::ColorScheme => self.current_state = State::Relay,
        }
    }

    pub fn set_theme_selection(&mut self, theme_selection: ThemeSelection) {
        self.theme_selection = theme_selection
    }

    pub fn next_theme(&mut self) {
        if matches!(self.current_state, State::ColorScheme) {
            self.theme_selection = self.theme_selection.next()
        }
    }

    pub fn previous_theme(&mut self) {
        if matches!(self.current_state, State::ColorScheme) {
            self.theme_selection = self.theme_selection.previous()
        }
    }

    pub fn theme_selection(&self) -> ThemeSelection {
        self.theme_selection.clone()
    }

    pub fn handle_input(&mut self, key: KeyEvent) {
        match &self.current_state {
            State::Relay => {
                self.relay.input(key);
            }
            State::Port => {
                self.port.input(key);
            }
            s => {
                if let KeyCode::Char(' ') = key.code {
                    match s {
                        State::AutoConnect => self.auto_connect = !self.auto_connect,
                        State::AutoShare => self.auto_share = !self.auto_share,
                        _ => {}
                    }
                }
            }
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
        self.default_area(r)
    }
}

impl StatefulWidget for SettingsWidget {
    type State = SettingsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let options_block = Block::default()
            .title("Settings")
            .borders(Borders::ALL)
            .style(state.theme.style());
        let info_text = Paragraph::new(Text::raw("Save settings by pressing Enter."));

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(2),
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

        let auto_connect = create_bool_button(
            state.auto_connect,
            matches!(state.current_state, State::AutoConnect),
            "Auto connect",
            state.theme.inner(),
        );
        let auto_share = create_bool_button(
            state.auto_share,
            matches!(state.current_state, State::AutoShare),
            "Auto share",
            state.theme.inner(),
        );
        let theme = create_selection_button(
            state.theme_selection.to_string(),
            "Color scheme",
            matches!(state.current_state, State::ColorScheme),
            state.theme.inner(),
        );
        let relay_field = state
            .relay
            .with_style(state.theme.inner())
            .with_placeholder("Relay");
        let port_field = state
            .port
            .with_style(state.theme.inner())
            .with_placeholder("Port");

        let block_style = state.theme.highlight_fg();
        let cursor_style = state.theme.highlight();
        match state.current_state {
            State::Relay => relay_field.highlight(block_style, cursor_style),
            State::Port => port_field.highlight(block_style, cursor_style),
            _ => {}
        }

        options_block.render(area, buf);
        info_text.render(layout[0], buf);
        relay_field.render(layout[1], buf);
        port_field.render(layout[2], buf);
        auto_connect.render(layout[3], buf);
        auto_share.render(layout[4], buf);
        theme.render(layout[5], buf);
    }
}

fn create_bool_button<'a>(
    condition: bool,
    highlight: bool,
    title: &'a str,
    theme: Theme,
) -> Paragraph<'a> {
    let mut block = Block::default().title(title).borders(Borders::ALL);
    if highlight {
        block = block.border_style(theme.highlight_fg());
    }

    match condition {
        true => {
            block = block.style(theme.base().green());
            Paragraph::new(Text::raw("On")).block(block)
        }
        false => {
            block = block.style(theme.base().red());
            Paragraph::new(Text::raw("Off")).block(block)
        }
    }
}

fn create_selection_button<'a>(
    name: String,
    title: &'a str,
    highlight: bool,
    theme: Theme,
) -> Paragraph<'a> {
    let mut block = Block::default().title(title).borders(Borders::ALL);
    if highlight {
        block = block.style(theme.highlight_fg());
    } else {
        block = block.style(theme.base());
    }
    Paragraph::new(Text::raw(name)).block(block)
}
