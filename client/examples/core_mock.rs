#![allow(dead_code)]

use std::path::PathBuf;

use chrono::Local;
use log::info;
use niketsu::core::file_database::FileEntry;
use niketsu::core::ui::{
    MessageLevel, MessageSource, PlayerMessageInner, UserChange, UserInterfaceEvent,
    UserInterfaceTrait,
};
use niketsu::core::user::UserStatus;
use niketsu::file_database::FileStore;

pub struct CoreMock<T: UserInterfaceTrait> {
    ui: T,
    file_database_status: f32,
    file_database: FileStore,
    playlist: Vec<String>,
    room_list: Vec<UserStatus>,
    user: UserChange,
}

impl<T: UserInterfaceTrait> CoreMock<T> {
    pub fn new(ui: T) -> Self {
        Self {
            ui,
            file_database_status: 0.0,
            file_database: FileStore::default(),
            playlist: Vec::new(),
            room_list: Vec::new(),
            user: UserChange {
                name: "ThisUser".to_string(),
                ready: false,
            },
        }
    }

    pub async fn run(mut self) {
        let mut i = 0;
        loop {
            self.change_inner(i);
            self.send_changes();
            self.receive_event().await;
            i += 1;
        }
    }

    async fn receive_event(&mut self) {
        let event = self.ui.event().await;
        info!("Received Event from UI: {event:?}");
        match event {
            UserInterfaceEvent::PlaylistChange(_p) => {
                // TODO
                // self.playlist = p.playlist;
            }
            UserInterfaceEvent::UserChange(u) => {
                self.user.name = u.name;
                self.user.ready = u.ready;
            }
            _ => {}
        }
    }

    fn send_changes(&mut self) {
        self.ui.file_database_status(self.file_database_status);
        self.ui.file_database(self.file_database.clone());
        // TODO
        // self.ui.playlist(self.playlist.clone());
        // self.ui.room_list(self.room_list.clone());
        self.ui.user_update(self.user.clone());
        self.send_player_message();
    }

    fn send_player_message(&mut self) {
        let msg = PlayerMessageInner {
            message: "Message".to_string(),
            source: MessageSource::UserMessage("ExternalUser".to_string()),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into();
        self.ui.player_message(msg);
    }

    fn change_inner(&mut self, i: usize) {
        self.change_file_data_base_status();
        self.change_file_database(i);
        self.change_playlist(i);
        if i % 10 == 0 {
            self.change_room_list(i)
        }
    }

    fn change_file_data_base_status(&mut self) {
        self.file_database_status += 0.35;
        self.file_database_status %= 1.0;
    }

    fn change_file_database(&mut self, i: usize) {
        let iter = self
            .file_database
            .iter()
            .cloned()
            .chain(std::iter::once(FileEntry::new(
                format!("file{i}"),
                PathBuf::from(format!("/folder/file{i}")),
                None,
            )));
        self.file_database = FileStore::from_iter(iter);
    }

    fn change_playlist(&mut self, i: usize) {
        self.playlist.push(format!("file{i}"));
    }

    fn change_room_list(&mut self, i: usize) {
        let ready = i % 5 == 0;
        self.room_list.push(UserStatus {
            // TODO
            // room: format!("Room{}", i % 10),
            name: format!("User{i}"),
            ready,
        })
    }
}

fn main() {}
