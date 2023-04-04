use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum ServerMessage {
    Ping {},
    VideoStatus {
        filename: String,
        // #[serde(with = "serde_millis")]
        position: Duration,
    },
}
