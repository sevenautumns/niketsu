use std::collections::{BTreeSet, HashMap};

use arcstr::ArcStr;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use multiaddr::PeerId;

use super::UserStatus;
use crate::playlist::Video;

/// Where the suffix of a forced name comes from: a user whose name is taken
/// is renamed to `name_<suffix>`.
pub type NameSuffixes = fn() -> String;

fn buzzword() -> String {
    Buzzword().fake()
}

/// How many suffixes are tried before a counter is appended, so finding a
/// free name ends even when every suffix is taken.
const SUFFIX_ROLLS: usize = 16;

/// Who is in the room. The authority over the room keys remote peers by
/// their peer id and has its own slot, since it never talks to itself over
/// the network; a follower only has the list the host published.
#[derive(Debug, Clone)]
pub struct RoomUsers {
    status_list: BTreeSet<UserStatus>,
    local: Option<UserStatus>,
    peers: HashMap<PeerId, Option<UserStatus>>,
    suffixes: NameSuffixes,
}

/// Nobody is in the room, and forced names get a buzzword suffix.
impl Default for RoomUsers {
    fn default() -> Self {
        Self::with_suffixes(buzzword)
    }
}

impl RoomUsers {
    pub fn with_suffixes(suffixes: NameSuffixes) -> Self {
        Self {
            status_list: BTreeSet::default(),
            local: None,
            peers: HashMap::default(),
            suffixes,
        }
    }

    /// The users as the room's host published them.
    pub fn listed(users: BTreeSet<UserStatus>) -> Self {
        Self {
            status_list: users,
            ..Self::default()
        }
    }

