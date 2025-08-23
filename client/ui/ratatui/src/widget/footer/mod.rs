use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Text,
    widgets::{Paragraph, StatefulWidget, Widget},
};

use crate::{
    handler::{OverlayState, State},
    view::Mode,
};

pub struct FooterWidget;

#[derive(Debug, Clone)]
pub struct FooterWidgetState {
    content: String,
    style: Style,
}

impl Default for FooterWidgetState {
    fn default() -> Self {
        Self {
            content: "".to_string(),
            style: Style::default().fg(Color::Magenta),
        }
    }
}

impl FooterWidgetState {
    pub fn set_content(
        &mut self,
        state: &State,
        overlay_state: &Option<OverlayState>,
        mode: &Mode,
    ) {
        match (mode, overlay_state) {
            (Mode::Inspecting, _) => {
                self.style = Style::default().fg(Color::Cyan);
            }
            (Mode::Overlay, Some(OverlayState::Option(_))) => {
                self.style = Style::default().fg(Color::Magenta);
            }
            (Mode::Overlay, _) => {
                self.style = Style::default().fg(Color::Cyan);
            }
            (_, _) => {
                self.style = Style::default().fg(Color::Magenta);
            }
        }

        match (mode, state, overlay_state) {
            (Mode::Normal, _, _) => {
                self.content =
                    "← → ↑ ↓: navigate, enter: select widget, space: options, ?: help, q: quit"
                        .to_string();
            }
            (Mode::Inspecting, State::Chat(_), _) => {
                self.content = "↑ ↓: navigate, ?: help, esc: back".to_string();
            }
            (Mode::Inspecting, State::ChatInput(_), _) => {
                self.content = "any key: input, esc: back".to_string();
            }
            (Mode::Inspecting, State::Users(_), _) => {
                self.content = "↑ ↓: navigate, esc: back".to_string();
            }
            (Mode::Inspecting, State::Recently(_), _) => {
                self.content = "↑ ↓: navigate, enter: select, x: extend, Tab: navigate tab, ?: help, esc: back".to_string();
            }
            (Mode::Inspecting, State::Playlist(_), _) => {
                self.content =
                    "↑ ↓: navigate, enter: select, x: extend, d: delete, p: paste, ?: help, esc: back"
                        .to_string();
            }
            (Mode::Overlay, _, Some(OverlayState::Login(_))) => {
                self.content = "↑ ↓: navigate, enter: join room".to_string();
            }
            (Mode::Overlay, _, Some(OverlayState::BrowserSearch(_))) => {
                self.content =
                    "↑ ↓: navigate, enter: select, ctrl + x: extend, esc: back".to_string();
            }
            (Mode::Overlay, _, Some(OverlayState::PlaylistSearch(_))) => {
                self.content =
                    "↑ ↓: navigate, enter: move, ctrl + x: extend, ctrl + d: remove, ctrl + j: jump, esc: back".to_string();
            }
            (Mode::Overlay, _, Some(OverlayState::MediaDir(_))) => {
                self.content = "↑ ↓: navigate, enter: add, ctrl + d: remove, esc: back".to_string();
            }
            (Mode::Overlay, _, Some(OverlayState::PlaylistBrowser(_))) => {
                self.content = "↑ ↓: navigate, enter: select, esc: back".to_string();
            }
            (Mode::Overlay, _, _) => {}
        }
    }
}

impl StatefulWidget for FooterWidget {
    type State = FooterWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let horizontal_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(2)].as_ref())
            .split(area);

        let content = &state.content;
        let nav_description = Paragraph::new(Text::raw(content)).style(state.style);
        nav_description.render(horizontal_split[1], buf);
    }
}
