use std::collections::BTreeSet;

use arcstr::ArcStr;

pub(crate) use crate::user::UserStatus;
pub(crate) use crate::UserStatusListMsg;

pub type RoomName = ArcStr;

#[derive(Debug, Clone, Default)]
pub struct UserList {
    room: RoomName,
    list: BTreeSet<UserStatus>,
}

impl UserList {
    pub fn get(&self, index: usize) -> Option<UserStatus> {
        self.list.iter().nth(index).cloned()
    }

    pub fn contains_user(&self, user: &String) -> bool {
        self.list.iter().any(|status| status.name == *user)
    }

    pub fn iter(&self) -> std::collections::btree_set::Iter<UserStatus> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get_room_name(&self) -> &RoomName {
        &self.room
    }

    pub fn insert(&mut self, user: UserStatus) -> bool {
        self.list.insert(user)
    }

    pub fn replace(&mut self, user: UserStatus) -> Option<UserStatus> {
        self.list.replace(user)
    }
}

impl From<UserStatusListMsg> for UserList {
    fn from(value: UserStatusListMsg) -> Self {
        Self {
            room: value.room_name,
            list: value.users,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_len() {
        let mut user_list = UserList::default();

        // Add some rooms for testi1ng
        user_list.list.insert(UserStatus {
            name: "User1".into(),
            ready: false,
        });
        user_list.list.insert(UserStatus {
            name: "User2".into(),
            ready: false,
        });

        assert_eq!(user_list.len(), 2);
    }

    #[test]
    fn test_is_empty() {
        let mut room_list = UserList::default();

        // Initially, the room list should be empty
        assert!(room_list.is_empty());

        // Add a room to make it non-empty
        room_list.list.insert(UserStatus {
            name: "User".into(),
            ready: false,
        });

        // Now, it should not be empty
        assert!(!room_list.is_empty());
    }

    #[test]
    fn test_from_niketsu_user_status() {
        let mut btreeset = BTreeSet::new();
        let user_name = "User1".to_string();

        btreeset.insert(UserStatus {
            name: arcstr::literal!("User1"),
            ready: true,
        });

        let user_list: UserList = UserStatusListMsg {
            room_name: arcstr::literal!("room"),
            users: btreeset,
        }
        .into();

        // Ensure the RoomList contains the converted data.
        assert!(user_list.contains_user(&user_name));
    }
}
