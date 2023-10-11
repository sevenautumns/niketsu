use niketsu_core::rooms::RoomList;
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

//TODO roomswitch
//TODO update user status
#[derive(Debug, Default, Clone)]
pub struct RoomsWidget;

#[derive(Debug, Default, Clone)]
pub struct RoomsWidgetState {
    rooms: RoomList,
    user: UserStatus,
    list_state: ListStateWrapper,
    vertical_scroll_state: ScrollbarState,
    style: Style,
}

impl RoomsWidgetState {
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn set_rooms(&mut self, rooms: RoomList) {
        self.rooms = rooms;
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
        self.list_state.limited_previous(self.rooms.len());
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i as u16);
        }
    }

    // fn get_current_room(&self) -> Option<&RoomName> {
    //     match self.state.state.selected() {
    //         Some(index) => self.state.rooms.get_room_name(index),
    //         None => None,
    //     }
    // }
}

impl StatefulWidget for RoomsWidget {
    type State = RoomsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        //TODO highlight room of own user
        //TODO scrolling
        //TODO change room
        //TODO split listitems since they cannot be display if too large
        let rooms: Vec<ListItem> = state
            .rooms
            .iter()
            .map(|(room_name, users)| {
                let room_line = Line::from(vec![room_name.gray()]);
                let mut user_lines: Vec<Line> = users
                    .into_iter()
                    .map(|u| {
                        let name = match u.eq(&state.user) {
                            true => format!("{} (me)", u.name),
                            false => u.name.clone(),
                        };
                        let mut user_line = match u.ready {
                            true => Line::from(format!("  {} (ready)", name)),
                            false => Line::from(format!("  {} (not ready)", name)),
                        };
                        user_line.patch_style(Style::default().fg(Color::Gray));
                        user_line
                    })
                    .collect();
                user_lines.insert(0, room_line);
                ListItem::new(user_lines)
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

        let mut state = state.vertical_scroll_state;
        state = state.content_length(rooms.len() as u16);
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
