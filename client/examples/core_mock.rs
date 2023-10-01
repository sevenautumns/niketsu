#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use chrono::Local;
use log::info;
use niketsu_core::file_database::{FileEntry, FileStore};
use niketsu_core::playlist::{Playlist, PlaylistVideo};
use niketsu_core::rooms::RoomList;
use niketsu_core::ui::{
    MessageLevel, MessageSource, PlayerMessageInner, UserChange, UserInterfaceEvent,
    UserInterfaceTrait,
};
use niketsu_core::user::UserStatus;

pub struct CoreMock<T: UserInterfaceTrait> {
    ui: T,
    file_database_status: f32,
    file_database: FileStore,
    playlist: Playlist,
    room_list: RoomList,
    room_list_map: BTreeMap<String, BTreeSet<UserStatus>>,
    user_change: UserChange,
}

impl<T: UserInterfaceTrait> CoreMock<T> {
    pub fn new(ui: T) -> Self {
        let mut mock = Self {
            ui,
            file_database_status: 0.0,
            file_database: FileStore::default(),
            playlist: Default::default(),
            room_list: Default::default(),
            room_list_map: Default::default(),
            user_change: UserChange {
                name: "ThisUser".to_string(),
                ready: false,
            },
        };
        mock.room_list_map.insert(
            "Test".into(),
            BTreeSet::from([UserStatus {
                name: "ThisUser".into(),
                ready: false,
            }]),
        );
        mock
    }

    pub async fn run(mut self) {
        let mut i = 0;
        loop {
            tokio::select! {
                event = self.ui.event() => self.receive_event(event),
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    self.change_inner(i);
                    self.send_changes();
                }
            }
            i += 1;
        }
    }

    fn receive_event(&mut self, event: UserInterfaceEvent) {
        info!("Received Event from UI: {event:?}");
        match event {
            UserInterfaceEvent::PlaylistChange(p) => {
                self.playlist = p.playlist;
            }
            UserInterfaceEvent::UserChange(u) => {
                self.user_change.name = u.name;
                self.user_change.ready = u.ready;
            }
            UserInterfaceEvent::UserMessage(m) => self.ui.player_message(
                PlayerMessageInner {
                    message: m.message,
                    source: MessageSource::UserMessage("SomeUser".to_string()),
                    level: MessageLevel::Normal,
                    timestamp: Local::now(),
                }
                .into(),
            ),
            _ => {}
        }
    }

    fn send_changes(&mut self) {
        self.ui.file_database_status(self.file_database_status);
        self.ui.file_database(self.file_database.clone());
        self.ui.playlist(self.playlist.clone());
        self.ui.room_list(self.room_list.clone());
        self.ui.user_update(self.user_change.clone());
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
        if i % 10 == 0 || i % 15 == 0 {
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
        self.playlist
            .push(PlaylistVideo::from(format!("file{i}").as_str()));
    }

    fn change_room_list(&mut self, i: usize) {
        let ready = i % 2 == 0;
        let room_name = match ready {
            true => "Test".to_string(),
            false => "NoTest".to_string(),
        };
        let user = UserStatus {
            name: format!("User{i}"),
            ready,
        };
        if let Some(bset) = self.room_list_map.get(&room_name) {
            let mut new_room_list = bset.clone();
            new_room_list.insert(user);
            self.room_list_map.insert(room_name, new_room_list);
        } else {
            self.room_list_map.insert(room_name, BTreeSet::from([user]));
        }
        self.room_list = RoomList::from(self.room_list_map.clone());
    }
}

fn main() {}
