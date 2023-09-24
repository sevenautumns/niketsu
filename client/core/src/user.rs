use super::communicator::{NiketsuUserStatus, OutgoingMessage};
use super::ui::UserChange;

/// TODO check if default results in problems
#[derive(Default, Debug, Clone, Eq)]
pub struct UserStatus {
    pub name: String,
    pub ready: bool,
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

impl From<UserStatus> for OutgoingMessage {
    fn from(value: UserStatus) -> Self {
        NiketsuUserStatus {
            ready: value.ready,
            username: value.name,
        }
        .into()
    }
}

impl From<UserChange> for UserStatus {
    fn from(value: UserChange) -> Self {
        Self {
            name: value.name,
            ready: value.ready,
        }
    }
}

impl PartialEq for UserStatus {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl PartialEq<String> for UserStatus {
    fn eq(&self, name: &String) -> bool {
        self.name.eq(name)
    }
}

impl Ord for UserStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for UserStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.name.partial_cmp(&other.name)
    }
}
