//! The room as this core takes part in it. The host keeps the authoritative
//! [`HostRoom`] and decides everything the room agrees on: who is in it,
//! what a peer joining later is greeted with, forced renames, when playback
//! starts, which video follows a removed one, and what a fresh room starts
//! from. A client forwards its own changes to the host and follows what the
//! room sends it.

use std::collections::BTreeSet;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tracing::{debug, warn};

pub(crate) use crate::UserStatusListMsg;
use crate::communicator::{
    ClientRequest, DirectMessage, ForcedRenameMsg, HostInstruction, OutgoingMessage, PlaylistMsg,
    SeekMsg, SelectMsg, StartMsg, ToPeerMsg,
};
use crate::player::MediaPlayerTrait;
use crate::ui::PlayerMessage;
pub(crate) use crate::user::UserStatus;
use crate::{CoreModel, EventHandler};

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
    /// the peer's behalf. A peer that has not told its name yet has none, so
    /// the host acts in its own.
    pub fn actor_of(&self, peer: &PeerId) -> ArcStr {
        match self.users.peer_status(peer) {
            Some(Some(status)) => status.name.clone(),
            Some(None) | None => HOST_ACTOR,
        }
    }

    /// Admits a directly connected peer and returns what greets it: the
    /// room's playlist and selection for the peer, and the user list for the
    /// room. A peer connecting again is greeted again.
    pub fn greet(&mut self, peer: PeerId) -> [OutgoingMessage; 3] {
        if !self.users.contains(&peer) {
            self.users.add_connected(peer);
        }
        let users = self.users.status_list().clone();
        debug!(?users, %peer, "greeting peer with the room state");
        [
            ToPeerMsg::new(peer, DirectMessage::Playlist(self.playlist.clone())).into(),
            ToPeerMsg::new(peer, DirectMessage::Select(self.select.clone())).into(),
            users.into(),
        ]
    }

    /// Applies a peer's own status. Returns the forced rename to send back
    /// when the peer asked for a name that is taken.
    pub fn peer_status(&mut self, peer: PeerId, status: UserStatus) -> Option<OutgoingMessage> {
        let forced = self.users.apply_status(status, peer)?;
        let rename = ForcedRenameMsg { name: forced.name };
        Some(ToPeerMsg::new(peer, DirectMessage::Rename(rename)).into())
    }

    /// Takes `msg` as the room's playlist. Returns the selection the room
    /// moves on to when the playlist lost the selected video.
    pub fn adopt_playlist(&mut self, msg: PlaylistMsg) -> Option<SelectMsg> {
        let next = select_next(&self.playlist, &self.select, &msg);
        self.playlist = msg;
        if let Some(select) = &next {
            self.select = select.clone();
        }
        next
    }

    pub fn adopt_select(&mut self, msg: SelectMsg) {
        self.select = msg;
    }

    /// Playback starts for the whole room once every user, the host
    /// included, is ready.
    pub fn start_if_all_ready(&self, actor: ArcStr) -> Option<StartMsg> {
        self.users.all_ready().then_some(StartMsg { actor })
    }
}

/// How this core takes part in the room. For its own changes a client sends
/// a request to the host, while the host adopts the change into the room
/// directly; everything a host decides on the room's behalf is here too.
impl CoreModel {
    pub fn is_host(&self) -> bool {
        self.host.is_some()
    }

