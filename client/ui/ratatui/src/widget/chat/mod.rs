use niketsu_core::ui::PlayerMessage;
use niketsu_core::user::UserStatus;
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span};
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{
    Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;

pub struct ChatWidget;

#[derive(Debug, Default, Clone)]
pub struct ChatWidgetState {
    vertical_scroll_state: ScrollbarState,
    list_state: ListStateWrapper,
    user: UserStatus,
    style: Style,
    messages: Vec<PlayerMessage>,
}

impl ChatWidgetState {
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn set_messages(&mut self, messages: Vec<PlayerMessage>) {
        self.messages = messages;
    }

    pub fn set_user(&mut self, user: UserStatus) {
        self.user = user;
    }

    pub fn next(&mut self) {
        self.list_state.next();
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn previous(&mut self) {
        self.list_state.overflowing_previous(self.messages.len());
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn update_cursor_latest(&mut self) {
        self.list_state
            .select(Some(self.messages.len().saturating_sub(1)));
        self.vertical_scroll_state = self
            .vertical_scroll_state
            .position(self.messages.len().saturating_sub(1));
    }
}

//TODO colour schemes for different users
impl StatefulWidget for ChatWidget {
    type State = ChatWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        use niketsu_core::ui::MessageSource::*;
        let messages: Vec<ListItem> = state
            .messages
            .iter()
            .map(|t| {
                let head_message = format!(" at {}:", t.timestamp.format("%H:%M:%S"));
                let head_line = match &t.source {
                    UserMessage(user_name) => {
                        let name = match state.user.eq(user_name) {
                            true => {
                                Span::styled(user_name, Style::default().italic().fg(Color::Green))
                            }
                            false => Span::raw(user_name),
                        };
                        Line::from(vec![name, Span::raw(head_message)])
                    }
                    UserAction(user_name) => {
                        let message = format!("User action of {user_name}{head_message}");
                        Line::styled(message, Style::default().fg(Color::LightMagenta))
                    }
                    Server => {
                        let message = format!("Server notification{head_message}");
                        Line::styled(message, Style::default().fg(Color::LightRed))
                    }
                    Internal => {
                        let message = format!("Internal notification{head_message}");
                        Line::styled(message, Style::default().fg(Color::Red))
                    }
                };
                let tail_message = textwrap::fill(t.message.as_str(), area.width as usize);
                let mut message_lines: Vec<Line> = tail_message
                    .split('\n')
                    .map(|l| Line::from(l.to_string()))
                    .collect();
                message_lines.insert(0, head_line);
                ListItem::new(message_lines)
            })
            .collect();

        let messages_block = Block::default()
            .style(state.style)
            .title(Title::from("Chat"))
            .borders(Borders::ALL);

        let messages_list = List::new(messages.clone())
            .gray()
            .block(messages_block)
            .highlight_style(Style::default().fg(Color::Cyan));

        StatefulWidget::render(messages_list, area, buf, state.list_state.inner());

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        let mut state = state.vertical_scroll_state;
        state = state.content_length(messages.len());
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }
}
