use iced::Element;
use iced::widget::{center, mouse_area, opaque, stack};

use crate::message::Message;

pub mod chat;
pub mod database;
pub mod file_search;
pub mod playlist;
pub mod rooms;
pub mod settings;

const MODAL_PADDING: f32 = 20.0;

/// Lays `content` centered on top of `base`, blocking interaction with
/// `base` and turning clicks next to `content` into `on_blur`.
pub fn modal<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    on_blur: Message,
) -> Element<'a, Message> {
    stack![
        base,
        opaque(mouse_area(center(opaque(content)).padding(MODAL_PADDING)).on_press(on_blur))
    ]
    .into()
}
