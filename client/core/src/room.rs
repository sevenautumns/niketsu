//! The room as this core takes part in it. The host keeps the authoritative
//! [`HostRoom`] and decides everything the room agrees on: who is in it,
//! forced renames, when playback starts, which video follows a removed one,
//! and what a fresh room starts from. Every user, the host's own included,
//! asks for a change with an [`Intent`]; the host applies it to its own
//! player and publishes the room's state, which a client follows.

use std::collections::BTreeSet;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tracing::{debug, warn};

use crate::CoreModel;
pub(crate) use crate::UserStatusListMsg;
use crate::chunk::SharedFile;
use crate::communicator::{
    DirectMessage, ForcedRenameMsg, Intent, IntentMsg, OutgoingMessage, RoomStateMsg, ToPeerMsg,
};
use crate::player::MediaPlayerTrait;
use crate::playlist::{Playlist, Video};
use crate::sync::Jump;
pub(crate) use crate::user::UserStatus;

mod users;

pub use users::{NameSuffixes, RoomUsers};

pub type RoomName = ArcStr;

/// The actor of what the host does on the room's behalf rather than a
/// user's.
const HOST_ACTOR: ArcStr = arcstr::literal!("host");

#[derive(Debug, Clone, Default)]
pub struct UserList {
    room: RoomName,
    list: BTreeSet<UserStatus>,
}

impl UserList {
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
}

impl From<UserStatusListMsg> for UserList {
    fn from(value: UserStatusListMsg) -> Self {
        Self {
            room: value.room_name,
            list: value.users,
        }
    }
}

/// What the host keeps of the room beside its own playlist and player: who
/// is in it, and the counters that order its states.
#[derive(Debug, Clone)]
pub struct HostRoom {
    pub users: RoomUsers,
    /// The host's own peer id, where the room's users reach the host.
    local: PeerId,
    version: u64,
    jumps: u64,
    /// The version that set the playlist, see `RoomStateMsg`.
    playlist: u64,
    /// The last jump of the host's player, while it has not landed.
    jump: Option<Jump>,
}

impl HostRoom {
    pub fn new(room: RoomName, local: PeerId) -> Self {
        Self {
            users: RoomUsers::new(room),
            local,
            version: 0,
            jumps: 0,
            playlist: 0,
            jump: None,
        }
    }

    /// Admits a directly connected peer. A peer connecting again keeps its
    /// place.
    pub fn admit(&mut self, peer: PeerId) {
        if !self.users.contains(&peer) {
            self.users.add_connected(peer);
        }
    }

    /// Applies a peer's own status. Returns the forced rename to send back
    /// when the peer asked for a name that is taken.
    pub fn peer_status(&mut self, peer: PeerId, status: UserStatus) -> Option<OutgoingMessage> {
        let forced = self.users.apply_status(status, peer)?;
        let rename = ForcedRenameMsg { name: forced.name };
        Some(ToPeerMsg::new(peer, DirectMessage::Rename(rename)).into())
    }

    /// The host's player was told to jump to `target`, and reported
    /// `before` right after.
    pub fn jumped(&mut self, target: Duration, before: Option<Duration>) {
        self.jumps += 1;
        self.jump = Some(Jump::new(target, before));
    }

    /// The room's position while the host's player reports `reported`.
    pub fn position(&mut self, reported: Option<Duration>) -> Duration {
        if let Some(target) = self.jump.and_then(|jump| jump.position(reported)) {
            return target;
        }
        self.jump = None;
        reported.unwrap_or_default()
    }
}

impl CoreModel {
    pub fn is_host(&self) -> bool {
        self.host.is_some()
    }

