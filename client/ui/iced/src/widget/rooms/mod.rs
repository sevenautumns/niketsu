use iced::widget::{Button, Column, Container, Id, Row, Scrollable, Space, Text, row};
use iced::{Element, Length};
use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;

use crate::main_window::message::{HandoverButton, MainMessage};
use crate::message::Message;
use crate::styling::FileButton;

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
            let mut row = row!(
                Space::new().width(Length::Fixed(5.0)),
                Button::new(Container::new(u.to_text(this_user)).padding(2))
                    .padding(0)
                    .width(Length::Fill)
                    .style(FileButton::theme(false, true)),
            );
            if is_host && u.name != this_user.name {
                row = row.push(
                    Button::new(Text::new("→H"))
                        .padding(2)
                        .on_press(MainMessage::from(HandoverButton { username: name }).into())
                        .style(iced::widget::button::secondary),
                );
            }
            row.into()
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
