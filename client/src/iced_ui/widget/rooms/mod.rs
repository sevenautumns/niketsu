use std::time::{Duration, Instant};

use iced::mouse::Cursor;
use iced::widget::scrollable::Id;
use iced::widget::{row, Button, Column, Container, Row, Scrollable, Space, Text};
use iced::{Element, Length, Rectangle, Renderer, Theme};

use self::message::ClickRoom;
use crate::core::user::UserStatus;
use crate::iced_ui::message::Message;
use crate::rooms::RoomList;
use crate::styling::FileButton;

pub mod message;

// TODO make configurable
pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct RoomsWidget<'a> {
    base: Element<'a, Message>,
}

impl<'a> RoomsWidget<'a> {
    pub fn new(state: &RoomsWidgetState, this_user: &UserStatus, theme: &Theme) -> Self {
        let mut elements = vec![];
        let mut rooms: Vec<_> = state.rooms.iter().collect();
        rooms.sort_by_key(|(k, _)| k.to_string());
        for room in &state.rooms {
            let selected = state.selected.eq(room.0);
            elements.push(
                Button::new(Container::new(Text::new(room.0.clone())).padding(2))
                    .on_press(ClickRoom(room.0.to_string()).into())
                    .padding(0)
                    .width(Length::Fill)
                    .style(FileButton::theme(selected, true))
                    .into(),
            );
            for user in room.1 {
                elements.push(
                    row!(
                        Space::with_width(Length::Fixed(5.0)),
                        Button::new(Container::new(user.to_text(this_user, theme)).padding(2))
                            .padding(0)
                            .width(Length::Fill)
                            .style(FileButton::theme(false, true)),
                    )
                    .into(),
                )
            }
        }

        Self {
            base: Scrollable::new(Column::with_children(elements).width(Length::Fill))
                .id(Id::new("rooms"))
                .height(Length::Fill)
                .width(Length::Fill)
                .into(),
        }
    }
}

impl<'a> iced::advanced::Widget<Message, Renderer> for RoomsWidget<'a> {
    fn width(&self) -> Length {
        self.base.as_widget().width()
    }

    fn height(&self) -> Length {
        self.base.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base.as_widget().layout(renderer, limits)
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
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
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
        //TODO Change mouse interaction

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
pub struct RoomsWidgetState {
    rooms: RoomList,
    last_press: Instant,
    selected: String,
}

impl Default for RoomsWidgetState {
    fn default() -> Self {
        Self {
            rooms: Default::default(),
            last_press: Instant::now(),
            selected: Default::default(),
        }
    }
}

impl RoomsWidgetState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_rooms(&mut self, rooms: RoomList) {
        self.rooms = rooms;
    }

    /// Returns if whether this is a double click or not
    pub fn is_double_click(&mut self, room: String) -> bool {
        let mut double_click = false;
        if self.rooms.contains_room(&room) {
            if self.selected.eq(&room) && self.last_press.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                double_click = true;
            }
            self.last_press = Instant::now();
            self.selected = room;
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
    fn to_text<'a>(&self, user: &UserStatus, theme: &Theme) -> Row<'a, Message, Renderer>;
}

impl UserStatusExt for UserStatus {
    fn to_text<'a>(&self, user: &UserStatus, theme: &Theme) -> Row<'a, Message, Renderer> {
        let mut row = Row::new();
        if self.name.eq(&user.name) {
            row = row.push(Text::new("(me) "));
        }
        let ready = match self.ready {
            true => Text::new("Ready").style(theme.palette().success),
            false => Text::new("Not Ready").style(theme.palette().danger),
        };
        row.push(Text::new(format!("{}: ", self.name))).push(ready)
    }
}