    /// Takes over the room. A core that already followed this room keeps
    /// what it has — playlist, running video and position — so a lost host
    /// does not interrupt playback. Otherwise the room starts from its
    /// saved playlist, which the host's chat tells as the room's own
    /// changes. Nobody else is in the room yet: a peer joining is greeted
    /// with the room's state.
    pub(crate) fn become_host(&mut self, local: PeerId) {
        let room = self.config.room.clone();
        if self.joined_room.as_ref() != Some(&room) {
            let saved = (self.saved_playlist)(&room);
            let restored = saved.is_some();
            let saved = saved.unwrap_or_default();
            let playlist = saved.get_playlist();
            let video = saved.get_current_video();
            self.show_playlist(playlist.clone());
            self.select_video(video.as_ref(), Duration::ZERO);
            if restored {
                self.show_action(&IntentMsg {
                    actor: HOST_ACTOR,
                    intent: Intent::Playlist(playlist),
                });
            }
            if video.is_some() {
                self.show_action(&IntentMsg {
                    actor: HOST_ACTOR,
                    intent: Intent::Select {
                        video,
                        position: Duration::ZERO,
                    },
                });
            }
        }
        self.host = Some(HostRoom::new(room, local));
    }

    /// The host greets a peer that is reachable over a direct connection
    /// with the room's state, at the host's current position. To the other
    /// members it is a heartbeat.
    pub(crate) fn greet_peer(&mut self, peer: PeerId) {
        let Some(host) = &mut self.host else {
            debug!(%peer, "ignoring peer connection as a client");
            return;
        };
        host.admit(peer);
        self.publish_room();
    }

    /// The host drops a peer whose last connection closed.
    pub(crate) fn peer_left(&mut self, peer: PeerId) {
        let Some(host) = &mut self.host else {
            return;
        };
        if host.users.remove(&peer) {
            self.publish_change(false);
        } else {
            debug!(%peer, "disconnected peer was not in the room");
        }
    }

    /// Asks the room for what this core's user did, which the local player
    /// or UI already shows. The host handles it like a peer's intent.
    pub fn request(&mut self, intent: Intent) {
        let msg = IntentMsg {
            actor: self.config.username.clone(),
            intent,
        };
        if self.is_host() {
            return self.handle_intent(None, msg);
        }
        self.apply_intent(&msg.intent);
        self.sync.intent_sent();
        self.communicator.send(msg.into());
    }

    /// Announces our own status. What this peer offers is read from the
    /// video provider's route, which the communicator answers file
    /// requests from.
    pub fn publish_status(&mut self) {
        let status = self
            .config
            .status(self.ready, self.video_provider.offered());
        self.request(Intent::Status(status));
    }

    /// The peers the room shows offering `video`, with the size of their
    /// file.
    pub(crate) fn providers_of(&self, video: &Video) -> Vec<(PeerId, u64)> {
        let users = match &self.host {
            Some(host) => &host.users.status_list().users,
            None => &self.sync.users,
        };
        let offer = |user: &UserStatus| {
            let file = user.offers.as_ref().filter(|f| f.name == video.as_str())?;
            Some((user.peer?, file.size))
        };
        users.iter().filter_map(offer).collect()
    }

    /// The host handles what a user asks of the room, a peer's intent or,
    /// without `peer`, its own user's, and publishes the outcome.
    pub(crate) fn handle_intent(&mut self, peer: Option<PeerId>, msg: IntentMsg) {
        if !self.is_host() {
            warn!(?msg, "received an intent without being the host");
            return;
        }
        let playlist = self.playlist.get_playlist();
        let video = self.player.playing_video();
        self.apply_intent(&msg.intent);
        self.tell(&msg);
        let IntentMsg { actor, intent } = msg;
        let playlist_set = matches!(intent, Intent::Playlist(_));
        let mut start = false;
        match intent {
            Intent::Seek { position, .. } | Intent::Select { position, .. } => {
                self.host_jumped(position)
            }
            Intent::Playlist(new) => self.select_next(&playlist, video.as_ref(), &new),
            Intent::Status(status) => {
                self.host_status(peer, status);
                start = true;
            }
            Intent::Pause | Intent::Start | Intent::Speed(_) => {}
        }
        // a new video starts paused, so a ready room starts it
        if start || self.player.playing_video() != video {
            self.start_if_all_ready(actor);
        }
        self.publish_change(playlist_set);
    }

