use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tracing::debug;

pub(crate) use crate::UserStatusListMsg;
use crate::communicator::{PlaylistMsg, SelectMsg};
pub(crate) use crate::user::UserStatus;

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

    pub fn iter(&'_ self) -> std::collections::btree_set::Iter<'_, UserStatus> {
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

/// The authoritative room state the host keeps and replays to peers that
/// join later: the playlist, the current selection (whose position follows
/// the host's own heartbeats), and who is in the room.
#[derive(Debug, Clone)]
pub struct HostRoom {
    pub users: RoomUsers,
    pub playlist: PlaylistMsg,
    pub select: SelectMsg,
}

impl HostRoom {
    pub fn new(room: RoomName, playlist: PlaylistMsg, select: SelectMsg) -> Self {
        Self {
            users: RoomUsers::new(room),
            playlist,
            select,
        }
    }

    /// The name a peer goes by in the room, for messages the host issues on
    /// the peer's behalf.
    pub fn actor_of(&self, peer: &PeerId) -> ArcStr {
        match self.users.peer_status(peer) {
            Some(Some(status)) => status.name.clone(),
            Some(None) => arcstr::literal!("unknown"),
            None => arcstr::literal!("server"),
        }
    }
}

/// Who is in the room, as the host sees it. Remote peers are keyed by their
/// peer id; the host itself has its own slot, since it never talks to
/// itself over the network.
#[derive(Debug, Clone)]
pub struct RoomUsers {
    status_list: UserStatusListMsg,
    local: Option<UserStatus>,
    peers: HashMap<PeerId, Option<UserStatus>>,
}

impl RoomUsers {
    pub fn new(room: RoomName) -> Self {
        Self {
            status_list: UserStatusListMsg {
                room_name: room,
                users: BTreeSet::default(),
            },
            local: None,
            peers: HashMap::default(),
        }
    }

    pub fn status_list(&self) -> &UserStatusListMsg {
        &self.status_list
    }

    pub fn contains(&self, peer: &PeerId) -> bool {
        self.peers.contains_key(peer)
    }

    pub fn add_connected(&mut self, peer: PeerId) {
        self.peers.insert(peer, None);
    }

    /// `None`: peer unknown; `Some(None)`: connected but no status yet;
    /// `Some(Some(_))`: established user.
    pub fn peer_status(&self, peer: &PeerId) -> Option<&Option<UserStatus>> {
        self.peers.get(peer)
    }

    // user that already sent status message and is known
    fn is_established_user(&self, peer: &PeerId) -> bool {
        self.peers.get(peer).is_some_and(|s| s.is_some())
    }

    // user that established connection but did not send status yet
    pub fn is_connected_user(&self, peer: &PeerId) -> bool {
        self.peers.get(peer).is_some_and(|s| s.is_none())
    }

    pub fn all_ready(&self) -> bool {
        self.status_list.users.iter().all(|u| u.ready)
    }

    /// Finds the remote peer currently using `name`. The host's own slot is
    /// never returned: it cannot be the target of anything sent to a peer.
    pub fn find_by_name(&self, name: &str) -> Option<PeerId> {
        self.peers
            .iter()
            .find(|(_, s)| s.as_ref().is_some_and(|u| u.name == name))
            .map(|(peer, _)| *peer)
    }

    /// Removes the peer from the user map and the status list.
    /// Returns false if the peer was not known at all.
    pub fn remove(&mut self, peer: &PeerId) -> bool {
        let Some(status) = self.peers.remove(peer) else {
            return false;
        };
        if let Some(s) = status {
            self.status_list.users.remove(&s);
        }
        true
    }

    fn username_exists(&self, name: &str) -> bool {
        self.status_list.users.iter().any(|u| u.name == name)
    }

    fn roll_new_username(&self, username: &str) -> ArcStr {
        let mut suffix = 2u32;
        let mut candidate = arcstr::format!("{username}_{suffix}");
        while self.username_exists(&candidate) {
            suffix += 1;
            candidate = arcstr::format!("{username}_{suffix}");
        }
        candidate
    }

    /// The host's own status. It is never force-renamed: the host decides
    /// names, so its own wins.
    pub fn update_local(&mut self, status: UserStatus) {
        if let Some(old) = self.local.take()
            && old.name != status.name
        {
            self.status_list.users.remove(&old);
        }
        self.status_list.users.replace(status.clone());
        self.local = Some(status);
    }

    fn update_peer(&mut self, status: UserStatus, peer: PeerId) {
        self.status_list.users.replace(status.clone());
        self.peers.insert(peer, Some(status));
    }

    /// Applies a peer's status update. If the requested name collides with
    /// another user's name, a fresh one is rolled; the returned status is
    /// the forced rename that must be sent back to the peer.
    pub fn apply_status(&mut self, status: UserStatus, peer: PeerId) -> Option<UserStatus> {
        let mut forced = None;
        let mut new_status = status.clone();

        if self.is_established_user(&peer) {
            let old_status = self.peers.get(&peer).cloned().flatten();
            let renamed = old_status.as_ref().is_some_and(|s| s.name != status.name);
            if renamed {
                if self.username_exists(&status.name) {
                    new_status = UserStatus {
                        name: self.roll_new_username(&status.name),
                        ready: status.ready,
                    };
                    forced = Some(new_status.clone());
                }
                if let Some(s) = &old_status {
                    self.status_list.users.remove(s);
                }
            }
            // same name: typical status update, only need to update map & list
        } else if self.is_connected_user(&peer) && self.username_exists(&status.name) {
            // new user whose name is already taken
            new_status = UserStatus {
                name: self.roll_new_username(&status.name),
                ready: status.ready,
            };
            forced = Some(new_status.clone());
        }

        self.update_peer(new_status, peer);
        forced
    }
}

/// When the playlist shrank and the currently selected video was removed,
/// picks the video that took its place by walking the common prefix of the
/// old and new playlist.
pub fn select_next(
    current_playlist: &PlaylistMsg,
    current_select: &SelectMsg,
    new_playlist: &PlaylistMsg,
) -> Option<SelectMsg> {
    if new_playlist.playlist.len() >= current_playlist.playlist.len() {
        return None;
    }

    let mut new_position = 0;
    let max_len = new_playlist.playlist.len();
    if let Some(current_video) = current_select.video.clone() {
        for old_video in current_playlist.playlist.iter() {
            if let Some(new_video) = new_playlist.playlist.get(new_position) {
                debug!(?current_video, ?new_video, "Current video and old");
                if *new_video == current_video {
                    // No need to select if current video is still in playlist
                    return None;
                }

                if *old_video == current_video {
                    break;
                }

                if *new_video == *old_video {
                    new_position += 1;
                }

                if new_position >= max_len {
                    new_position -= 1;
                    break;
                }
            }
        }
    }

    let new_select = new_playlist.playlist.get(new_position)?;
    Some(SelectMsg {
        actor: arcstr::literal!("host"),
        position: Duration::ZERO,
        video: Some(new_select.clone()),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::playlist::Video;

    #[test]
    fn test_len() {
        let mut user_list = UserList::default();

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

    fn peer(n: u8) -> PeerId {
        // an identity multihash: a valid peer id without key material
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    fn status(name: &str, ready: bool) -> UserStatus {
        UserStatus {
            name: name.into(),
            ready,
        }
    }

    fn names(room: &RoomUsers) -> Vec<&str> {
        room.status_list()
            .users
            .iter()
            .map(|u| u.name.as_str())
            .collect()
    }

    #[test]
    fn new_user_keeps_free_name() {
        let mut room = RoomUsers::new("room".into());
        let alice = peer(1);
        room.add_connected(alice);

        let forced = room.apply_status(status("alice", false), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
    }

    #[test]
    fn new_user_with_taken_name_is_renamed() {
        let mut room = RoomUsers::new("room".into());
        let (alice, intruder) = (peer(1), peer(2));
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder);

        let forced = forced.expect("collision must force a rename");
        assert_ne!(forced.name, "alice");
        assert!(forced.name.starts_with("alice_"));
        assert_eq!(room.status_list().users.len(), 2);
        // the original user is untouched
        assert_eq!(room.find_by_name("alice"), Some(alice));
        assert_eq!(room.find_by_name(forced.name.as_str()), Some(intruder));
    }

    #[test]
    fn new_user_taking_the_hosts_name_is_renamed() {
        let mut room = RoomUsers::new("room".into());
        room.update_local(status("alice", true));
        let intruder = peer(2);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder);

        let forced = forced.expect("the host's name is taken too");
        assert!(forced.name.starts_with("alice_"));
        assert_eq!(room.status_list().users.len(), 2);
        assert_eq!(room.find_by_name("alice"), None, "the host is no peer");
    }

    #[test]
    fn rolled_names_avoid_each_other() {
        let mut room = RoomUsers::new("room".into());
        room.update_local(status("alice", true));
        let mut rolled = vec![];
        for n in 2..6 {
            let p = peer(n);
            room.add_connected(p);
            rolled.push(room.apply_status(status("alice", false), p).unwrap().name);
        }

        rolled.sort();
        rolled.dedup();
        assert_eq!(rolled.len(), 4, "every collision must get its own name");
        assert_eq!(room.status_list().users.len(), 5);
    }

    #[test]
    fn ready_toggle_updates_in_place() {
        let mut room = RoomUsers::new("room".into());
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", false), alice);
        assert!(!room.all_ready());

        let forced = room.apply_status(status("alice", true), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
        assert!(room.all_ready());
    }

    #[test]
    fn all_ready_includes_the_host() {
        let mut room = RoomUsers::new("room".into());
        let bob = peer(2);
        room.add_connected(bob);
        room.apply_status(status("bob", true), bob);
        room.update_local(status("alice", false));
        assert!(!room.all_ready());

        room.update_local(status("alice", true));
        assert!(room.all_ready());
    }

    #[test]
    fn host_rename_replaces_its_old_entry() {
        let mut room = RoomUsers::new("room".into());
        room.update_local(status("alice", true));
        room.update_local(status("alicia", true));

        assert_eq!(names(&room), vec!["alicia"]);
    }

    #[test]
    fn rename_to_free_name_replaces_old_entry() {
        let mut room = RoomUsers::new("room".into());
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);

        let forced = room.apply_status(status("alicia", true), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alicia"]);
        assert_eq!(room.find_by_name("alicia"), Some(alice));
    }

    #[test]
    fn rename_into_collision_is_rerolled_and_leaves_other_user_alone() {
        let mut room = RoomUsers::new("room".into());
        let (alice, bob) = (peer(1), peer(2));
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);
        room.add_connected(bob);
        room.apply_status(status("bob", true), bob);

        // bob renames himself to "alice"
        let forced = room.apply_status(status("alice", true), bob);

        let forced = forced.expect("collision must force a rename");
        assert!(forced.name.starts_with("alice_"));
        // alice keeps her entry, bob's old name is gone, no ghost entries
        assert_eq!(room.status_list().users.len(), 2);
        assert_eq!(room.find_by_name("alice"), Some(alice));
        assert_eq!(room.find_by_name("bob"), None);
        assert_eq!(room.find_by_name(forced.name.as_str()), Some(bob));
    }

    #[test]
    fn remove_drops_user_and_status() {
        let mut room = RoomUsers::new("room".into());
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", false), alice);

        assert!(room.remove(&alice));
        assert!(names(&room).is_empty());
        assert!(!room.contains(&alice));
        assert!(!room.remove(&alice));
    }

    #[test]
    fn connection_states_are_tracked() {
        let mut room = RoomUsers::new("room".into());
        let alice = peer(1);
        assert!(room.peer_status(&alice).is_none());

        room.add_connected(alice);
        assert!(room.is_connected_user(&alice));

        room.apply_status(status("alice", false), alice);
        assert!(!room.is_connected_user(&alice));
        assert_eq!(
            room.peer_status(&alice),
            Some(&Some(status("alice", false)))
        );
    }

    fn playlist_msg(videos: &[&str]) -> PlaylistMsg {
        PlaylistMsg {
            actor: arcstr::literal!("test"),
            playlist: videos.iter().copied().collect(),
        }
    }

    fn select_msg(video: Option<&str>) -> SelectMsg {
        SelectMsg {
            actor: arcstr::literal!("test"),
            position: Duration::ZERO,
            video: video.map(Video::from),
        }
    }

    fn selected(result: Option<SelectMsg>) -> Option<String> {
        result.and_then(|s| s.video).map(|v| v.as_str().to_string())
    }

    #[test]
    fn select_next_ignores_grown_or_equal_playlist() {
        let old = playlist_msg(&["a", "b"]);
        let select = select_msg(Some("a"));

        assert!(select_next(&old, &select, &playlist_msg(&["a", "b"])).is_none());
        assert!(select_next(&old, &select, &playlist_msg(&["a", "b", "c"])).is_none());
    }

    #[test]
    fn select_next_keeps_current_video_if_still_present() {
        let old = playlist_msg(&["a", "b", "c"]);
        let select = select_msg(Some("b"));

        assert!(select_next(&old, &select, &playlist_msg(&["a", "b"])).is_none());
    }

    #[test]
    fn select_next_picks_successor_when_current_removed_in_middle() {
        let old = playlist_msg(&["a", "b", "c"]);
        let select = select_msg(Some("b"));

        let result = select_next(&old, &select, &playlist_msg(&["a", "c"]));

        assert_eq!(selected(result), Some("c".into()));
    }

    #[test]
    fn select_next_picks_new_first_when_current_removed_at_start() {
        let old = playlist_msg(&["a", "b", "c"]);
        let select = select_msg(Some("a"));

        let result = select_next(&old, &select, &playlist_msg(&["b", "c"]));

        assert_eq!(selected(result), Some("b".into()));
    }

    #[test]
    fn select_next_picks_last_when_current_removed_at_end() {
        let old = playlist_msg(&["a", "b", "c"]);
        let select = select_msg(Some("c"));

        let result = select_next(&old, &select, &playlist_msg(&["a", "b"]));

        assert_eq!(selected(result), Some("b".into()));
    }

    #[test]
    fn select_next_returns_none_for_emptied_playlist() {
        let old = playlist_msg(&["a"]);
        let select = select_msg(Some("a"));

        assert!(select_next(&old, &select, &playlist_msg(&[])).is_none());
    }

    // Documents current behavior: with nothing selected, any shrink of the
    // playlist selects the new first video. Whether that is intended is an
    // open question; see select_next.
    #[test]
    fn select_next_without_selection_picks_first_video() {
        let old = playlist_msg(&["a", "b", "c"]);
        let select = select_msg(None);

        let result = select_next(&old, &select, &playlist_msg(&["a", "b"]));

        assert_eq!(selected(result), Some("a".into()));
    }
}
