use std::path::PathBuf;

use async_trait::async_trait;

use super::user::RoomList;

#[async_trait]
pub trait UserInterface {
    fn file_database_status(&mut self, update_status: f32);
    fn file_database(&mut self, db: Vec<PathBuf>);
    fn playlist(&mut self, playlist: Vec<String>);
    fn room_list(&mut self, room_list: RoomList);
    // TODO
    async fn event(&mut self) -> UserInterfaceEvent;
}

#[derive(Debug, Clone)]
pub enum UserInterfaceEvent {
    PlaylistChange(PlaylistChange),
    // TODO
}

#[derive(Debug, Clone)]
pub struct PlaylistChange {
    playlist: Vec<String>,
}
