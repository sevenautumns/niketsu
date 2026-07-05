//! Host-side room state: pure bookkeeping that is unit-testable without a
//! swarm. Side effects (sending forced renames, broadcasting the status
//! list) stay in the host handler; this module only decides them.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use arcstr::ArcStr;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use libp2p::PeerId;
use niketsu_core::communicator::{PlaylistMsg, SelectMsg, UserStatusListMsg};
use niketsu_core::room::RoomName;
use niketsu_core::user::UserStatus;
use tracing::debug;

pub(crate) struct RoomUsers {
    status_list: UserStatusListMsg,
    users: HashMap<PeerId, Option<UserStatus>>,
}

impl RoomUsers {
    pub(crate) fn new(room: RoomName) -> Self {
        Self {
            status_list: UserStatusListMsg {
                room_name: room,
                users: BTreeSet::default(),
            },
            users: HashMap::default(),
        }
    }

    pub(crate) fn status_list(&self) -> &UserStatusListMsg {
        &self.status_list
    }

    pub(crate) fn contains(&self, peer_id: &PeerId) -> bool {
        self.users.contains_key(peer_id)
    }

    pub(crate) fn add_connected(&mut self, peer_id: PeerId) {
        self.users.insert(peer_id, None);
    }

    /// `None`: peer unknown; `Some(None)`: connected but no status yet;
    /// `Some(Some(_))`: established user.
    pub(crate) fn peer_status(&self, peer_id: &PeerId) -> Option<&Option<UserStatus>> {
        self.users.get(peer_id)
    }

    // user that already sent status message and is known
    fn is_established_user(&self, peer_id: PeerId) -> bool {
        self.users.get(&peer_id).is_some_and(|s| s.is_some())
    }

    // user that established connection but did not send status yet
    pub(crate) fn is_connected_user(&self, peer_id: PeerId) -> bool {
        self.users.get(&peer_id).is_some_and(|s| s.is_none())
    }

    pub(crate) fn all_ready(&self) -> bool {
        self.status_list.users.iter().all(|u| u.ready)
    }

    pub(crate) fn find_by_name(&self, name: &str) -> Option<PeerId> {
        self.users
            .iter()
            .find(|(_, s)| s.as_ref().is_some_and(|u| u.name == name))
            .map(|(peer_id, _)| *peer_id)
    }

    /// Removes the peer from the user map and the status list.
    /// Returns false if the peer was not known at all.
    pub(crate) fn remove(&mut self, peer_id: &PeerId) -> bool {
        let Some(status) = self.users.remove(peer_id) else {
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
        let mut buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        while self.username_exists(&buzzword) {
            buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        }
        buzzword
    }

    /// Applies a status update without collision handling (used for the
    /// host's own status, which is never force-renamed).
    pub(crate) fn update_status(&mut self, status: UserStatus, peer_id: PeerId) {
        self.status_list.users.replace(status.clone());
        self.users.insert(peer_id, Some(status));
    }

    /// Applies a peer's status update. If the requested name collides with
    /// another user's name, a fresh one is rolled; the returned status is
    /// the forced rename that must be sent back to the peer.
    pub(crate) fn apply_status(
        &mut self,
        status: UserStatus,
        peer_id: PeerId,
    ) -> Option<UserStatus> {
        let mut forced = None;
        let mut new_status = status.clone();

        if self.is_established_user(peer_id) {
            let old_status = self
                .users
                .get(&peer_id)
                .cloned()
                .expect("user should exist");
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
        } else if self.is_connected_user(peer_id) && self.username_exists(&status.name) {
            // new user whose name is already taken
            new_status = UserStatus {
                name: self.roll_new_username(&status.name),
                ready: status.ready,
            };
            forced = Some(new_status.clone());
        }

        self.update_status(new_status, peer_id);
        forced
    }
}

/// When the playlist shrank and the currently selected video was removed,
/// picks the video that took its place by walking the common prefix of the
/// old and new playlist.
pub(crate) fn select_next(
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
mod tests {
    use niketsu_core::playlist::Video;

    use super::*;

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
        let peer = PeerId::random();
        room.add_connected(peer);

        let forced = room.apply_status(status("alice", false), peer);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
    }

    #[test]
    fn new_user_with_taken_name_is_renamed() {
        let mut room = RoomUsers::new("room".into());
        let (alice, intruder) = (PeerId::random(), PeerId::random());
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
    fn ready_toggle_updates_in_place() {
        let mut room = RoomUsers::new("room".into());
        let peer = PeerId::random();
        room.add_connected(peer);
        room.apply_status(status("alice", false), peer);
        assert!(!room.all_ready());

        let forced = room.apply_status(status("alice", true), peer);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
        assert!(room.all_ready());
    }

    #[test]
    fn rename_to_free_name_replaces_old_entry() {
        let mut room = RoomUsers::new("room".into());
        let peer = PeerId::random();
        room.add_connected(peer);
        room.apply_status(status("alice", true), peer);

        let forced = room.apply_status(status("alicia", true), peer);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alicia"]);
        assert_eq!(room.find_by_name("alicia"), Some(peer));
    }

    #[test]
    fn rename_into_collision_is_rerolled_and_leaves_other_user_alone() {
        let mut room = RoomUsers::new("room".into());
        let (alice, bob) = (PeerId::random(), PeerId::random());
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
        let peer = PeerId::random();
        room.add_connected(peer);
        room.apply_status(status("alice", false), peer);

        assert!(room.remove(&peer));
        assert!(names(&room).is_empty());
        assert!(!room.contains(&peer));
        assert!(!room.remove(&peer));
    }

    #[test]
    fn connection_states_are_tracked() {
        let mut room = RoomUsers::new("room".into());
        let peer = PeerId::random();
        assert!(room.peer_status(&peer).is_none());

        room.add_connected(peer);
        assert!(room.is_connected_user(peer));

        room.apply_status(status("alice", false), peer);
        assert!(!room.is_connected_user(peer));
        assert_eq!(room.peer_status(&peer), Some(&Some(status("alice", false))));
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
