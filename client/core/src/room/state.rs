//! The room's state as a plain value, and every rule that changes it. The
//! authority over a room applies what its users ask to this value and
//! publishes it; everyone, the authority included, brings its player and UI
//! in line with it. Nothing here reads a clock or a player: time comes in
//! as a parameter.

use std::collections::BTreeMap;
use std::time::Duration;

use multiaddr::PeerId;
use tokio::time::Instant;
use tracing::debug;

use super::RoomName;
use super::users::{NameSuffixes, buzzword, free_name};
use crate::communicator::{Intent, LineMsg};
use crate::playlist::{Playlist, Video};
use crate::user::User;

/// Whether a player can play at `speed`. A timeline only ever has such a
/// speed, whatever a peer sends.
fn playable(speed: f64) -> bool {
    speed.is_finite() && speed > 0.0
}

/// Where the room is in its video: a position at an instant, from which it
/// plays on at `speed` unless paused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// The position at `since`.
    position: Duration,
    since: Instant,
    paused: bool,
    speed: f64,
    /// Bumped whenever the position is set rather than played to. Whoever
    /// follows the timeline takes such a position exactly.
    jumps: u64,
}

impl Timeline {
    /// Paused at the start, at normal speed.
    pub fn new(now: Instant) -> Self {
        Self::at(Duration::ZERO, true, 1.0, 0, now)
    }

    /// The timeline that is at `position` at `now`. A speed no player
    /// plays at counts as the normal one.
    pub fn at(position: Duration, paused: bool, speed: f64, jumps: u64, now: Instant) -> Self {
        Self {
            position,
            since: now,
            paused,
            speed: if playable(speed) { speed } else { 1.0 },
            jumps,
        }
    }

    pub fn position_at(&self, now: Instant) -> Duration {
        if self.paused {
            return self.position;
        }
        let elapsed = now.saturating_duration_since(self.since).as_secs_f64();
        let played = Duration::try_from_secs_f64(elapsed * self.speed).unwrap_or(Duration::MAX);
        self.position.saturating_add(played)
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn jumps(&self) -> u64 {
        self.jumps
    }

    pub fn pause(&mut self, now: Instant) {
        self.anchor(now);
        self.paused = true;
    }

    pub fn start(&mut self, now: Instant) {
        if self.paused {
            self.since = now;
            self.paused = false;
        }
    }

    pub fn set_speed(&mut self, speed: f64, now: Instant) {
        self.anchor(now);
        self.speed = speed;
    }

    pub fn jump(&mut self, to: Duration, now: Instant) {
        self.position = to;
        self.since = now;
        self.jumps += 1;
    }

    /// Keeps the position played to so far, so that what changes next only
    /// counts from `now` on.
    fn anchor(&mut self, now: Instant) {
        self.position = self.position_at(now);
        self.since = now;
    }
}

/// Everything a room agrees on.
#[derive(Debug, Clone)]
pub struct RoomState {
    /// The room this is the state of. None while this core has been in no
    /// room, and its state is only its own.
    pub room: Option<RoomName>,
    pub playlist: Playlist,
    /// The selected video.
    pub video: Option<Video>,
    pub timeline: Timeline,
    /// Every user of the room by peer id, the authority's own included.
    pub users: BTreeMap<PeerId, User>,
    suffixes: NameSuffixes,
}

impl RoomState {
    /// An empty room with nobody in it.
    pub fn new(now: Instant) -> Self {
        Self {
            room: None,
            playlist: Playlist::default(),
            video: None,
            timeline: Timeline::new(now),
            users: BTreeMap::new(),
            suffixes: buzzword,
        }
    }

    /// Does what the user at `peer` asks of the room. Returns the chat
    /// lines for what was done, each in the name the room knows the user
    /// by. A user is in the room from its first status on; until then it
    /// has nothing else to ask. Every intent of a user in the room counts
    /// as handled, also one that changes nothing: the count tells the user
    /// that the room's state is the outcome of what it asked. A status
    /// sets the count it goes on from, see [`User::handled`].
    pub fn apply(&mut self, peer: PeerId, intent: Intent, now: Instant) -> Vec<LineMsg> {
        let status = matches!(intent, Intent::Status(_));
        if !status && !self.users.contains_key(&peer) {
            debug!(%peer, ?intent, "ignoring an intent of a user outside the room");
            return vec![];
        }
        let selected = self.video.clone();
        let mut lines = self.act(peer, intent, now);
        if let Some(user) = self.users.get_mut(&peer) {
            user.handled += 1;
        }
        // a new video starts paused, so a ready room starts it
        if (status || self.video != selected) && self.start_if_all_ready(now) {
            lines.extend(self.line(peer, &Intent::Start));
        }
        lines
    }

