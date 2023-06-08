#![allow(clippy::new_without_default)]

use async_trait::async_trait;

use crate::core::playlist::PlaylistVideo;
use crate::core::ui::*;
use crate::file_database::FileStore;
use crate::playlist::Playlist;
use crate::rooms::RoomList;

#[derive(Debug)]
pub struct TauriUI {}

impl TauriUI {
    pub fn new() -> Self {
        TauriUI {}
    }
}

#[async_trait]
impl UserInterfaceTrait for TauriUI {
    fn file_database_status(&mut self, _update_status: f32) {
        todo!()
    }

    fn file_database(&mut self, _db: FileStore) {
        todo!()
    }

    fn playlist(&mut self, _playlist: Playlist) {
        todo!()
    }

    fn video_change(&mut self, _video: Option<PlaylistVideo>) {
        todo!()
    }

    fn room_list(&mut self, _room_list: RoomList) {
        todo!()
    }

    fn user_update(&mut self, _user: UserChange) {
        todo!()
    }

    fn player_message(&mut self, _msg: PlayerMessage) {
        todo!()
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        todo!()
    }
}
