use arcstr::ArcStr;
use multiaddr::PeerId;
use serde::{Deserialize, Serialize};

use super::chunk::SharedFile;
use super::playlist::Video;
use super::ui::UserChange;

#[derive(Debug, Clone, Deserialize, Serialize, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub name: ArcStr,
    pub ready: bool,
    /// The file this user offers to the others. The host keeps an offer
    /// only of the room's selected video and drops every offer when the
    /// selection changes, so in a room state it is the selected video or
    /// nothing. The name is kept because a status can reach the host after
    /// the selection it was sent for has changed.
    pub offers: Option<SharedFile>,
    /// The video the user's player has loaded. It counts while it names the
    /// room's selected video.
    pub loaded: Option<Video>,
    /// Where the host reaches the user. The host sets it from the
    /// connection; what a user sends is ignored.
    pub peer: Option<PeerId>,
}

impl UserStatus {
    /// Whether everything about the two users is the same. Equality only
    /// compares the names, which is what the user list is ordered by.
    pub fn same_as(&self, other: &Self) -> bool {
        self.name == other.name
            && self.ready == other.ready
            && self.offers == other.offers
            && self.loaded == other.loaded
            && self.peer == other.peer
    }

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