    fn act(&mut self, peer: PeerId, intent: Intent, now: Instant) -> Vec<LineMsg> {
        let mut lines: Vec<LineMsg> = self.line(peer, &intent).into_iter().collect();
        match intent {
            Intent::Pause => {
                self.timeline.pause(now);
                self.set_ready(peer, false);
            }
            Intent::CachePause => self.timeline.pause(now),
            Intent::Start => {
                self.timeline.start(now);
                self.set_ready(peer, true);
            }
            Intent::Speed(speed) if playable(speed) => self.timeline.set_speed(speed, now),
            Intent::Speed(speed) => {
                debug!(speed, "ignoring a speed no player plays at");
                return vec![];
            }
            Intent::Seek { video, position } => self.select(Some(video), position, now),
            Intent::Select { video, position } => self.select(video, position, now),
            Intent::Ended(video) => {
                // every player reaches the end, and the room moves on once
                if self.video.as_ref() != Some(&video) {
                    return vec![];
                }
                let next = self.video_after(&video);
                lines.extend(self.line(peer, &selection(next.clone())));
                self.select(next, Duration::ZERO, now);
            }
            Intent::Playlist(playlist) => {
                let old = std::mem::replace(&mut self.playlist, playlist);
                if let Some(next) = select_next(&old, self.video.as_ref(), &self.playlist) {
                    // the edit that removed the selected video selects its
                    // successor
                    lines.extend(self.line(peer, &selection(Some(next.clone()))));
                    self.select(Some(next), Duration::ZERO, now);
                }
            }
            Intent::Status(user) => self.take_status(peer, user),
            Intent::Chat(_) => {}
        }
        lines
    }

    /// The line telling the room what the user at `peer` did, in the name
    /// the room knows the user by.
    pub(crate) fn line(&self, peer: PeerId, intent: &Intent) -> Option<LineMsg> {
        let actor = &self.users.get(&peer)?.name;
        intent.line(actor, peer)
    }

    /// Moves the room to `position` in `video`. Another video than the
    /// selected one becomes the selection and starts paused.
    pub(crate) fn select(&mut self, video: Option<Video>, position: Duration, now: Instant) {
        if self.video != video {
            self.video = video;
            self.timeline.pause(now);
        }
        self.timeline.jump(position, now);
    }

    fn video_after(&self, video: &Video) -> Option<Video> {
        let index = self.playlist.find(video)?;
        self.playlist.get(index + 1).cloned()
    }

    /// Takes a user's own status, under a name no other user goes by.
    fn take_status(&mut self, peer: PeerId, mut user: User) {
        user.name = free_name(&self.users, &user.name, peer, self.suffixes);
        self.users.insert(peer, user);
    }

    fn set_ready(&mut self, peer: PeerId, ready: bool) {
        if let Some(user) = self.users.get_mut(&peer) {
            user.ready = ready;
        }
    }