    /// Brings the local player and playlist in line with what `intent` asks
    /// of the room.
    fn apply_intent(&mut self, intent: &Intent) {
        match intent {
            Intent::Pause => self.player.pause(),
            Intent::Start => self.player.start(),
            Intent::Speed(speed) => self.player.set_speed(*speed),
            Intent::Seek { video, position } => self.jump(Some(video), *position),
            Intent::Select { video, position } => self.jump(video.as_ref(), *position),
            Intent::Playlist(playlist) => self.show_playlist(playlist.clone()),
            Intent::Status(_) => {}
        }
    }

    /// Moves the player to `position` in `video`, which it loads unless it
    /// plays it already.
    fn jump(&mut self, video: Option<&Video>, position: Duration) {
        if self.player.playing_video().as_ref() != video {
            return self.select_video(video, position);
        }
        if self.player.get_position().is_some_and(|p| p != position) {
            self.player.set_position(position);
        }
    }

    fn show_playlist(&mut self, playlist: Playlist) {
        self.replace_playlist(playlist.clone());
        self.ui.playlist(playlist);
    }

    /// The host's player jumped to `target` for the room.
    fn host_jumped(&mut self, target: Duration) {
        let before = self.player.get_position();
        if let Some(host) = &mut self.host {
            host.jumped(target, before);
        }
    }

    /// If the playlist lost the video playing, the host moves the room on
    /// to the one that took its place.
    fn select_next(&mut self, old: &Playlist, video: Option<&Video>, new: &Playlist) {
        let Some(next) = select_next(old, video, new) else {
            return;
        };
        self.jump(Some(&next), Duration::ZERO);
        self.host_jumped(Duration::ZERO);
        self.tell(&IntentMsg {
            actor: HOST_ACTOR,
            intent: Intent::Select {
                video: Some(next),
                position: Duration::ZERO,
            },
        });
    }

    fn host_status(&mut self, peer: Option<PeerId>, mut status: UserStatus) {
        let video = self.player.playing_video();
        let Some(host) = &mut self.host else {
            return;
        };
        let selected =
            |offer: &SharedFile| video.as_ref().is_some_and(|v| offer.name == v.as_str());
        status.offers = status.offers.filter(selected);
        status.peer = Some(peer.unwrap_or(host.local));
        let Some(peer) = peer else {
            return host.users.update_local(status);
        };
        if let Some(rename) = host.peer_status(peer, status) {
            self.communicator.send(rename);
        }
    }

    /// Playback starts for the whole room once every user, the host
    /// included, is ready.
    fn start_if_all_ready(&mut self, actor: ArcStr) {
        if !self.host.as_ref().is_some_and(|h| h.users.all_ready()) {
            return;
        }
        debug!("All users are ready. Starting playback for the room");
        self.player.start();
        self.tell(&IntentMsg {
            actor,
            intent: Intent::Start,
        });
    }

    /// The host tells the room's chat what it did for `msg`. This is the
    /// only source of the room's action lines: the host shows each line
    /// itself, and every client, the actor included, shows it when it
    /// arrives. A client shows nothing when it sends its own intent, so
    /// each line appears once per peer.
    fn tell(&mut self, msg: &IntentMsg) {
        let Some(action) = msg.action() else {
            return;
        };
        self.ui.player_message(action.clone().into());
        self.communicator.send(action.into());
    }

    /// A line in the host's own chat for what it did.
    fn show_action(&mut self, msg: &IntentMsg) {
        if let Some(action) = msg.action() {
            self.ui.player_message(action.into());
        }
    }

