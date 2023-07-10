use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, Local};

use super::user::RoomList;

#[async_trait]
pub trait UserInterface {
    fn file_database_status(&mut self, update_status: f32);
    fn file_database(&mut self, db: Vec<PathBuf>);
    fn playlist(&mut self, playlist: Vec<String>);
    fn room_list(&mut self, room_list: RoomList);
    fn user_update(&mut self, user: UserChange);
    fn player_message(&mut self, msg: PlayerMessage);
    // TODO
    async fn event(&mut self) -> UserInterfaceEvent;
}

#[derive(Debug, Clone)]
pub enum UserInterfaceEvent {
    PlaylistChange(PlaylistChange),
    ServerChange(ServerChange),
    RoomChange(RoomChange),
    UserChange(UserChange),
    UserMessage(Message),
    // TODO
}

#[derive(Debug, Clone)]
pub struct PlaylistChange {
    pub playlist: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ServerChange {
    pub addr: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RoomChange {
    pub room: String,
}

#[derive(Debug, Clone)]
pub struct UserChange {
    pub name: String,
    pub ready: bool,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct PlayerMessage {
    pub message: Message,
    pub source: MessageSource,
    pub level: MessageLevel,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum MessageSource {
    User(String),
    Server,
    Internal,
}

#[derive(Debug, Copy, Clone)]
pub enum MessageLevel {
    Normal,
    Warn,
    Error,
}