    /// Playback starts for the whole room once every user is ready and has
    /// the selected video loaded. Returns whether it started.
    fn start_if_all_ready(&mut self, now: Instant) -> bool {
        let Some(video) = &self.video else {
            return false;
        };
        let ready = |user: &User| user.ready && user.has_loaded(video);
        let start = self.timeline.paused() && self.users.values().all(ready);
        if start {
            debug!("All users are ready. Starting playback for the room");
            self.timeline.start(now);
        }
        start
    }
}

/// The intent of selecting `video` from its start.
fn selection(video: Option<Video>) -> Intent {
    Intent::Select {
        video,
        position: Duration::ZERO,
    }
}

/// The video an edit of the playlist selects: when the edit removed the
/// `selected` video, the one now at its index, or the last one when the
/// playlist ends before that.
fn select_next(old: &Playlist, selected: Option<&Video>, new: &Playlist) -> Option<Video> {
    let selected = selected?;
    if new.find(selected).is_some() {
        return None;
    }
    let index = old.find(selected)?;
    new.get(index).or(new.iter().last()).cloned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::chunk::SharedFile;
    use crate::communicator::LineKind;

    const SECOND: Duration = Duration::from_secs(1);

    fn peer(n: u8) -> PeerId {
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    fn playlist(videos: &[&str]) -> Playlist {
        videos.iter().copied().collect()
    }

    /// The authority's own user.
    fn alice() -> PeerId {
        peer(1)
    }

    fn user(name: &str, ready: bool, loaded: Option<&str>) -> User {
        User {
            name: name.into(),
            ready,
            loaded: loaded.map(Video::from),
            ..Default::default()
        }
    }

    fn status(name: &str, ready: bool, loaded: Option<&str>) -> Intent {
        Intent::Status(user(name, ready, loaded))
    }

    fn select(video: &str) -> Intent {
        selection(Some(Video::from(video)))
    }

    fn lines(lines: Vec<LineMsg>) -> Vec<String> {
        lines.into_iter().map(|line| line.message).collect()
    }

    fn name_of(state: &RoomState, peer: PeerId) -> &str {
        state.users[&peer].name.as_str()
    }

    fn names(state: &RoomState) -> Vec<&str> {
        let mut names: Vec<&str> = state.users.values().map(|u| u.name.as_str()).collect();
        names.sort();
        names
    }

    fn synergy() -> String {
        "synergy".into()
    }

    /// A room whose given names are predictable.
    fn empty_room(now: Instant) -> RoomState {
        RoomState {
            suffixes: synergy,
            ..RoomState::new(now)
        }
    }

    /// A room of alice, its authority, and bob, with ep1 to ep3 in the
    /// playlist.
    fn room(now: Instant) -> (RoomState, PeerId) {
        let mut state = empty_room(now);
        let bob = peer(2);
        state.apply(alice(), status("alice", false, None), now);
        state.apply(bob, status("bob", false, None), now);
        let episodes = Intent::Playlist(playlist(&["ep1", "ep2", "ep3"]));
        state.apply(alice(), episodes, now);
        (state, bob)
    }

    /// The room with ep1 selected, which alice and bob have loaded.
    fn room_with_ep1(now: Instant) -> (RoomState, PeerId) {
        let (mut state, bob) = room(now);
        state.apply(alice(), select("ep1"), now);
        let loaded = Some("ep1");
        state.apply(alice(), status("alice", false, loaded), now);
        state.apply(bob, status("bob", false, loaded), now);
        (state, bob)
    }

    #[test]
    fn a_paused_timeline_stays_and_a_started_one_plays_at_its_speed() {
        let t0 = Instant::now();
        let mut timeline = Timeline::new(t0);
        assert_eq!(timeline.position_at(t0 + 5 * SECOND), Duration::ZERO);

        timeline.start(t0);
        assert_eq!(timeline.position_at(t0 + 5 * SECOND), 5 * SECOND);

        timeline.set_speed(2.0, t0 + 5 * SECOND);
        assert_eq!(timeline.position_at(t0 + 5 * SECOND), 5 * SECOND);
        assert_eq!(timeline.position_at(t0 + 8 * SECOND), 11 * SECOND);

        timeline.pause(t0 + 8 * SECOND);
        assert_eq!(timeline.position_at(t0 + 60 * SECOND), 11 * SECOND);
        assert_eq!(timeline.jumps(), 0, "playing to a position is no jump");
    }

    #[test]
    fn starting_a_playing_timeline_changes_nothing() {
        let t0 = Instant::now();
        let mut timeline = Timeline::new(t0);
        timeline.start(t0);
        timeline.start(t0 + 5 * SECOND);
        assert_eq!(timeline.position_at(t0 + 8 * SECOND), 8 * SECOND);
    }

    #[test]
    fn a_jump_sets_the_position_and_is_counted() {
        let t0 = Instant::now();
        let mut timeline = Timeline::new(t0);
        timeline.start(t0);
        timeline.jump(300 * SECOND, t0 + 5 * SECOND);

        assert_eq!(timeline.jumps(), 1);
        assert_eq!(timeline.position_at(t0 + 5 * SECOND), 300 * SECOND);
        assert_eq!(timeline.position_at(t0 + 6 * SECOND), 301 * SECOND);
    }

    #[test]
    fn a_speed_no_player_plays_at_is_not_the_rooms() {
        let t0 = Instant::now();
        for speed in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let timeline = Timeline::at(10 * SECOND, false, speed, 0, t0);
            assert_eq!(timeline.speed(), 1.0);
            assert_eq!(timeline.position_at(t0 + 5 * SECOND), 15 * SECOND);
        }

        let (mut state, _) = room_with_ep1(t0);
        let told = state.apply(alice(), Intent::Speed(f64::NAN), t0);
        assert!(told.is_empty());
        assert_eq!(state.timeline.speed(), 1.0);
    }

    #[test]
    fn a_users_pause_and_start_are_the_rooms_and_set_their_readiness() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);