    /// The host publishes the room after a change, under a new version,
    /// which also names the playlist if that is what changed.
    fn publish_change(&mut self, playlist_set: bool) {
        let Some(host) = &mut self.host else {
            return;
        };
        host.version += 1;
        if playlist_set {
            host.playlist = host.version;
        }
        let users = host.users.status_list().clone();
        self.ui.user_list(UserList::from(users));
        self.publish_room();
    }

    /// The host publishes the room as it is; the heartbeat does so to
    /// refresh the room's clock.
    pub(crate) fn publish_room(&mut self) {
        if let Some(state) = self.room_state() {
            self.communicator.send(state.into());
        }
    }

    /// The room as the host has it: its own player, its playlist and who
    /// is in the room.
    fn room_state(&mut self) -> Option<RoomStateMsg> {
        let host = self.host.as_mut()?;
        let reported = self.player.get_position();
        Some(RoomStateMsg {
            version: host.version,
            jumps: host.jumps,
            playlist_version: host.playlist,
            playlist: Some(self.playlist.get_playlist()),
            video: self.player.playing_video(),
            position: host.position(reported),
            paused: self.player.is_paused().unwrap_or(true),
            speed: self.player.host_speed(),
            file_loaded: self.player.video_loaded(),
            users: host.users.status_list().clone(),
        })
    }

    /// A client brings itself in line with the room's state.
    pub(crate) fn follow_room(&mut self, mut state: RoomStateMsg) {
        if self.is_host() {
            return warn!("ignoring a room state as the host");
        }
        if let Some(playlist) = state.playlist.take() {
            self.sync.playlist = playlist;
        }
        let Some(admitted) = self.sync.admit(&state) else {
            return debug!(version = state.version, "ignoring a room state");
        };
        if self.sync.playlist != self.playlist.get_playlist() {
            self.show_playlist(self.sync.playlist.clone());
        }
        self.player.set_speed(state.speed);
        let position = self.sync.host_position(state.position, state.paused);
        if admitted.jumped || self.player.playing_video() != state.video {
            self.jump(state.video.as_ref(), position);
        }
        if admitted.changed {
            self.sync.users = state.users.users.clone();
            self.ui.user_list(UserList::from(state.users));
        }
        // While the host is still loading it reports the play state and
        // position of the previous video. Following either would start our
        // copy and yank it back with every state.
        if !state.file_loaded {
            return self.player.pause();
        }
        match (self.player.is_paused(), state.paused) {
            (Some(true), false) => self.player.start(),
            (Some(false), true) => self.player.pause(),
            _ => {}
        }
        if !admitted.jumped {
            self.player.reconcile(position);
        }
    }
}