    /// Takes over the room. A core that already followed this room keeps
    /// what it has — playlist, running video and position — so a lost host
    /// does not interrupt playback. Otherwise the room starts from its
    /// saved playlist.
    pub(crate) fn become_host(&mut self) {
        let room = self.config.room.clone();
        let select = if self.joined_room.as_ref() == Some(&room) {
            SelectMsg {
                actor: HOST_ACTOR,
                position: self.player.get_position().unwrap_or_default(),
                video: self.player.playing_video(),
            }
        } else {
            let saved = (self.saved_playlist)(&room).unwrap_or_default();
            let select = SelectMsg {
                actor: HOST_ACTOR,
                position: Duration::ZERO,
                video: saved.get_current_video(),
            };
            self.apply_playlist(PlaylistMsg {
                actor: HOST_ACTOR,
                playlist: saved.get_playlist(),
            });
            self.apply_select(select.clone());
            select
        };
        let playlist = PlaylistMsg {
            actor: HOST_ACTOR,
            playlist: self.playlist.get_playlist(),
        };
        self.host = Some(HostRoom::new(room, playlist, select));
    }

    /// The host greets a peer that is reachable over a direct connection.
    pub(crate) fn greet_peer(&mut self, peer: PeerId) {
        let Some(host) = &mut self.host else {
            debug!(%peer, "ignoring peer connection as a client");
            return;
        };
        // The selection's position follows the heartbeats and lags behind a
        // seek since the last one. A peer greeted with it would start there
        // and, that far behind, have the room wait for it.
        if self.player.playing_video() == host.select.video
            && let Some(position) = self.player.get_position()
        {
            host.select.position = position;
        }
        for msg in host.greet(peer) {
            self.communicator.send(msg);
        }
    }

    /// The host drops a peer whose last connection closed.
    pub(crate) fn peer_left(&mut self, peer: PeerId) {
        let Some(host) = &mut self.host else {
            return;
        };
        if host.users.remove(&peer) {
            self.publish_user_list();
        } else {
            debug!(%peer, "disconnected peer was not in the room");
        }
    }

    /// The host applies a peer's own status.
    pub(crate) fn peer_status(&mut self, peer: PeerId, status: UserStatus) {
        let Some(host) = &mut self.host else {
            warn!(%peer, "received a peer status without being the host");
            return;
        };
        if let Some(rename) = host.peer_status(peer, status) {
            self.communicator.send(rename);
        }
        let actor = host.actor_of(&peer);
        self.start_if_all_ready(actor);
        self.publish_user_list();
    }

    /// Shows a playlist that arrived from the room (or was restored).
    pub(crate) fn apply_playlist(&mut self, msg: PlaylistMsg) {
        self.replace_playlist(msg.playlist.clone());
        self.ui.playlist(msg.playlist.clone());
        self.ui.player_message(PlayerMessage::from(msg));
    }

    /// Plays a selection that arrived from the room (or was restored).
    pub(crate) fn apply_select(&mut self, msg: SelectMsg) {
        self.select_video(msg.video.as_ref(), msg.position);
        self.ui.player_message(PlayerMessage::from(msg));
    }

    /// A playlist from the room: a client follows it; the host also adopts a
    /// client's edit and passes it on, since it reaches the room only
    /// through the host.
    pub(crate) fn receive_playlist(&mut self, msg: PlaylistMsg) {
        self.apply_playlist(msg.clone());
        if self.is_host() {
            self.communicator
                .send(HostInstruction::Playlist(msg.clone()).into());
            self.host_adopt_playlist(msg);
        }
    }

    /// A selection from the room, which the host also adopts.
    pub(crate) fn receive_select(&mut self, msg: SelectMsg) {
        self.apply_select(msg.clone());
        self.host_adopt_select(msg);
    }

    /// A seek from the room. Within the running video it only moves the
    /// player. Into another video it is a selection of that video, which
    /// the host also adopts, so a peer joining later is greeted with the
    /// video the room plays. Whether playback starts stays with the seek's
    /// sender: the host does not start the room over it.
    pub(crate) fn receive_seek(&mut self, msg: &SeekMsg) {
        if self.player.playing_video().as_ref() == Some(&msg.video) {
            self.playlist.select_playing(&msg.video);
            self.save_playlist();
            self.player.set_position(msg.position);
            return;
        }
        self.select_video(Some(&msg.video), msg.position);
        if let Some(host) = &mut self.host {
            host.adopt_select(SelectMsg {
                actor: msg.actor.clone(),
                position: msg.position,
                video: Some(msg.video.clone()),
            });
        }
    }

