use dyn_clone::DynClone;
use niketsu_core::ui::RoomChange;

use super::RoomsWidgetState;
use crate::message::Message;
use crate::UiModel;

pub trait RoomsWidgetMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, state: &mut RoomsWidgetState, model: &UiModel);
}

dyn_clone::clone_trait_object!(RoomsWidgetMessage);

#[derive(Debug, Clone)]
pub struct ClickRoom(pub String);

impl RoomsWidgetMessage for ClickRoom {
    fn handle(self: Box<Self>, state: &mut RoomsWidgetState, model: &UiModel) {
        if state.is_double_click(self.0.clone()) {
            model.change_room(RoomChange { room: self.0 })
        }
    }
}

impl From<ClickRoom> for Message {
    fn from(value: ClickRoom) -> Self {
        Message::RoomsWidget(Box::new(value))
    }
}
