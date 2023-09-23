use std::collections::{BTreeMap, BTreeSet};

use crate::core::user::UserStatus;

pub type RoomName = String;

#[derive(Debug, Clone, Default)]
pub struct RoomList {
    list: BTreeMap<RoomName, BTreeSet<UserStatus>>,
}

impl RoomList {
    pub fn contains_room(&self, room: &str) -> bool {
        self.list.contains_key(room)
    }

    pub fn iter(&self) -> Rooms<'_> {
        self.into_iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn get_room_name(&self, index: usize) -> Option<&RoomName> {
        self.list.keys().nth(index)
    }
}

impl<'a> IntoIterator for &'a RoomList {
    type Item = (&'a RoomName, User<'a>);

    type IntoIter = Rooms<'a>;

    fn into_iter(self) -> Self::IntoIter {
        Rooms {
            room_iter: self.list.iter(),
        }
    }
}

impl From<BTreeMap<RoomName, BTreeSet<UserStatus>>> for RoomList {
    fn from(list: BTreeMap<RoomName, BTreeSet<UserStatus>>) -> Self {
        Self { list }
    }
}

#[derive(Debug, Clone)]
pub struct Rooms<'a> {
    room_iter: std::collections::btree_map::Iter<'a, RoomName, BTreeSet<UserStatus>>,
}

impl<'a> Iterator for Rooms<'a> {
    type Item = (&'a RoomName, User<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (room, user) = self.room_iter.next()?;
        let user_iter = user.iter();
        Some((room, User { user_iter }))
    }
}

#[derive(Debug, Clone)]
pub struct User<'a> {
    user_iter: std::collections::btree_set::Iter<'a, UserStatus>,
}

impl<'a> Iterator for User<'a> {
    type Item = &'a UserStatus;

    fn next(&mut self) -> Option<Self::Item> {
        self.user_iter.next()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_room_present() {
        let mut room_list = RoomList::default();
        let room_name = "Room 1".to_string();

        let mut user_set = BTreeSet::new();
        user_set.insert(UserStatus {
            name: "User 1".to_string(),
            ready: true,
        });

        room_list.list.insert(room_name.clone(), user_set.clone());

        assert!(room_list.contains_room(&room_name));
    }

    #[test]
    fn test_contains_room_not_present() {
        let room_list = RoomList::default();
        let room_name = "Room 2";

        assert!(!room_list.contains_room(room_name));
    }

    #[test]
    fn test_empty_roomlist_iteration() {
        let room_list = RoomList::default();
        let mut room_iter = room_list.iter();

        // Ensure there are no rooms to iterate in an empty RoomList.
        assert!(room_iter.next().is_none());
    }

    #[test]
    fn test_single_room_single_user_iteration() {
        let mut room_list = RoomList::default();
        let room_name = "Room 1".to_string();
        let user_name = "User 1".to_string();
        let user_status = UserStatus {
            name: user_name.clone(),
            ready: true,
        };

        let mut user_set = BTreeSet::new();
        user_set.insert(user_status.clone());

        room_list.list.insert(room_name.clone(), user_set.clone());

        let mut room_iter = room_list.iter();

        // Check the room and its user.
        let (room, user) = room_iter.next().unwrap();
        assert_eq!(room, &room_name);
        let users_in_room: Vec<&UserStatus> = user.collect();
        assert_eq!(users_in_room.len(), 1);
        assert_eq!(users_in_room[0].name, user_name);
        assert!(users_in_room[0].ready);

        // Ensure there are no more rooms to iterate.
        assert!(room_iter.next().is_none());
    }

    #[test]
    fn test_multiple_rooms_users_iteration() {
        let mut room_list = RoomList::default();
        let room1_name = "Room 1".to_string();
        let room2_name = "Room 2".to_string();

        let mut user_set1 = BTreeSet::new();
        user_set1.insert(UserStatus {
            name: "User 1".to_string(),
            ready: true,
        });

        let mut user_set2 = BTreeSet::new();
        user_set2.insert(UserStatus {
            name: "User 2".to_string(),
            ready: false,
        });

        room_list.list.insert(room1_name.clone(), user_set1.clone());
        room_list.list.insert(room2_name.clone(), user_set2.clone());

        let mut room_iter = room_list.iter();

        // Check the first room and its users.
        let (room1, user1) = room_iter.next().unwrap();
        assert_eq!(room1, &room1_name);
        let users_in_room1: Vec<&UserStatus> = user1.collect();
        assert_eq!(users_in_room1.len(), 1);
        assert_eq!(users_in_room1[0].name, "User 1");
        assert!(users_in_room1[0].ready);

        // Check the second room and its users.
        let (room2, user2) = room_iter.next().unwrap();
        assert_eq!(room2, &room2_name);
        let users_in_room2: Vec<&UserStatus> = user2.collect();
        assert_eq!(users_in_room2.len(), 1);
        assert_eq!(users_in_room2[0].name, "User 2");
        assert!(!users_in_room2[0].ready);

        // Ensure there are no more rooms to iterate.
        assert!(room_iter.next().is_none());
    }

    #[test]
    fn test_from_btreemap() {
        let mut btreemap = BTreeMap::new();
        let room_name = "Room 1".to_string();

        let mut user_set = BTreeSet::new();
        user_set.insert(UserStatus {
            name: "User 1".to_string(),
            ready: true,
        });

        btreemap.insert(room_name.clone(), user_set.clone());

        let room_list: RoomList = btreemap.into();

        // Ensure the RoomList contains the converted data.
        assert!(room_list.contains_room(&room_name));
    }
}
