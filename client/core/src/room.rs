//! The room as this core takes part in it. The core always has a
//! [`RoomState`], in one of two roles. As the room's authority it decides
//! what the room agrees on: every user, its own included, asks for a change
//! with an [`Intent`], which the authority applies to the state and
//! publishes. As a follower it takes the state its host publishes. In both
//! roles the player and the UI are brought in line with the state by the
//! same [`CoreModel::follow`].

use std::collections::BTreeSet;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::CoreModel;
pub(crate) use crate::UserStatusListMsg;
use crate::communicator::{
    DirectMessage, ForcedRenameMsg, Intent, IntentMsg, RoomStateMsg, StreamEndedMsg, ToPeerMsg,
};
use crate::player::MediaPlayerTrait;
use crate::playlist::{Playlist, Video};
use crate::sync::ClientSync;
pub(crate) use crate::user::UserStatus;

mod state;
mod users;

use state::HOST_ACTOR;
pub use state::{RoomState, Timeline, select_next};
pub use users::{NameSuffixes, RoomUsers};

pub type RoomName = ArcStr;

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

/// Whose word the room's state is.
#[derive(Debug, Clone)]
pub enum Role {
    /// This core decides its room's state: it hosts the room, or it is in
    /// none and is a room of one.
    Authority(Authority),
    /// This core takes the state its room's host publishes.
    Follower(ClientSync),
}

/// What only the authority over a room keeps: how its states are told
/// apart, and where the room's users reach it.
#[derive(Debug, Clone, Default)]
pub struct Authority {
    /// This core's own peer id while it hosts a room.
    hosting: Option<PeerId>,
    version: u64,
    /// The version that set the playlist, see `RoomStateMsg`.
    playlist: u64,
}

/// What this core's player and UI were last told, or reported themselves.
/// Following the room tells them only what differs from this.
#[derive(Debug, Clone)]
pub struct Followed {
    /// The playlist and the selection the UI shows and the saved playlist
    /// holds.
    playlist: Playlist,
    video: Option<Video>,
    users: UserStatusListMsg,
    /// The UI's user edited the playlist or selected a video. The UI shows
    /// that already, whether or not the room takes it.
    pub(crate) unconfirmed: bool,
    /// The video the player holds.
    pub(crate) player: Option<Video>,
    /// Whether the player has finished loading its video. Until then it has
    /// no position to bring in line.
    pub(crate) loaded: bool,
    pub(crate) speed: f64,
    /// The jumps of the timeline the player's position was last brought in
    /// line with.
    pub(crate) jumps: Option<u64>,
}

impl Default for Followed {
    fn default() -> Self {
        Self {
            playlist: Playlist::default(),
            video: None,
            users: UserStatusListMsg::default(),
            unconfirmed: false,
            player: None,
            loaded: false,
            speed: 1.0,
            jumps: None,
        }
    }
}

fn same_users(a: &UserStatusListMsg, b: &UserStatusListMsg) -> bool {
    a.room_name == b.room_name
        && a.users.len() == b.users.len()
        && a.users.iter().zip(&b.users).all(|(a, b)| a.same_as(b))
}

impl CoreModel {
    pub fn is_host(&self) -> bool {
        matches!(&self.role, Role::Authority(authority) if authority.hosting.is_some())
    }

    fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority(_))
    }

    /// A session with the configured room began, in which this core hosts
    /// the room or follows its host.
    ///
    /// A host that already followed this room keeps the state it has —
    /// playlist, selection and position — so a lost host does not interrupt
    /// playback. Otherwise the room starts from its saved playlist, which
    /// the host's chat tells as the room's own changes. Nobody else is in
    /// the room yet: a peer joining is greeted with the room's state.
    pub(crate) fn connected(&mut self, is_host: bool, peer: PeerId) {
        let now = Instant::now();
        let room = self.config.room.clone();
        let followed = self.state.room.as_ref() == Some(&room);
        self.state.room = Some(room);
        // the first position of a session is taken exactly
        self.followed.jumps = None;
        if !is_host {
            self.role = Role::Follower(ClientSync::default());
            // the host knows this user and its offer from the status
            return self.publish_status();
        }
        self.role = Role::Authority(Authority {
            hosting: Some(peer),
            ..Default::default()
        });
        if !followed {
            self.restore_saved(now);
        }
        self.alone();
        self.follow(now);
        self.publish_change(false, now);
    }

    /// This core is in no room, and so the authority over the state it has.
    pub(crate) fn disconnected(&mut self) {
        if self.is_authority() && !self.is_host() {
            return;
        }
        self.role = Role::Authority(Authority::default());
        self.alone();
        self.follow(Instant::now());
    }

    /// The room starts from the playlist and selection saved for it, or
    /// empty.
    fn restore_saved(&mut self, now: Instant) {
        let room = self.state.room.as_ref();
        let saved = room.and_then(|room| (self.saved_playlist)(room));
        let restored = saved.is_some();
        let saved = saved.unwrap_or_default();
        let video = saved.get_current_video();
        self.state.playlist = saved.get_playlist();
        self.state.select(video.clone(), Duration::ZERO, now);
        if restored {
            self.show_action(Intent::Playlist(self.state.playlist.clone()));
        }
        if video.is_some() {
            let position = Duration::ZERO;
            self.show_action(Intent::Select { video, position });
        }
    }

    /// A line in this core's own chat for what it did on the room's behalf.
    fn show_action(&mut self, intent: Intent) {
        let actor = HOST_ACTOR;
        if let Some(action) = (IntentMsg { actor, intent }).action() {
            self.ui.player_message(action.into());
        }
    }

    /// Nobody but this core's own user is in the room.
    fn alone(&mut self) {
        self.state.users = RoomUsers::default();
        let status = self.own_status();
        self.state.users.update_local(status);
    }

    /// The host greets a peer that is reachable over a direct connection
    /// with the room's state. To the other members it is a heartbeat. A
    /// peer connecting again keeps its place.
    pub(crate) fn greet_peer(&mut self, peer: PeerId) {
        if !self.is_authority() {
            return debug!(%peer, "a peer connected to a follower");
        }
        if !self.state.users.contains(&peer) {
            self.state.users.add_connected(peer);
        }
        self.publish_room(Instant::now());
    }

    /// The host drops a peer whose last connection closed.
    pub(crate) fn peer_left(&mut self, peer: PeerId) {
        if !self.is_authority() || !self.state.users.remove(&peer) {
            return debug!(%peer, "disconnected peer was not in this core's room");
        }
        let now = Instant::now();
        self.follow(now);
        self.publish_change(false, now);
    }

    /// Asks the room for what this core's user did. The authority handles
    /// it like a peer's intent. A follower sends it to its host and leaves
    /// its state and player alone: what the user did in the player or the
    /// UI shows there already, and the rest comes with the host's state.
    pub fn request(&mut self, intent: Intent) {
        let msg = IntentMsg {
            actor: self.config.username.clone(),
            intent,
        };
        if let Role::Follower(sync) = &mut self.role {
            sync.intent_sent();
            return self.communicator.send(msg.into());
        }
        self.handle_intent(None, msg);
    }

    /// Announces our own status.
    pub fn publish_status(&mut self) {
        let status = self.own_status();
        self.request(Intent::Status(status));
    }

    /// This core's user as the room is to know it. What this peer offers is
    /// read from the video provider's route, which the communicator answers
    /// file requests from.
    fn own_status(&self) -> UserStatus {
        let hosting = match &self.role {
            Role::Authority(authority) => authority.hosting,
            Role::Follower(_) => None,
        };
        let player = self.followed.player.clone();
        UserStatus {
            name: self.config.username.clone(),
            ready: self.ready,
            offers: self.video_provider.offered(),
            loaded: player.filter(|_| self.followed.loaded),
            peer: hosting,
        }
    }

    /// The peers the room shows offering `video`, with the size of their
    /// file.
    pub(crate) fn providers_of(&self, video: &Video) -> Vec<(PeerId, u64)> {
        let offer = |user: &UserStatus| {
            let file = user.offers.as_ref().filter(|f| f.name == video.as_str())?;
            Some((user.peer?, file.size))
        };
        self.state.users.list().iter().filter_map(offer).collect()
    }

    /// The authority handles what a user asks of the room, a peer's intent
    /// or, without `peer`, its own user's, and publishes the outcome.
    ///
    /// It tells the room's chat what it did. This is the only source of the
    /// room's action lines: the authority shows each line itself, and every
    /// client, the actor included, shows it when it arrives.
    pub(crate) fn handle_intent(&mut self, peer: Option<PeerId>, msg: IntentMsg) {
        if !self.is_authority() {
            return warn!(?msg, "received an intent while following a host");
        }
        let now = Instant::now();
        let playlist_set = matches!(msg.intent, Intent::Playlist(_));
        let asked_name = match &msg.intent {
            Intent::Status(status) => Some(status.name.clone()),
            _ => None,
        };
        let hosting = self.is_host();
        for line in self.state.apply(peer, msg, now) {
            self.ui.player_message(line.clone().into());
            if hosting {
                self.communicator.send(line.into());
            }
        }
        if let (Some(peer), Some(asked)) = (peer, asked_name) {
            self.tell_forced_name(peer, &asked);
        }
        self.follow(now);
        self.publish_change(playlist_set, now);
    }

    /// Tells `peer` the name the room gave it, if that is not the one it
    /// asked for.
    fn tell_forced_name(&mut self, peer: PeerId, asked: &ArcStr) {
        let Some(Some(status)) = self.state.users.peer_status(&peer) else {
            return;
        };
        if status.name != *asked {
            let name = status.name.clone();
            let rename = DirectMessage::Rename(ForcedRenameMsg { name });
            self.communicator.send(ToPeerMsg::new(peer, rename).into());
        }
    }

    /// A follower takes the state its host published.
    pub(crate) fn take_room_state(&mut self, mut msg: RoomStateMsg) {
        let Role::Follower(sync) = &mut self.role else {
            return warn!("received a room state as the room's authority");
        };
        if let Some(playlist) = msg.playlist.take() {
            sync.playlist = playlist;
        }
        if !sync.admit(&msg) {
            return debug!(version = msg.version, "ignoring a room state");
        }
        let now = Instant::now();
        let position = sync.host_position(msg.position, msg.paused);
        if self.state.playlist != sync.playlist {
            self.state.playlist = sync.playlist.clone();
        }
        self.state.video = msg.video;
        self.state.timeline = Timeline::at(position, msg.paused, msg.speed, msg.jumps, now);
        self.state.users = RoomUsers::listed(msg.users.users);
        self.follow(now);
    }

    /// Brings the UI, the player and what this core shares in line with
    /// the room's state.
    pub(crate) fn follow(&mut self, now: Instant) {
        let playlist_changed = self.followed.playlist != self.state.playlist;
        let video_changed = self.followed.video != self.state.video;
        let unconfirmed = std::mem::take(&mut self.followed.unconfirmed);
        if playlist_changed || unconfirmed {
            self.followed.playlist = self.state.playlist.clone();
            self.ui.playlist(self.state.playlist.clone());
        }
        if self.followed.player != self.state.video {
            self.load(now);
        }
        if video_changed {
            self.followed.video = self.state.video.clone();
            self.selection_changed();
        }
        if video_changed || unconfirmed {
            self.ui.video_change(self.state.video.clone());
        }
        if playlist_changed || video_changed {
            self.save_playlist();
        }
        self.follow_playback(now);
        let users = self.user_list();
        if !same_users(&users, &self.followed.users) {
            self.followed.users = users.clone();
            self.ui.user_list(users.into());
        }
    }

    /// The player loads the selected video where the room is in it.
    fn load(&mut self, now: Instant) {
        let video = self.state.video.clone();
        match &video {
            Some(video) => {
                let position = self.state.timeline.position_at(now);
                let store = self.database.all_files();
                self.player.load_video(video.clone(), position, store);
            }
            None => self.player.unload_video(),
        }
        self.followed.player = video;
        self.followed.loaded = false;
    }

    /// What goes with the selection: a stream is one of the selected
    /// video, so the stream ends, and so is what this core shares.
    fn selection_changed(&mut self) {
        self.video_server.stop_server();
        self.communicator.send(StreamEndedMsg.into());
        let video = self.state.video.clone();
        self.reshare_or_stop(video.as_ref());
    }

    /// Brings the player's speed, play state and position in line with the
    /// room's timeline. A position the room jumped to and one the room is
    /// paused at are taken exactly; while the room plays on, the player
    /// only has to be within the tolerance.
    fn follow_playback(&mut self, now: Instant) {
        let timeline = self.state.timeline;
        if self.followed.speed != timeline.speed() {
            self.followed.speed = timeline.speed();
            self.player.set_speed(timeline.speed());
        }
        match (self.player.is_paused(), timeline.paused()) {
            (Some(true), false) => self.player.start(),
            (Some(false), true) => self.player.pause(),
            _ => {}
        }
        if !self.followed.loaded {
            return;
        }
        let position = timeline.position_at(now);
        let jumped = self.followed.jumps != Some(timeline.jumps());
        self.followed.jumps = Some(timeline.jumps());
        if jumped || timeline.paused() {
            self.player.align(position);
        } else {
            self.player.reconcile(position, timeline.speed());
        }
    }

    /// The authority's heartbeat: its own player follows the room's clock
    /// like everyone's, and the room is told where that clock is.
    pub(crate) fn heartbeat(&mut self) {
        if !self.is_authority() {
            return;
        }
        let now = Instant::now();
        self.follow(now);
        self.publish_room(now);
    }

    /// The authority publishes the room after a change, under a new
    /// version, which also names the playlist if that is what changed.
    fn publish_change(&mut self, playlist_set: bool, now: Instant) {
        let Role::Authority(authority) = &mut self.role else {
            return;
        };
        authority.version += 1;
        if playlist_set {
            authority.playlist = authority.version;
        }
        self.publish_room(now);
    }

    /// The host publishes the room as it is at `now`.
    fn publish_room(&mut self, now: Instant) {
        let Role::Authority(authority) = &self.role else {
            return;
        };
        if authority.hosting.is_none() {
            return;
        }
        let timeline = &self.state.timeline;
        let state = RoomStateMsg {
            version: authority.version,
            jumps: timeline.jumps(),
            playlist_version: authority.playlist,
            playlist: Some(self.state.playlist.clone()),
            video: self.state.video.clone(),
            position: timeline.position_at(now),
            paused: timeline.paused(),
            speed: timeline.speed(),
            users: self.user_list(),
        };
        self.communicator.send(state.into());
    }

    fn user_list(&self) -> UserStatusListMsg {
        UserStatusListMsg {
            room_name: self.state.room.clone().unwrap_or_default(),
            users: self.state.users.list().clone(),
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

    #[test]
    fn users_differ_by_more_than_their_names() {
        let list = |ready: bool| UserStatusListMsg {
            room_name: "room".into(),
            users: BTreeSet::from([UserStatus {
                name: "alice".into(),
                ready,
                ..Default::default()
            }]),
        };

        assert!(same_users(&list(true), &list(true)));
        assert!(!same_users(&list(true), &list(false)));
    }
}