    /// Announces our own status: a client tells the host, the host updates
    /// the room directly.
    pub fn publish_status(&mut self) {
        let status = self.config.status(self.ready);
        let Some(host) = &mut self.host else {
            self.communicator
                .send(ClientRequest::UserStatus(status).into());
            return;
        };
        let actor = status.name.clone();
        host.users.update_local(status);
        self.start_if_all_ready(actor);
        self.publish_user_list();
    }

    /// Our own playlist edit, already applied locally.
    pub fn publish_playlist(&mut self, msg: PlaylistMsg) {
        if !self.is_host() {
            self.communicator.send(ClientRequest::Playlist(msg).into());
            return;
        }
        self.host_adopt_playlist(msg.clone());
        self.communicator
            .send(HostInstruction::Playlist(msg).into());
    }

    /// Our own selection, already applied locally.
    pub fn publish_select(&mut self, msg: SelectMsg) {
        self.communicator.send(msg.clone().into());
        self.host_adopt_select(msg);
    }

    /// The host takes `msg` as the room's playlist. If the playlist lost the
    /// selected video, the host moves the room on to the one that took its
    /// place.
    fn host_adopt_playlist(&mut self, msg: PlaylistMsg) {
        let Some(host) = &mut self.host else {
            return;
        };
        let actor = msg.actor.clone();
        let next = host.adopt_playlist(msg);
        let Some(select) = next else {
            return;
        };
        self.apply_select(select.clone());
        self.communicator.send(select.into());
        self.start_if_all_ready(actor);
    }

    fn host_adopt_select(&mut self, msg: SelectMsg) {
        let Some(host) = &mut self.host else {
            return;
        };
        let actor = msg.actor.clone();
        host.adopt_select(msg);
        self.start_if_all_ready(actor);
    }

    fn start_if_all_ready(&mut self, actor: ArcStr) {
        let Some(start) = self
            .host
            .as_ref()
            .and_then(|host| host.start_if_all_ready(actor))
        else {
            return;
        };
        debug!("All users are ready. Starting playback for the room");
        self.communicator.send(start.clone().into());
        start.handle(self);
    }

    /// The host shows the room's user list and sends it to the room.
    fn publish_user_list(&mut self) {
        let Some(host) = &self.host else {
            return;
        };
        let list = host.users.status_list().clone();
        self.ui.user_list(UserList::from(list.clone()));
        self.communicator.send(list.into());
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
        actor: HOST_ACTOR,
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
        assert!(user_list.iter().any(|user| user.name == "User1"));
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

    fn peer(n: u8) -> PeerId {
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    #[test]
    fn greeting_admits_the_peer_once_and_replays_the_room() {
        let mut room = HostRoom::new("room".into(), playlist_msg(&["a"]), select_msg(Some("a")));
        let bob = peer(2);

        let greeting = room.greet(bob);
        room.greet(bob);

        assert!(room.users.is_connected_user(&bob));
        assert_eq!(
            greeting[0],
            ToPeerMsg::new(bob, DirectMessage::Playlist(playlist_msg(&["a"]))).into()
        );
        assert_eq!(
            greeting[1],
            ToPeerMsg::new(bob, DirectMessage::Select(select_msg(Some("a")))).into()
        );
    }

    #[test]
    fn adopting_a_playlist_without_the_selected_video_moves_on() {
        let mut room = HostRoom::new(
            "room".into(),
            playlist_msg(&["a", "b", "c"]),
            select_msg(Some("b")),
        );

        let next = room.adopt_playlist(playlist_msg(&["a", "c"]));

        assert_eq!(selected(next), Some("c".into()));
        assert_eq!(room.select.video, Some(Video::from("c")));
        assert_eq!(room.playlist, playlist_msg(&["a", "c"]));
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