/// When the playlist shrank and the video playing was removed, picks the
/// video that took its place by walking the common prefix of the old and
/// new playlist.
pub fn select_next(
    current_playlist: &Playlist,
    current_video: Option<&Video>,
    new_playlist: &Playlist,
) -> Option<Video> {
    if new_playlist.len() >= current_playlist.len() {
        return None;
    }

    let mut new_position = 0;
    let max_len = new_playlist.len();
    if let Some(current_video) = current_video {
        for old_video in current_playlist.iter() {
            if let Some(new_video) = new_playlist.get(new_position) {
                debug!(?current_video, ?new_video, "Current video and old");
                if new_video == current_video {
                    // No need to select if current video is still in playlist
                    return None;
                }

                if old_video == current_video {
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

    new_playlist.get(new_position).cloned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_len() {
        let mut user_list = UserList::default();

        user_list.list.insert(UserStatus {
            name: "User1".into(),
            ..Default::default()
        });
        user_list.list.insert(UserStatus {
            name: "User2".into(),
            ..Default::default()
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
            ..Default::default()
        });

        // Now, it should not be empty
        assert!(!room_list.is_empty());
    }

    #[test]
    fn test_from_niketsu_user_status() {
        let mut btreeset = BTreeSet::new();

        btreeset.insert(UserStatus {
            name: arcstr::literal!("User1"),
            ready: true,
            ..Default::default()
        });

        let user_list: UserList = UserStatusListMsg {
            room_name: arcstr::literal!("room"),
            users: btreeset,
        }
        .into();

        // Ensure the RoomList contains the converted data.
        assert!(user_list.iter().any(|user| user.name == "User1"));
    }

    fn playlist(videos: &[&str]) -> Playlist {
        videos.iter().copied().collect()
    }

    fn selected(result: Option<Video>) -> Option<String> {
        result.map(|v| v.as_str().to_string())
    }

    fn peer(n: u8) -> PeerId {
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    #[test]
    fn admitting_a_peer_again_keeps_its_place() {
        let mut room = HostRoom::new("room".into(), peer(1));
        let bob = peer(2);

        room.admit(bob);
        room.peer_status(
            bob,
            UserStatus {
                name: "bob".into(),
                ready: true,
                ..Default::default()
            },
        );
        room.admit(bob);

        assert!(!room.users.is_connected_user(&bob), "bob's status stays");
        assert_eq!(room.users.status_list().users.len(), 1);
    }

    #[test]
    fn the_room_is_told_a_jumps_target_until_the_player_moved() {
        let mut room = HostRoom::new("room".into(), peer(1));
        let (before, target) = (Duration::from_secs(10), Duration::from_secs(300));

        room.jumped(target, Some(before));
        assert_eq!(room.position(Some(before)), target);

        let moved = Duration::from_secs(301);
        assert_eq!(room.position(Some(moved)), moved);
        assert_eq!(
            room.position(Some(before)),
            before,
            "a landed jump is not told again"
        );
    }

    #[test]
    fn select_next_ignores_grown_or_equal_playlist() {
        let old = playlist(&["a", "b"]);
        let a = Video::from("a");

        assert!(select_next(&old, Some(&a), &playlist(&["a", "b"])).is_none());
        assert!(select_next(&old, Some(&a), &playlist(&["a", "b", "c"])).is_none());
    }

    #[test]
    fn select_next_keeps_current_video_if_still_present() {
        let old = playlist(&["a", "b", "c"]);
        let b = Video::from("b");

        assert!(select_next(&old, Some(&b), &playlist(&["a", "b"])).is_none());
    }

    #[test]
    fn select_next_picks_successor_when_current_removed_in_middle() {
        let old = playlist(&["a", "b", "c"]);
        let b = Video::from("b");

        let result = select_next(&old, Some(&b), &playlist(&["a", "c"]));

        assert_eq!(selected(result), Some("c".into()));
    }

    #[test]
    fn select_next_picks_new_first_when_current_removed_at_start() {
        let old = playlist(&["a", "b", "c"]);
        let a = Video::from("a");

        let result = select_next(&old, Some(&a), &playlist(&["b", "c"]));

        assert_eq!(selected(result), Some("b".into()));
    }

    #[test]
    fn select_next_picks_last_when_current_removed_at_end() {
        let old = playlist(&["a", "b", "c"]);
        let c = Video::from("c");

        let result = select_next(&old, Some(&c), &playlist(&["a", "b"]));

        assert_eq!(selected(result), Some("b".into()));
    }

    #[test]
    fn select_next_returns_none_for_emptied_playlist() {
        let old = playlist(&["a"]);
        let a = Video::from("a");

        assert!(select_next(&old, Some(&a), &playlist(&[])).is_none());
    }

    // Documents current behavior: with nothing selected, any shrink of the
    // playlist selects the new first video. Whether that is intended is an
    // open question; see select_next.
    #[test]
    fn select_next_without_selection_picks_first_video() {
        let old = playlist(&["a", "b", "c"]);

        let result = select_next(&old, None, &playlist(&["a", "b"]));

        assert_eq!(selected(result), Some("a".into()));
    }
}
