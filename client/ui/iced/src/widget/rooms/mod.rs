use iced::widget::{Button, Column, Container, Id, Row, Scrollable, Space, Text, row};
use iced::{Element, Length};
use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;

use crate::message::Message;
use crate::styling::FileButton;
use crate::widget::user_actions::message::{Open, UserActionsWidgetMessage};

pub fn view<'a>(
    state: &'a UsersWidgetState,
    this_user: &UserStatus,
    is_host: bool,
) -> Element<'a, Message> {
    let elements: Vec<_> = state
        .users
        .iter()
        .map(|u| {
            let name = u.name.clone();
            let mut button = Button::new(Container::new(u.to_text(this_user)).padding(2))
                .padding(0)
                .width(Length::Fill)
                .style(FileButton::theme(false, true));
            if is_host && u.name != this_user.name {
                button =
                    button.on_press(UserActionsWidgetMessage::from(Open { user: name }).into());
            }
            row!(Space::new().width(Length::Fixed(5.0)), button).into()
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
}

trait UserStatusExt {
    fn to_text<'a>(&self, user: &UserStatus) -> Row<'a, Message>;
}

impl UserStatusExt for UserStatus {
    fn to_text<'a>(&self, user: &UserStatus) -> Row<'a, Message> {
        let mut row = Row::new();
        if self.name.eq(&user.name) {
            row = row.push(Text::new("(me) "));
        }
        let ready = match self.ready {
            true => Text::new("Ready").style(iced::widget::text::success),
            false => Text::new("Not Ready").style(iced::widget::text::danger),
        };
        row.push(Text::new(format!("{}: ", self.name))).push(ready)
    }
}
