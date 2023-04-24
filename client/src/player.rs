use std::sync::Arc;

use iced::Subscription;
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::sync::Mutex;

use crate::file_table::PlaylistWidgetMessage;
use crate::fs::DatabaseMessage;
use crate::mpv::event::MpvEvent;
use crate::window::MainMessage;
use crate::ws::WebSocketMessage;

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    WebSocket(WebSocketMessage),
    Mpv(MpvEvent),
    Database(DatabaseMessage),
    FileTable(PlaylistWidgetMessage),
    Heartbeat,
}

#[derive(Debug)]
pub struct Player {
    rec: Arc<Mutex<MpscReceiver<PlayerMessage>>>,
    send: Arc<MpscSender<PlayerMessage>>,
}

impl Player {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            rec: Arc::new(Mutex::new(rx)),
            send: Arc::new(tx),
        }
    }

    pub fn subscribe(&self) -> Subscription<MainMessage> {
        iced::subscription::unfold(
            std::any::TypeId::of::<Self>(),
            self.rec.clone(),
            |event_pipe| async move {
                let event = event_pipe.lock().await.recv().await;
                match event {
                    Some(PlayerMessage::WebSocket())
                    _ => {}
                }
                (MainMessage::PlayerUpdate, event_pipe)
            },
        )
    }
}