    pub fn list(&self) -> &BTreeSet<UserStatus> {
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

    /// Whether every user is ready and has `video`, the room's selection,
    /// loaded in its player.
    pub fn all_ready(&self, video: Option<&Video>) -> bool {
        let ready = |user: &UserStatus| user.ready && user.loaded.as_ref() == video;
        self.status_list.iter().all(ready)
    }

    /// Sets the readiness of the user at `peer`, or of the local one
    /// without it. A peer that has sent no status yet has none to set.
    pub fn set_ready(&mut self, peer: Option<PeerId>, ready: bool) {
        let status = match peer {
            Some(peer) => self.peers.get_mut(&peer).and_then(Option::as_mut),
            None => self.local.as_mut(),
        };
        if let Some(status) = status {
            status.ready = ready;
            self.status_list.replace(status.clone());
        }
    }

    /// Removes the peer from the user map and the status list.
    /// Returns false if the peer was not known at all.
    pub fn remove(&mut self, peer: &PeerId) -> bool {
        let Some(status) = self.peers.remove(peer) else {
            return false;
        };
        if let Some(s) = status {
            self.status_list.remove(&s);
        }
        true
    }

    /// Forgets what every user offers.
    pub fn drop_offers(&mut self) {
        let statuses = self.peers.values_mut().flatten().chain(&mut self.local);
        statuses.for_each(|status| status.offers = None);
        let users = std::mem::take(&mut self.status_list);
        let without_offers = users.into_iter().map(|status| UserStatus {
            offers: None,
            ..status
        });
        self.status_list = without_offers.collect();
    }

    /// Whether any user, the host included, goes by `name`.
    fn username_exists(&self, name: &str) -> bool {
        self.status_list.iter().any(|u| u.name == name)
    }

    /// A name no user goes by: `username_<suffix>`, rolled again while
    /// taken, with a counter appended once the rolls are used up.
    fn roll_new_username(&self, username: &str) -> ArcStr {
        let mut candidate = ArcStr::default();
        for _ in 0..SUFFIX_ROLLS {
            candidate = arcstr::format!("{username}_{}", (self.suffixes)());
            if !self.username_exists(&candidate) {
                return candidate;
            }
        }
        let mut counter = 2u32;
        loop {
            let numbered = arcstr::format!("{candidate}_{counter}");
            if !self.username_exists(&numbered) {
                return numbered;
            }
            counter += 1;
        }
    }

    /// The host's own status. It is never force-renamed: the host decides
    /// names, so its own wins.
    pub fn update_local(&mut self, status: UserStatus) {
        if let Some(old) = self.local.take()
            && old.name != status.name
        {
            self.status_list.remove(&old);
        }
        self.status_list.replace(status.clone());
        self.local = Some(status);
    }

    fn update_peer(&mut self, status: UserStatus, peer: PeerId) {
        self.status_list.replace(status.clone());
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
                        ..status.clone()
                    };
                    forced = Some(new_status.clone());
                }
                if let Some(s) = &old_status {
                    self.status_list.remove(s);
                }
            }
            // same name: typical status update, only need to update map & list
        } else if self.is_connected_user(&peer) && self.username_exists(&status.name) {
            // new user whose name is already taken
            new_status = UserStatus {
                name: self.roll_new_username(&status.name),
                ..status.clone()
            };
            forced = Some(new_status.clone());
        }

        self.update_peer(new_status, peer);
        forced
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn peer(n: u8) -> PeerId {
        // an identity multihash: a valid peer id without key material
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    fn status(name: &str, ready: bool) -> UserStatus {
        UserStatus {
            name: name.into(),
            ready,
            ..Default::default()
        }
    }

    fn names(room: &RoomUsers) -> Vec<&str> {
        room.list().iter().map(|u| u.name.as_str()).collect()
    }

    /// The name `peer` goes by in the room.
    fn name_of(room: &RoomUsers, peer: PeerId) -> Option<&str> {
        let status = room.peer_status(&peer)?.as_ref()?;
        Some(status.name.as_str())
    }

    fn synergy() -> String {
        "synergy".into()
    }

    /// A room whose forced names are predictable.
    fn room() -> RoomUsers {
        RoomUsers::with_suffixes(synergy)
    }

    #[test]
    fn new_user_keeps_free_name() {
        let mut room = room();
        let alice = peer(1);
        room.add_connected(alice);

        let forced = room.apply_status(status("alice", false), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
    }

    #[test]
    fn new_user_with_taken_name_is_renamed() {
        let mut room = room();
        let (alice, intruder) = (peer(1), peer(2));
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder);

        let forced = forced.expect("collision must force a rename");
        assert_eq!(forced.name, "alice_synergy");
        assert_eq!(room.list().len(), 2);
        // the original user is untouched
        assert_eq!(name_of(&room, alice), Some("alice"));
        assert_eq!(name_of(&room, intruder), Some("alice_synergy"));
    }

    #[test]
    fn forced_names_are_buzzwords_by_default() {
        let mut room = RoomUsers::default();
        room.update_local(status("alice", true));
        let intruder = peer(2);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder).unwrap();

        let suffix = forced.name.strip_prefix("alice_").unwrap();
        assert!(!suffix.is_empty());
        assert!(
            !suffix.chars().all(|c| c.is_ascii_digit()),
            "a buzzword, not a counter: {suffix}"
        );
    }

    #[test]
    fn new_user_taking_the_hosts_name_is_renamed() {
        let mut room = room();
        room.update_local(status("alice", true));
        let intruder = peer(2);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder);

        let forced = forced.expect("the host's name is taken too");
        assert_eq!(forced.name, "alice_synergy");
        assert_eq!(names(&room), vec!["alice", "alice_synergy"]);
        assert_eq!(name_of(&room, intruder), Some("alice_synergy"));
    }

    #[test]
    fn rolled_names_avoid_each_other() {
        let mut room = room();
        room.update_local(status("alice", true));
        let mut rolled = vec![];
        for n in 2..6 {
            let p = peer(n);
            room.add_connected(p);
            rolled.push(room.apply_status(status("alice", false), p).unwrap().name);
        }

        // the one suffix is taken after the first roll, so a counter follows
        assert_eq!(
            rolled,
            [
                "alice_synergy",
                "alice_synergy_2",
                "alice_synergy_3",
                "alice_synergy_4"
            ]
        );
        assert_eq!(room.list().len(), 5);
    }

    #[test]
    fn a_forced_name_avoids_the_hosts_name() {
        let mut room = room();
        // the host took the name the first roll would produce
        room.update_local(status("alice_synergy", true));
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);
        let intruder = peer(2);
        room.add_connected(intruder);

        let forced = room.apply_status(status("alice", false), intruder).unwrap();

        assert_eq!(forced.name, "alice_synergy_2");
        assert_eq!(room.list().len(), 3);
    }

    #[test]
    fn ready_toggle_updates_in_place() {
        let mut room = room();
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", false), alice);
        assert!(!room.all_ready(None));

        let forced = room.apply_status(status("alice", true), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alice"]);
        assert!(room.all_ready(None));
    }

    #[test]
    fn all_ready_includes_the_host() {
        let mut room = room();
        let bob = peer(2);
        room.add_connected(bob);
        room.apply_status(status("bob", true), bob);
        room.update_local(status("alice", false));
        assert!(!room.all_ready(None));

        room.update_local(status("alice", true));
        assert!(room.all_ready(None));
    }

    #[test]
    fn ready_takes_the_selected_video_loaded() {
        let mut room = room();
        let ep1 = Video::from("ep1");
        room.update_local(UserStatus {
            loaded: Some(Video::from("ep0")),
            ..status("alice", true)
        });
        assert!(!room.all_ready(Some(&ep1)), "another video is loaded");

        room.update_local(UserStatus {
            loaded: Some(ep1.clone()),
            ..status("alice", true)
        });
        assert!(room.all_ready(Some(&ep1)));
    }

    #[test]
    fn readiness_is_set_for_the_host_and_for_a_peer_with_a_status() {
        let mut room = room();
        let (bob, carol) = (peer(2), peer(3));
        room.update_local(status("alice", false));
        room.add_connected(bob);
        room.apply_status(status("bob", false), bob);
        room.add_connected(carol);

        room.set_ready(None, true);
        room.set_ready(Some(bob), true);
        room.set_ready(Some(carol), true);

        assert!(room.all_ready(None));
        assert_eq!(names(&room), vec!["alice", "bob"]);
    }

    #[test]
    fn host_rename_replaces_its_old_entry() {
        let mut room = room();
        room.update_local(status("alice", true));
        room.update_local(status("alicia", true));

        assert_eq!(names(&room), vec!["alicia"]);
    }

    #[test]
    fn rename_to_free_name_replaces_old_entry() {
        let mut room = room();
        let alice = peer(1);
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);

        let forced = room.apply_status(status("alicia", true), alice);

        assert!(forced.is_none());
        assert_eq!(names(&room), vec!["alicia"]);
        assert_eq!(name_of(&room, alice), Some("alicia"));
    }

    #[test]
    fn rename_into_collision_is_rerolled_and_leaves_other_user_alone() {
        let mut room = room();
        let (alice, bob) = (peer(1), peer(2));
        room.add_connected(alice);
        room.apply_status(status("alice", true), alice);
        room.add_connected(bob);
        room.apply_status(status("bob", true), bob);

        // bob renames himself to "alice"
        let forced = room.apply_status(status("alice", true), bob);

        let forced = forced.expect("collision must force a rename");
        assert_eq!(forced.name, "alice_synergy");
        // alice keeps her entry, bob's old name is gone, no ghost entries
        assert_eq!(names(&room), vec!["alice", "alice_synergy"]);
        assert_eq!(name_of(&room, alice), Some("alice"));
        assert_eq!(name_of(&room, bob), Some("alice_synergy"));
    }

    #[test]
    fn remove_drops_user_and_status() {
        let mut room = room();
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
        let mut room = room();
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
}
