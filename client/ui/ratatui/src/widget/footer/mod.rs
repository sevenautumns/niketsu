use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};

use crate::handler::{OverlayState, State};
use crate::theme::{Theme, ThemeWrapper, ThemedWidget};
use crate::view::Mode;

pub struct FooterWidget;

#[derive(Debug, Clone, Default)]
pub struct FooterWidgetState {
    content: String,
    theme: ThemeWrapper,
    style: Style,
}

impl ThemedWidget for FooterWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl FooterWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            content: "".to_string(),
            theme: ThemeWrapper::new(theme),
            style: theme.base(),
        }
    }

    pub fn set_content(
        &mut self,
        state: &State,
        overlay_state: &Option<OverlayState>,
        mode: &Mode,
    ) {
        match (mode, overlay_state) {
            (Mode::Inspecting, _) => {
                self.style = self.theme.highlight_fg();
            }
            (Mode::Overlay, Some(OverlayState::Option(_))) => {
                self.style = self.theme.highlight_fg();
            }
            (Mode::Overlay, _) => {
                self.style = self.theme.accent();
            }
            (_, _) => {
                self.style = self.theme.accent();
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
                    "↑ ↓: navigate, enter: select, x: extend, d: delete, p: paste, /: search, ?: help, esc: back"
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
            (Mode::Overlay, _, Some(OverlayState::Settings(_))) => {
                self.content =
                    "↑ ↓: navigate, ← →: choose, space: toggle, enter: select, ctrl + d: reset, esc: back"
                        .to_string();
            }
            (Mode::Overlay, _, _) => {}
        }
    }
}

impl StatefulWidget for FooterWidget {
    type State = FooterWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let content = &state.content;
        let nav_description =
            Paragraph::new(Text::raw(format!("{}{}", " ", content))).style(state.style);
        nav_description.render(area, buf);
    }
}
