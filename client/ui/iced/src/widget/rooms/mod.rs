use iced::widget::{Column, Container, Id, Row, Scrollable, Space, Text, row};
use iced::{Element, Length};
use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;

use crate::message::Message;

pub fn view<'a>(
    state: &'a UsersWidgetState,
    this_user: &UserStatus,
    is_host: bool,
) -> Element<'a, Message> {
    let elements: Vec<_> = state
        .users
        .iter()
        .map(|u| {
            let label = u.to_text(this_user, is_host);
            let content = Container::new(label).padding(2).width(Length::Fill);
            row!(Space::new().width(Length::Fixed(5.0)), content).into()
        })
        .collect();

    Scrollable::new(Column::with_children(elements).width(Length::Fill))
        .id(Id::new("rooms"))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

#[derive(Debug, Clone, Default)]
pub struct UsersWidgetState {
    users: UserList,
}

impl UsersWidgetState {
    pub fn replace_users(&mut self, users: UserList) {
        self.users = users;
    }

    pub fn room_name(&self) -> &str {
        self.users.get_room_name()
    }
}

trait UserStatusExt {
    fn to_text<'a>(&self, user: &UserStatus, is_host: bool) -> Row<'a, Message>;
}

impl UserStatusExt for UserStatus {
    fn to_text<'a>(&self, user: &UserStatus, is_host: bool) -> Row<'a, Message> {
        let mut row = Row::new();
        if self.name.eq(&user.name) {
            let role = if is_host { "host" } else { "client" };
            row = row.push(Text::new(format!("(me, {role}) ")));
        }
        let ready = match self.ready {
            true => Text::new("Ready").style(iced::widget::text::success),
            false => Text::new("Not Ready").style(iced::widget::text::danger),
        };
        row.push(Text::new(format!("{}: ", self.name))).push(ready)
    }
}
