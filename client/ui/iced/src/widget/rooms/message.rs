use enum_dispatch::enum_dispatch;
use iced::Command;
use niketsu_core::ui::{RoomChange, UiModel};

use super::RoomsWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait RoomsWidgetMessageTrait {
    fn handle(self, state: &mut RoomsWidgetState, model: &UiModel);
}

#[enum_dispatch(RoomsWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum RoomsWidgetMessage {
    ClickRoom,
}

impl MessageHandler for RoomsWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        RoomsWidgetMessageTrait::handle(self, &mut model.rooms_widget_state, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct ClickRoom(pub String);

impl RoomsWidgetMessageTrait for ClickRoom {
    fn handle(self, state: &mut RoomsWidgetState, model: &UiModel) {
        if state.is_double_click(self.0.clone()) {
            model.change_room(RoomChange { room: self.0 })
        }
    }
}
