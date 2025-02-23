use std::time::{Duration, Instant};

use iced::advanced::widget::Operation;
use iced::mouse::Cursor;
use iced::widget::scrollable::Id;
use iced::widget::{row, Button, Column, Container, Row, Scrollable, Space, Text};
use iced::{Element, Length, Rectangle, Renderer, Theme};
use niketsu_core::room::UserList;
use niketsu_core::user::UserStatus;

use crate::message::Message;
use crate::styling::FileButton;

// TODO make configurable
pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct RoomsWidget<'a> {
    base: Element<'a, Message>,
}

impl RoomsWidget<'_> {
    pub fn new(state: &UsersWidgetState, this_user: &UserStatus) -> Self {
        let elements: Vec<_> = state
            .users
            .iter()
            .map(|u| {
                row!(
                    Space::with_width(Length::Fixed(5.0)),
                    Button::new(Container::new(u.to_text(this_user)).padding(2))
                        .padding(0)
                        .width(Length::Fill)
                        .style(FileButton::theme(false, true)),
                )
                .into()
            })
            .collect();

        Self {
            base: Scrollable::new(Column::with_children(elements).width(Length::Fill))
                .id(Id::new("rooms"))
                .height(Length::Fill)
                .width(Length::Fill)
                .into(),
        }
    }
}

impl iced::advanced::Widget<Message, Theme, Renderer> for RoomsWidget<'_> {
    fn size(&self) -> iced::Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        )
    }

    fn on_event(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) -> iced::event::Status {
        self.base.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }
}

#[derive(Debug, Clone)]
pub struct UsersWidgetState {
    users: UserList,
    last_press: Instant,
    selected: String,
}

impl Default for UsersWidgetState {
    fn default() -> Self {
        Self {
            users: Default::default(),
            last_press: Instant::now(),
            selected: Default::default(),
        }
    }
}

impl UsersWidgetState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_users(&mut self, users: UserList) {
        self.users = users;
    }

    pub fn is_double_click(&mut self, user: String) -> bool {
        let mut double_click = false;
        if self.users.contains_user(&user) {
            if self.selected.eq(&user) && self.last_press.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                double_click = true;
            }
            self.last_press = Instant::now();
            self.selected = user;
        }
        double_click
    }
}

impl<'a> From<RoomsWidget<'a>> for Element<'a, Message> {
    fn from(table: RoomsWidget<'a>) -> Self {
        Self::new(table)
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
