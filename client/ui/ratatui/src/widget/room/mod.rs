use niketsu_core::rooms::{RoomList, RoomName};
use niketsu_core::user::UserStatus;
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::Title;
use ratatui::widgets::{
    Block, Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;

#[derive(Debug, Default, Clone)]
pub struct RoomsWidget;

//TODO implement meaningful scrolling
#[derive(Debug, Default, Clone)]
pub struct RoomsWidgetState {
    rooms: RoomList,
    user: UserStatus,
    list_state: ListStateWrapper,
    vertical_scroll_state: ScrollbarState,
    scroll_length: usize,
    style: Style,
}

impl RoomsWidgetState {
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn set_rooms(&mut self, rooms: RoomList) {
        self.rooms = rooms;
        self.scroll_length = self.scroll_length()
    }

    fn scroll_length(&self) -> usize {
        self.rooms
            .iter()
            .map(|(_, users)| users.into_iter().count().saturating_add(1))
            .sum()
    }

    pub fn set_user(&mut self, user: UserStatus) {
        self.user = user;
    }

    pub fn next(&mut self) {
        self.list_state.next();
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i as u16);
        }
    }

    pub fn previous(&mut self) {
        self.list_state.limited_previous(self.scroll_length);
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i as u16);
        }
    }

    pub fn get_current_room(&self) -> Option<RoomName> {
        match self.list_state.selected() {
            Some(index) => {
                let mut count = 0;
                let mut current_room: Option<RoomName> = None;
                for (room_name, users) in self.rooms.iter() {
                    count += users.into_iter().count().saturating_add(1);
                    if index < count {
                        current_room = Some(room_name.clone());
                        break;
                    }
                }
                current_room
            }
            None => None,
        }
    }
}

impl StatefulWidget for RoomsWidget {
    type State = RoomsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let rooms: Vec<ListItem> = state
            .rooms
            .iter()
            .flat_map(|(room_name, users)| {
                let mut user_lines: Vec<ListItem> = users
                    .into_iter()
                    .map(|u| {
                        let name = match u.eq(&state.user) {
                            true => format!("{} (me)", u.name),
                            false => u.name.clone(),
                        };
                        let user_line = match u.ready {
                            true => ListItem::new(vec![Line::styled(
                                format!("    {}", name),
                                Style::default().fg(Color::Green),
                            )]),
                            false => ListItem::new(vec![Line::styled(
                                format!("    {}", name),
                                Style::default().fg(Color::Red),
                            )]),
                        };
                        user_line
                    })
                    .collect();
                let room_line = ListItem::new(vec![Line::from(room_name.to_string())]);
                user_lines.insert(0, room_line);
                user_lines
            })
            .collect();

        let messages_block = Block::default()
            .style(state.style)
            .title(Title::from("Rooms"))
            .borders(Borders::ALL);

        let rooms_list = List::new(rooms.clone())
            .gray()
            .block(messages_block)
            .highlight_style(Style::default().fg(Color::Cyan));

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        StatefulWidget::render(rooms_list, area, buf, state.list_state.inner());

        let mut scroll_state = state.vertical_scroll_state;
        scroll_state = scroll_state.content_length(state.scroll_length as u16);
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut scroll_state,
        );
    }
}