        let told = state.apply(bob, Intent::Start, t0);
        assert_eq!(lines(told), ["bob started playback"]);
        assert!(!state.timeline.paused());
        let ready = |state: &RoomState, peer: PeerId| state.users[&peer].ready;
        assert!(ready(&state, bob) && !ready(&state, alice()));

        state.apply(alice(), Intent::Start, t0 + SECOND);
        let told = state.apply(bob, Intent::Pause, t0 + 4 * SECOND);
        assert_eq!(lines(told), ["bob paused playback"]);
        assert!(state.timeline.paused());
        assert_eq!(state.timeline.position_at(t0 + 9 * SECOND), 4 * SECOND);
        assert!(!ready(&state, bob) && ready(&state, alice()));
    }

    #[test]
    fn a_cache_pause_pauses_the_room_and_leaves_readiness_alone() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        state.apply(alice(), Intent::Start, t0);
        state.apply(bob, Intent::Start, t0);

        state.apply(bob, Intent::CachePause, t0 + SECOND);

        assert!(state.timeline.paused());
        assert!(state.users.values().all(|u| u.ready));
    }

    #[test]
    fn a_new_selection_starts_paused_at_its_position() {
        let t0 = Instant::now();
        let (mut state, _) = room_with_ep1(t0);
        state.apply(alice(), Intent::Start, t0);
        let jumps = state.timeline.jumps();

        let told = state.apply(alice(), select("ep2"), t0 + 5 * SECOND);

        assert_eq!(lines(told), ["alice selected ep2"]);
        assert_eq!(state.video, Some(Video::from("ep2")));
        assert!(state.timeline.paused());
        assert_eq!(state.timeline.position_at(t0 + 9 * SECOND), Duration::ZERO);
        assert_eq!(state.timeline.jumps(), jumps + 1);
    }

    #[test]
    fn selecting_the_selected_video_is_a_seek() {
        let t0 = Instant::now();
        let (mut state, _) = room_with_ep1(t0);
        state.apply(alice(), Intent::Start, t0);

        state.apply(alice(), select("ep1"), t0 + 5 * SECOND);

        assert!(!state.timeline.paused(), "the room plays on");
        assert_eq!(state.timeline.position_at(t0 + 6 * SECOND), SECOND);
    }

    #[test]
    fn a_seek_into_another_video_selects_it() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        let seek = Intent::Seek {
            video: Video::from("ep2"),
            position: 300 * SECOND,
        };

        let told = state.apply(bob, seek, t0);

        assert_eq!(lines(told), ["bob seeked to 300s"]);
        assert_eq!(state.video, Some(Video::from("ep2")));
        assert!(state.timeline.paused());
        assert_eq!(state.timeline.position_at(t0), 300 * SECOND);
    }

    #[test]
    fn the_room_starts_once_everyone_is_ready_and_has_the_video_loaded() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(alice(), status("alice", true, None), t0);
        state.apply(bob, status("bob", true, None), t0);

        let told = state.apply(alice(), select("ep1"), t0);
        assert_eq!(lines(told), ["alice selected ep1"]);
        assert!(state.timeline.paused(), "nobody has loaded it yet");

        state.apply(alice(), status("alice", true, Some("ep1")), t0);
        assert!(state.timeline.paused(), "bob has not loaded it yet");

        state.apply(bob, status("bob", true, Some("ep0")), t0);
        assert!(state.timeline.paused(), "bob has another video loaded");

        let told = state.apply(bob, status("bob", true, Some("ep1")), t0);
        assert_eq!(lines(told), ["bob started playback"]);
        assert!(!state.timeline.paused());

        let told = state.apply(bob, status("bob", true, Some("ep1")), t0);
        assert!(told.is_empty(), "a room that plays is not started");
    }

    #[test]
    fn a_video_loaded_before_its_selection_counts_once_it_is_selected() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(alice(), status("alice", true, Some("ep1")), t0);
        state.apply(bob, status("bob", true, Some("ep1")), t0);

        let told = state.apply(alice(), select("ep1"), t0);

        assert_eq!(
            lines(told),
            ["alice selected ep1", "alice started playback"]
        );
        assert!(!state.timeline.paused());
    }

    #[test]
    fn an_explicit_start_does_not_wait_for_anyone() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(alice(), select("ep1"), t0);

        state.apply(bob, Intent::Start, t0);

        assert!(!state.timeline.paused());
    }

    #[test]
    fn a_video_that_ended_for_two_users_moves_the_room_on_once() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        let ended = || Intent::Ended(Video::from("ep1"));

        let told = state.apply(alice(), ended(), t0);
        assert_eq!(lines(told), ["alice selected ep2"]);
        assert_eq!(state.video, Some(Video::from("ep2")));
        state.timeline.jump(40 * SECOND, t0);
        let jumps = state.timeline.jumps();

        let told = state.apply(bob, ended(), t0);
        assert!(told.is_empty());
        assert_eq!(state.video, Some(Video::from("ep2")));
        assert_eq!(state.timeline.jumps(), jumps, "ep2 is not seeked back");
    }

    #[test]
    fn the_end_of_the_last_video_unselects_it() {
        let t0 = Instant::now();
        let (mut state, _) = room(t0);
        state.apply(alice(), select("ep3"), t0);

        let told = state.apply(alice(), Intent::Ended(Video::from("ep3")), t0);

        assert_eq!(lines(told), ["alice unselected video"]);
        assert_eq!(state.video, None);
    }

    #[test]
    fn removing_the_selected_video_selects_its_successor() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(alice(), select("ep2"), t0);

        let edit = Intent::Playlist(playlist(&["ep1", "ep3"]));
        let told = state.apply(bob, edit, t0);

        assert_eq!(lines(told), ["bob changed playlist", "bob selected ep3"]);
        assert_eq!(state.video, Some(Video::from("ep3")));
        assert_eq!(state.playlist, playlist(&["ep1", "ep3"]));
    }

    #[test]
    fn an_offer_counts_for_the_video_it_names() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        let offer = |video: &str| {
            Intent::Status(User {
                offers: Some(SharedFile {
                    name: video.into(),
                    size: 1,
                }),
                ..user("bob", false, Some("ep1"))
            })
        };
        let offered = |state: &RoomState| {
            let video = state.video.as_ref().unwrap();
            let offers = state.users.values().filter_map(|u| u.offer_of(video));
            offers.map(|file| file.name.to_string()).collect::<Vec<_>>()
        };

        state.apply(bob, offer("ep2"), t0);
        assert!(offered(&state).is_empty(), "ep2 is not selected");

        state.apply(bob, offer("ep1"), t0);
        assert_eq!(offered(&state), ["ep1"]);

        state.apply(alice(), select("ep2"), t0);
        assert!(offered(&state).is_empty(), "the offer is one of ep1");
        assert!(state.users[&bob].offers.is_some(), "nobody cleared it");
    }

    #[test]
    fn a_free_name_is_kept_and_a_taken_one_is_suffixed() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        let carol = peer(3);

        state.apply(carol, status("carol", false, None), t0);
        assert_eq!(name_of(&state, carol), "carol");

        let impostor = peer(4);
        state.apply(impostor, status("bob", false, None), t0);
        assert_eq!(name_of(&state, impostor), "bob_synergy");
        assert_eq!(name_of(&state, bob), "bob", "the first bob keeps his name");

        state.apply(impostor, status("bob_synergy", true, None), t0);
        assert_eq!(name_of(&state, impostor), "bob_synergy", "its own name");
    }

    #[test]
    fn a_rename_to_a_taken_name_is_suffixed_and_leaves_the_other_user_alone() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);

        state.apply(bob, status("alice", false, None), t0);

        assert_eq!(name_of(&state, bob), "alice_synergy");
        assert_eq!(names(&state), ["alice", "alice_synergy"]);

        // the authority's own user is named like everyone else
        state.apply(alice(), status("alice_synergy", false, None), t0);
        assert_eq!(name_of(&state, alice()), "alice_synergy_synergy");
    }

    #[test]
    fn given_names_avoid_each_other() {
        let t0 = Instant::now();
        let (mut state, _) = room(t0);
        let rolled: Vec<String> = (3..7)
            .map(|n| {
                state.apply(peer(n), status("alice", false, None), t0);
                name_of(&state, peer(n)).to_string()
            })
            .collect();

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
    }

    #[test]
    fn given_names_are_buzzwords_by_default() {
        let t0 = Instant::now();
        let mut state = RoomState::new(t0);
        state.apply(alice(), status("alice", false, None), t0);
        let intruder = peer(2);

        state.apply(intruder, status("alice", false, None), t0);

        let suffix = name_of(&state, intruder).strip_prefix("alice_").unwrap();
        assert!(!suffix.is_empty());
        assert!(
            !suffix.chars().all(|c| c.is_ascii_digit()),
            "a buzzword, not a counter: {suffix}"
        );
    }

    #[test]
    fn a_line_is_in_the_name_the_room_knows_its_user_by() {
        let t0 = Instant::now();
        let (mut state, _) = room(t0);
        let impostor = peer(4);
        state.apply(impostor, status("alice", false, None), t0);

        let told = state.apply(impostor, Intent::Chat("hi".into()), t0);

        assert_eq!(
            told,
            [LineMsg {
                actor: "alice_synergy".into(),
                message: "hi".into(),
                kind: LineKind::Chat { author: impostor },
            }]
        );
    }

    #[test]
    fn a_user_outside_the_room_asks_for_nothing_but_its_status() {
        let t0 = Instant::now();
        let (mut state, _) = room(t0);
        let stranger = peer(5);

        assert!(
            state
                .apply(stranger, Intent::Chat("hi".into()), t0)
                .is_empty()
        );
        assert!(state.apply(stranger, select("ep2"), t0).is_empty());
        assert_eq!(state.video, None);
        assert!(!state.users.contains_key(&stranger));
    }

    #[test]
    fn every_intent_of_a_user_in_the_room_counts_as_handled() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        let handled = |state: &RoomState, peer: PeerId| state.users[&peer].handled;
        assert_eq!(handled(&state, bob), 1, "the status that brought bob in");
        let alices = handled(&state, alice());

        state.apply(bob, select("ep1"), t0);
        state.apply(bob, Intent::Speed(f64::NAN), t0);
        state.apply(bob, Intent::Ended(Video::from("ep3")), t0);
        assert_eq!(handled(&state, bob), 4, "also what changes nothing");

        let fifth = User {
            handled: 4,
            ..user("bob", true, None)
        };
        state.apply(bob, Intent::Status(fifth), t0);
        assert_eq!(handled(&state, bob), 5, "a status says what came before");
        assert_eq!(handled(&state, alice()), alices, "each user's own");

        state.apply(peer(5), Intent::Chat("hi".into()), t0);
        assert!(!state.users.contains_key(&peer(5)));
    }

    #[test]
    fn a_user_that_joins_again_counts_anew() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(bob, select("ep1"), t0);
        state.apply(bob, Intent::Pause, t0);

        // bob's session ended unnoticed, and the first status of his next
        // one arrives
        state.apply(bob, status("bob", false, None), t0);

        assert_eq!(state.users[&bob].handled, 1);
    }

    fn selected(result: Option<Video>) -> Option<String> {
        result.map(|v| v.as_str().to_string())
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

    #[test]
    fn select_next_without_selection_selects_nothing() {
        let old = playlist(&["a", "b", "c"]);

        assert!(select_next(&old, None, &playlist(&["a", "b"])).is_none());
    }

    #[test]
    fn select_next_picks_the_video_now_at_the_index_of_the_removed_one() {
        let old = playlist(&["a", "b", "c", "d", "e"]);
        let c = Video::from("c");

        let result = select_next(&old, Some(&c), &playlist(&["a", "d", "e"]));

        assert_eq!(selected(result), Some("e".into()));
    }
}
