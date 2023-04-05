use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum ServerMessage {
    Ping(#[serde(rename = "uuid")] String),
    VideoStatus {
        filename: String,
        #[serde(with = "serde_millis")]
        position: Duration,
    },
    StatusList(#[serde(rename = "users")] Vec<UserStatus>),
    Pause(#[serde(rename = "filename")] String),
    Start(#[serde(rename = "filename")] String),
    Seek {
        filename: String,
        #[serde(with = "serde_millis")]
        position: Duration,
    },
    Select(#[serde(rename = "filename")] String),
    Message {
        username: String,
        message: String,
    },
    Playlist(#[serde(rename = "playlist")] Vec<String>),
    Status {
        username: String,
        ready: bool,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub username: String,
    pub ready: bool,
}
