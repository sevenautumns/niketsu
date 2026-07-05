use arcstr::ArcStr;
use enum_dispatch::enum_dispatch;
use iced::Task;
use niketsu_core::ui::UiModel;

use super::UserActionsWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait UserActionsWidgetMessageTrait {
    fn handle(self, state: &mut UserActionsWidgetState, model: &UiModel) -> Task<Message>;
}

#[enum_dispatch(UserActionsWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum UserActionsWidgetMessage {
    Open,
    Close,
    Handover,
}

impl MessageHandler for UserActionsWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        UserActionsWidgetMessageTrait::handle(
            self,
            &mut model.user_actions_widget_state,
            &model.model,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Open {
    pub user: ArcStr,
}

impl UserActionsWidgetMessageTrait for Open {
    fn handle(self, state: &mut UserActionsWidgetState, _: &UiModel) -> Task<Message> {
        state.user = Some(self.user);
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct Close;

impl UserActionsWidgetMessageTrait for Close {
    fn handle(self, state: &mut UserActionsWidgetState, _: &UiModel) -> Task<Message> {
        state.user = None;
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct Handover;

impl UserActionsWidgetMessageTrait for Handover {
    fn handle(self, state: &mut UserActionsWidgetState, model: &UiModel) -> Task<Message> {
        if let Some(user) = state.user.take() {
            model.host_handover(user);
        }
        Task::none()
    }
}
