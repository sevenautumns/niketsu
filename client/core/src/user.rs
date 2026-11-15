use arcstr::ArcStr;
use serde::{Deserialize, Serialize};

use super::chunk::SharedFile;
use super::playlist::Video;
use super::ui::UserChange;

/// A user of the room, as the room's authority keeps it and publishes it.
/// What a user says about a video names that video, and counts while the
/// room has it selected: nobody clears it when the selection changes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// The name the room knows the user by. The authority gives a user
    /// whose name another user goes by a name of its own.
    pub name: ArcStr,
    pub ready: bool,
    /// The video the user's player has loaded.
    pub loaded: Option<Video>,
    /// The file the user offers to the others.
    pub offers: Option<SharedFile>,
}

impl User {
    /// Whether the user's player has `video` loaded.
    pub fn has_loaded(&self, video: &Video) -> bool {
        self.loaded.as_ref() == Some(video)
    }

    /// What the user offers of `video`.
    pub fn offer_of(&self, video: &Video) -> Option<&SharedFile> {
        self.offers
            .as_ref()
            .filter(|file| file.name == video.as_str())
    }
}

/// A user as the UI shows it: by name, ready or not, and what it offers of
/// the selected video. Users are told apart and ordered by name, which is
/// what the user list is sorted by.
#[derive(Debug, Clone, Eq, Default)]
pub struct UserStatus {
    pub name: ArcStr,
    pub ready: bool,
    pub offers: Option<SharedFile>,
}

impl UserStatus {
    pub fn ready(&mut self) {
        self.ready = true;
    }

    pub fn not_ready(&mut self) {
        self.ready = false;
    }

    pub fn toggle_ready(&mut self) {
        self.ready = !self.ready;
    }
}

impl From<UserChange> for UserStatus {
    fn from(value: UserChange) -> Self {
        Self {
            name: value.name,
            ready: value.ready,
            ..Default::default()
        }
    }
}

impl PartialEq for UserStatus {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl<T: AsRef<str>> PartialEq<T> for UserStatus {
    fn eq(&self, name: &T) -> bool {
        self.name.eq(name.as_ref())
    }
}

impl Ord for UserStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for UserStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
