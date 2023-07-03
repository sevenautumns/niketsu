use std::collections::{BTreeMap, BTreeSet};

use super::communicator::{NiketsuUserStatus, OutgoingMessage};

#[derive(Debug, Clone)]
pub struct UserStatus {
    pub room: String,
    pub name: String,
    pub ready: bool,
}

pub struct RoomList {
    rooms: BTreeMap<String, BTreeSet<NiketsuUserStatus>>,
}

impl From<UserStatus> for OutgoingMessage {
    fn from(value: UserStatus) -> Self {
        NiketsuUserStatus {
            ready: value.ready,
            username: value.name,
        }
        .into()
    }
}
