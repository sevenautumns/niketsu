//! The room as this core takes part in it. The core always has a
//! [`RoomState`], in one of two roles. As the room's authority it decides
//! what the room agrees on: every user, its own included, asks for a change
//! with an [`Intent`], which the authority applies to the state and
//! publishes. As a follower it takes the state its host publishes. In both
//! roles the player and the UI are brought in line with the state by the
//! same [`CoreModel::follow`].

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::CoreModel;
use crate::chunk::SharedFile;
use crate::communicator::{Intent, RoomStateMsg};
use crate::player::Source;
use crate::player::offset::{ALIGNMENT, Offset};
use crate::playlist::{Playlist, Video, VideoInner};
use crate::sync::ClientSync;
use crate::ui::PlayerMessage;
use crate::user::{User, UserStatus};

mod state;
mod users;

pub use state::{RoomState, Timeline};
pub use users::NameSuffixes;

pub type RoomName = ArcStr;

/// The room's users as the UI shows them.
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

/// Whose word the room's state is.
#[derive(Debug, Clone)]
pub enum Role {
    /// This core decides its room's state: it hosts the room, or it is in
    /// none and is a room of one.
    Authority(Authority),
    /// This core takes the state its room's host publishes.
    Follower(ClientSync),
}

/// What only the authority over a room keeps: whether its room has other
/// users to reach.
#[derive(Debug, Clone, Default)]
pub struct Authority {
    hosting: bool,
}

/// What this core's player and UI were last told, or reported themselves.
/// Following the room tells them only what differs from this.
#[derive(Debug, Clone)]
pub struct Followed {
    /// The playlist and the selection the UI shows and the saved playlist
    /// holds.
    playlist: Playlist,
    video: Option<Video>,
    /// The room and the users the UI's user list shows.
    room: Option<RoomName>,
    users: BTreeMap<PeerId, User>,
    /// The UI's user edited the playlist or selected a video. The UI shows
    /// that already, whether or not the room takes it.
    pub(crate) unconfirmed: bool,
    /// The video the player was last told to hold.
    pub(crate) player: Option<Video>,
    /// What the player plays that video from. Without one the player holds
    /// nothing.
    pub(crate) source: Option<Source>,
    /// Whether the player has finished loading its video. Until then it has
    /// no position to bring in line.
    pub(crate) loaded: bool,
    pub(crate) speed: f64,
    /// The jumps of the timeline the player's position was last brought in
    /// line with.
    pub(crate) jumps: Option<u64>,
    /// How far the player has been from the room while both played on.
    offset: Offset,
}

impl Default for Followed {
    fn default() -> Self {
        Self {
            playlist: Playlist::default(),
            video: None,
            room: None,
            users: BTreeMap::new(),
            unconfirmed: false,
            player: None,
            source: None,
            loaded: false,
            speed: 1.0,
            jumps: None,
            offset: Offset::default(),
        }
    }
}

impl CoreModel {
    pub fn is_host(&self) -> bool {
        matches!(&self.role, Role::Authority(authority) if authority.hosting)
    }

    fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority(_))
    }

    /// This core's own peer id, under which the room knows its user.
    pub(crate) fn me(&self) -> PeerId {
        self.communicator.peer()
    }

    /// A session with the configured room began, in which this core hosts
    /// the room or follows its host.
    ///
    /// A host that already followed this room keeps the state it has —
    /// playlist, selection and position — so a lost host does not interrupt
    /// playback. Otherwise the room starts from its saved playlist, which
    /// the host's chat tells as the room's own changes. Nobody else is in
    /// the room yet: a peer is in it from its first status on.
    pub(crate) fn connected(&mut self, is_host: bool) {
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
        self.role = Role::Authority(Authority { hosting: true });
        self.alone();
        if !followed {
            self.restore_saved(now);
        }
        self.follow(now);
        self.publish_room(now);
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
        if let Some(line) = self.state.line(self.me(), &intent) {
            self.ui.player_message(line.into());
        }
    }

    /// Nobody but this core's own user is in the room.
    fn alone(&mut self) {
        self.state.users = BTreeMap::from([(self.me(), self.own_user())]);
    }

    /// The host drops a peer whose last connection closed.
    pub(crate) fn peer_left(&mut self, peer: PeerId) {
        if !self.is_authority() || self.state.users.remove(&peer).is_none() {
            return debug!(%peer, "disconnected peer was not in this core's room");
        }
        let now = Instant::now();
        self.follow(now);
        self.publish_room(now);
    }

    /// Asks the room for what this core's user did. The authority handles
    /// it like a peer's intent. A follower sends it to its host and leaves
    /// its state and player alone: what the user did in the player or the
    /// UI shows there already, and the rest comes with the first state the
    /// host publishes after it handled the intent.
    pub fn request(&mut self, intent: Intent) {
        if let Role::Follower(sync) = &mut self.role {
            sync.sent += 1;
            return self.communicator.send(intent.into());
        }
        self.handle_intent(self.me(), intent);
    }

    /// Announces our own status.
    pub fn publish_status(&mut self) {
        let user = self.own_user();
        self.request(Intent::Status(user));
    }

    /// This core's user as the room is to know it. What this peer offers is
    /// read from the video provider's route, which the communicator answers
    /// file requests from.
    fn own_user(&self) -> User {
        let player = self.followed.player.clone();
        let handled = match &self.role {
            Role::Follower(sync) => sync.sent,
            Role::Authority(_) => self.state.users.get(&self.me()).map_or(0, |me| me.handled),
        };
        User {
            name: self.config.username.clone(),
            ready: self.ready,
            loaded: player.filter(|_| self.followed.loaded),
            offers: self.video_provider.offered(),
            handled,
        }
    }

    /// The peers the room shows offering `video`, with their file.
    pub(crate) fn providers_of(&self, video: &Video) -> Vec<(PeerId, SharedFile)> {
        let offer = |(peer, user): (&PeerId, &User)| Some((*peer, user.offer_of(video)?.clone()));
        self.state.users.iter().filter_map(offer).collect()
    }

    /// The authority handles what the user at `peer` asks of the room, and
    /// publishes the outcome.
    ///
    /// It tells the room's chat what it did. This is the only source of the
    /// room's lines: the authority shows each line itself, and every
    /// client shows it when it arrives. A chat line is the exception, which
    /// its author's chat shows from the moment it was written.
    pub(crate) fn handle_intent(&mut self, peer: PeerId, intent: Intent) {
        if !self.is_authority() {
            return warn!(?intent, "received an intent while following a host");
        }
        let now = Instant::now();
        let me = self.me();
        let hosting = self.is_host();
        for line in self.state.apply(peer, intent, now) {
            if line.author() != Some(me) {
                self.ui.player_message(line.clone().into());
            }
            if hosting {
                self.communicator.send(line.into());
            }
        }
        self.follow(now);
        self.publish_room(now);
    }

    /// A follower takes the state its host published, unless the host
    /// published it before it handled what this core's user asked.
    pub(crate) fn take_room_state(&mut self, mut msg: RoomStateMsg) {
        let me = self.me();
        let Role::Follower(sync) = &mut self.role else {
            return warn!("received a room state as the room's authority");
        };
        if let Some(playlist) = msg.playlist.take() {
            sync.playlist = playlist;
        }
        if !sync.admits(msg.users.get(&me)) {
            return debug!(
                sent = sync.sent,
                "ignoring a room state from before our intents"
            );
        }
        let now = Instant::now();
        let position = sync.host_position(msg.position, msg.paused);
        if self.state.playlist != sync.playlist {
            self.state.playlist = sync.playlist.clone();
        }
        self.state.video = msg.video;
        self.state.timeline = Timeline::at(position, msg.paused, msg.speed, msg.jumps, now);
        self.state.users = msg.users;
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
            // what this core shares is a file of the selected video
            let video = self.state.video.clone();
            self.reshare_or_stop(video.as_ref());
        }
        if video_changed || unconfirmed {
            self.ui.video_change(self.state.video.clone());
        }
        if playlist_changed || video_changed {
            self.save_playlist();
        }
        self.follow_playback(now);
        self.take_given_name();
        let users_changed = self.followed.users != self.state.users;
        if users_changed || self.followed.room != self.state.room || video_changed {
            self.followed.users = self.state.users.clone();
            self.followed.room = self.state.room.clone();
            self.ui.user_list(self.user_list());
        }
    }

    /// This core's user goes by the name the room knows it by, which is
    /// another than the one it asked for when another user goes by that.
    fn take_given_name(&mut self) {
        let Some(user) = self.state.users.get(&self.me()) else {
            return;
        };
        if user.name == self.config.username {
            return;
        }
        let name = user.name.clone();
        self.config.username = name.clone();
        self.ui.username_change(name.clone());
        self.ui.player_message(PlayerMessage::renamed(&name));
    }

    /// The player loads the selected video where the room is in it, from
    /// where this core finds it.
    fn load(&mut self, now: Instant) {
        let source = self.state.video.as_ref().and_then(|video| self.find(video));
        self.play(source, now);
    }

    /// The player plays the selected video from `source` where the room is
    /// in it, or holds nothing. A stream is one of these sources, so it
    /// ends when the player is given anything else.
    pub(crate) fn play(&mut self, source: Option<Source>, now: Instant) {
        match source.clone() {
            Some(source) => {
                let position = self.state.timeline.position_at(now);
                self.player.load(source, position);
            }
            None => self.player.unload(),
        }
        self.followed.player = self.state.video.clone();
        self.followed.source = source;
        self.followed.loaded = false;
    }

    /// Where this core has `video` itself: a URL is its own source, a file
    /// is one the file database knows.
    fn find(&self, video: &Video) -> Option<Source> {
        match &**video {
            VideoInner::Url(url) => Some(Source::Url(url.to_string())),
            VideoInner::File(name) => {
                let file = self.database.find_file(name)?;
                Some(Source::File(file.path().to_path_buf()))
            }
        }
    }

    /// The file database knows other files than before. A selected video
    /// the player had nothing to play from is loaded if it is among them.
    pub(crate) fn files_changed(&mut self) {
        if self.followed.source.is_some() {
            return;
        }
        let source = self.state.video.as_ref().and_then(|video| self.find(video));
        if source.is_some() {
            self.play(source, Instant::now());
        }
    }

    /// Brings the player's speed, play state and position in line with the
    /// room's timeline. A position the room jumped to and one the room is
    /// paused at are taken exactly; while the room plays on, the player
    /// is judged by its offset over a few seconds. A player without a
    /// position is brought in line once it has one.
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
        let at = match self.followed.loaded {
            true => self.player.get_position(),
            false => None,
        };
        let Some(at) = at else {
            return self.followed.offset.restart();
        };
        let room = timeline.position_at(now);
        let jumped = self.followed.jumps.replace(timeline.jumps()) != Some(timeline.jumps());
        let off = match jumped || timeline.paused() {
            true => at.abs_diff(room) > ALIGNMENT,
            false => self
                .followed
                .offset
                .measure(at, room, timeline.speed(), now),
        };
        if off {
            debug!(?at, ?room, "putting the player where the room is");
            self.player.set_position(room);
        }
        if off || jumped || timeline.paused() {
            self.followed.offset.restart();
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

    /// The host publishes the room as it is at `now`.
    fn publish_room(&mut self, now: Instant) {
        if !self.is_host() {
            return;
        }
        let timeline = &self.state.timeline;
        let state = RoomStateMsg {
            jumps: timeline.jumps(),
            playlist: Some(self.state.playlist.clone()),
            video: self.state.video.clone(),
            position: timeline.position_at(now),
            paused: timeline.paused(),
            speed: timeline.speed(),
            users: self.state.users.clone(),
        };
        self.communicator.send(state.into());
    }

    /// The users as the UI shows them. An offer shows while it is one of
    /// the selected video.
    fn user_list(&self) -> UserList {
        let video = self.state.video.as_ref();
        let shown = |user: &User| UserStatus {
            name: user.name.clone(),
            ready: user.ready,
            offers: video.and_then(|video| user.offer_of(video)).cloned(),
        };
        UserList {
            room: self.state.room.clone().unwrap_or_default(),
            list: self.state.users.values().map(shown).collect(),
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
}
