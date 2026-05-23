use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Modifier;
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;
use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

#[derive(Debug, Default, Clone)]
pub struct UsersWidget;

#[derive(Debug, Default, Clone)]
pub struct UsersWidgetState {
    user_list: UserList,
    user: UserStatus,
    is_host: bool,
    list_state: ListStateWrapper,
    vertical_scroll_state: ScrollbarState,
    scroll_length: usize,
    theme: ThemeWrapper,
    active: bool,
}

impl ThemedWidget for UsersWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl UsersWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme: ThemeWrapper::new(theme),
            ..Default::default()
        }
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

    pub fn set_is_host(&mut self, is_host: bool) {
        self.is_host = is_host;
    }

    pub fn toggle_ready(&mut self) {
        self.user.ready = !self.user.ready;
    }

    pub fn next(&mut self) {
        self.list_state.next();
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
        self.set_active(true);
    }

    pub fn previous(&mut self) {
        self.list_state.limited_previous(self.scroll_length);
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
        self.set_active(true);
    }

    pub fn get_current_user(&self) -> Option<UserStatus> {
        match self.list_state.selected() {
            Some(index) => self.user_list.get(index),
            None => None,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }
}

impl StatefulWidget for UsersWidget {
    type State = UsersWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let style = state.theme.style();

        let rooms: Vec<ListItem> = state
            .user_list
            .iter()
            .map(|u| {
                let is_me = u.eq(&state.user);
                let is_host_user = state.user_list.is_host_name(&u.name);
                let row_style = if u.ready { style.green() } else { style.red() };

                let mut spans: Vec<Span> = Vec::with_capacity(4);
                spans.push(Span::raw(u.name.to_string()));
                if is_me {
                    spans.push(Span::raw(" (me)"));
                }
                if is_host_user {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "(host)",
                        style.yellow().add_modifier(Modifier::BOLD),
                    ));
                }
                ListItem::new(vec![Line::from(spans).style(row_style)])
            })
            .collect();

        let role_span = if state.is_host {
            Span::styled(
                " [HOST]",
                style.green().add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" [CLIENT]", style.add_modifier(Modifier::DIM))
        };
        let footer = Line::from(vec![
            Span::raw(format!("({})", state.user_list.len())),
            role_span,
        ])
        .right_aligned();

        let messages_block = Block::default()
            .style(style)
            .title(format!("Users in room {}", state.user_list.get_room_name()))
            .title_bottom(footer)
            .borders(Borders::ALL);

        let mut rooms_list = List::new(rooms).block(messages_block);

        if state.active {
            rooms_list = rooms_list.highlight_style(state.theme.highlight());
        }

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
