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
    Pause {
        filename: String,
        username: String,
    },
    Start {
        filename: String,
        username: String,
    },
    Seek {
        filename: String,
        #[serde(with = "serde_millis")]
        position: Duration,
        username: String,
    },
    Select {
        filename: String,
        username: String,
    },
    Message {
        message: String,
        username: String,
    },
    Playlist {
        playlist: Vec<String>,
        username: String,
    },
    Status {
        ready: bool,
        username: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub username: String,
    pub ready: bool,
}
