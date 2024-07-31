use arcstr::ArcStr;
use serde::{Deserialize, Serialize};

use super::ui::UserChange;

#[derive(Debug, Clone, Deserialize, Serialize, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub name: ArcStr,
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
