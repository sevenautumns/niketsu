use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use iced::widget::scrollable::Id;
use iced::widget::{row, Button, Column, Container, Scrollable, Space, Text};
use iced::{Element, Length, Renderer, Theme};
use iced_native::widget::Tree;
use iced_native::Widget;

use crate::file_table::MAX_DOUBLE_CLICK_INTERVAL;
use crate::styling::FileButton;
use crate::user::ThisUser;
use crate::window::MainMessage;
use crate::ws::UserStatus;

pub struct RoomsWidget<'a> {
    base: Element<'a, MainMessage>,
}

impl<'a> RoomsWidget<'a> {
    pub fn new(state: &'a RoomsWidgetState, this_user: &ThisUser, theme: &Theme) -> Self {
        let mut elements = vec![];
        let mut rooms: Vec<_> = state.rooms.iter().collect();
        rooms.sort_by_key(|(k, _)| k.to_string());
        for room in &state.rooms {
            let selected = state.selected.eq(room.0);
            elements.push(
                Button::new(Container::new(Text::new(room.0)).padding(2))
                    .on_press(MainMessage::Rooms(RoomsWidgetMessage::ClickRoom(
                        room.0.to_string(),
                    )))
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

impl<'a> Widget<MainMessage, Renderer> for RoomsWidget<'a> {
    fn width(&self) -> iced_native::Length {
        self.base.as_widget().width()
    }

    fn height(&self) -> iced_native::Length {
        self.base.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced_native::layout::Limits,
    ) -> iced_native::layout::Node {
        self.base.as_widget().layout(renderer, limits)
    }

    fn children(&self) -> Vec<iced_native::widget::Tree> {
        vec![Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut Tree,
        layout: iced_native::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced_native::widget::Operation<MainMessage>,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: iced_native::Layout<'_>,
        cursor_position: iced::Point,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced_native::mouse::Interaction {
        //TODO Change mouse interaction

        self.base.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor_position,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        state: &iced_native::widget::Tree,
        renderer: &mut Renderer,
        theme: &<Renderer as iced_native::Renderer>::Theme,
        style: &iced_native::renderer::Style,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
        viewport: &iced_native::Rectangle,
    ) {
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor_position,
            viewport,
        )
    }

    fn on_event(
        &mut self,
        state: &mut Tree,
        event: iced::Event,
        layout: iced_native::Layout<'_>,
        cursor_position: iced::Point,
        renderer: &Renderer,
        clipboard: &mut dyn iced_native::Clipboard,
        shell: &mut iced_native::Shell<'_, MainMessage>,
    ) -> iced::event::Status {
        self.base.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor_position,
            renderer,
            clipboard,
            shell,
        )
    }
}

#[derive(Debug, Clone)]
pub enum RoomsWidgetMessage {
    ClickRoom(String),
}

#[derive(Debug)]
pub struct RoomsWidgetState {
    rooms: BTreeMap<String, BTreeSet<UserStatus>>,
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

    pub fn replace_rooms(&mut self, rooms: BTreeMap<String, BTreeSet<UserStatus>>) {
        self.rooms = rooms
    }

    /// Returns if whether this is a double click or not
    pub fn click_room(&mut self, room: String) -> bool {
        let mut double_click = false;
        if self.rooms.contains_key(&room) {
            if self.selected.eq(&room) && self.last_press.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                double_click = true;
            }
            self.last_press = Instant::now();
            self.selected = room;
        }
        double_click
    }
}

impl<'a> From<RoomsWidget<'a>> for Element<'a, MainMessage> {
    fn from(table: RoomsWidget<'a>) -> Self {
        Self::new(table)
    }
}
