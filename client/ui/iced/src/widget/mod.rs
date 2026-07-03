use iced::Element;
use iced::widget::{center, mouse_area, opaque, stack};

use crate::message::Message;

pub mod chat;
pub mod database;
pub mod file_search;
pub mod playlist;
pub mod rooms;
pub mod settings;
pub mod user_actions;

const MODAL_PADDING: f32 = 20.0;

/// Lays an optional modal centered on top of `base`, blocking
/// interaction with `base` and turning clicks next to the content into
/// its `on_blur` message. The stack is kept even without a modal, so
/// that opening one doesn't change the shape of the widget tree (which
/// would reset widget state such as scroll positions in `base`).
pub fn modal<'a>(
    base: Element<'a, Message>,
    overlay: Option<(Element<'a, Message>, Message)>,
) -> Element<'a, Message> {
    let layers = stack![base];
    match overlay {
        Some((content, on_blur)) => layers
            .push(opaque(
                mouse_area(center(opaque(content)).padding(MODAL_PADDING)).on_press(on_blur),
            ))
            .into(),
        None => layers.into(),
    }
}
