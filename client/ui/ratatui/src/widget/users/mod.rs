use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{
    Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;

#[derive(Debug, Default, Clone)]
pub struct UsersWidget;

//TODO implement meaningful scrolling
#[derive(Debug, Default, Clone)]
pub struct UsersWidgetState {
    user_list: UserList,
    user: UserStatus,
    list_state: ListStateWrapper,
    vertical_scroll_state: ScrollbarState,
    scroll_length: usize,
    hightlight_style: Style,
    style: Style,
}

impl UsersWidgetState {
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
        self.hightlight_style = Style::default().fg(Color::Cyan);
    }

    pub fn set_user_list(&mut self, user_list: UserList) {
        self.user_list = user_list;
        self.scroll_length = self.scroll_length()
    }

    fn scroll_length(&self) -> usize {
        self.user_list.len()
    }

    pub fn set_user(&mut self, user: UserStatus) {
        self.user = user;
    }

    pub fn toggle_ready(&mut self) {
        self.user.ready = !self.user.ready
    }

    pub fn next(&mut self) {
        self.list_state.next();
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn previous(&mut self) {
        self.list_state.limited_previous(self.scroll_length);
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn get_current_user(&self) -> Option<UserStatus> {
        match self.list_state.selected() {
            Some(index) => self.user_list.get(index),
            None => None,
        }
    }

    pub fn reset(&mut self) {
        self.hightlight_style = Style::default();
    }
}

impl StatefulWidget for UsersWidget {
    type State = UsersWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let rooms: Vec<ListItem> = state
            .user_list
            .iter()
            .map(|u| {
                let name = match u.eq(&state.user) {
                    true => arcstr::format!("{} (me)", u.name),
                    false => u.name.clone(),
                };
                let user_line = match u.ready {
                    true => ListItem::new(vec![Line::styled(
                        name.to_string(),
                        Style::default().fg(Color::Green),
                    )]),
                    false => ListItem::new(vec![Line::styled(
                        name.to_string(),
                        Style::default().fg(Color::Red),
                    )]),
                };
                user_line
            })
            .collect();

        let messages_block = Block::default()
            .style(state.style)
            .title(Title::from(format!(
                "Users in room {}",
                state.user_list.get_room_name()
            )))
            .title_bottom(Line::from(format!("({})", state.user_list.len())).right_aligned())
            .borders(Borders::ALL);

        let rooms_list = List::new(rooms)
            .gray()
            .block(messages_block)
            .highlight_style(state.hightlight_style);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        StatefulWidget::render(rooms_list, area, buf, state.list_state.inner());

        let mut scroll_state = state.vertical_scroll_state;
        scroll_state = scroll_state.content_length(state.scroll_length);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut scroll_state,
        );
    }
}
