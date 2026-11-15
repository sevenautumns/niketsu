//! The room's state as a plain value, and every rule that changes it. The
//! authority over a room applies what its users ask to this value and
//! publishes it; everyone, the authority included, brings its player and UI
//! in line with it. Nothing here reads a clock or a player: time comes in
//! as a parameter.

use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use tokio::time::Instant;
use tracing::debug;

use super::{RoomName, RoomUsers};
use crate::chunk::SharedFile;
use crate::communicator::{ActionMsg, Intent, IntentMsg};
use crate::playlist::{Playlist, Video};
use crate::user::UserStatus;

/// The actor of what the authority does on the room's behalf rather than a
/// user's.
pub(crate) const HOST_ACTOR: ArcStr = arcstr::literal!("host");

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
    pub users: RoomUsers,
}

impl RoomState {
    /// An empty room with nobody in it.
    pub fn new(now: Instant) -> Self {
        Self {
            room: None,
            playlist: Playlist::default(),
            video: None,
            timeline: Timeline::new(now),
            users: RoomUsers::default(),
        }
    }

    /// Does what a user asks of the room: the user at `peer`, or without it
    /// the authority's own. Returns the chat lines for what was done.
    pub fn apply(&mut self, peer: Option<PeerId>, msg: IntentMsg, now: Instant) -> Vec<ActionMsg> {
        let mut lines: Vec<ActionMsg> = msg.action().into_iter().collect();
        let IntentMsg { actor, intent } = msg;
        let selected = self.video.clone();
        let mut status_changed = false;
        match intent {
            Intent::Pause => {
                self.timeline.pause(now);
                self.users.set_ready(peer, false);
            }
            Intent::CachePause => self.timeline.pause(now),
            Intent::Start => {
                self.timeline.start(now);
                self.users.set_ready(peer, true);
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
                lines.extend(selection_line(&actor, next.clone()));
                self.select(next, Duration::ZERO, now);
            }
            Intent::Playlist(playlist) => {
                let old = std::mem::replace(&mut self.playlist, playlist);
                if let Some(next) = select_next(&old, self.video.as_ref(), &self.playlist) {
                    lines.extend(selection_line(&HOST_ACTOR, Some(next.clone())));
                    self.select(Some(next), Duration::ZERO, now);
                }
            }
            Intent::Status(status) => {
                self.status(peer, status);
                status_changed = true;
            }
        }
        // a new video starts paused, so a ready room starts it
        if (status_changed || self.video != selected) && self.start_if_all_ready(now) {
            let intent = Intent::Start;
            lines.extend(IntentMsg { actor, intent }.action());
        }
        lines
    }

    /// Moves the room to `position` in `video`. Another video than the
    /// selected one becomes the selection and starts paused; an offer is
    /// one of the selected video, so every offer goes with the old one.
    pub(crate) fn select(&mut self, video: Option<Video>, position: Duration, now: Instant) {
        if self.video != video {
            self.video = video;
            self.timeline.pause(now);
            self.users.drop_offers();
        }
        self.timeline.jump(position, now);
    }

    fn video_after(&self, video: &Video) -> Option<Video> {
        let index = self.playlist.find(video)?;
        self.playlist.get(index + 1).cloned()
    }

    /// Takes a user's own status. An offer counts only of the selected
    /// video: a status can arrive after the selection it was sent for has
    /// changed.
    fn status(&mut self, peer: Option<PeerId>, mut status: UserStatus) {
        let video = self.video.as_ref();
        let selected = |offer: &SharedFile| video.is_some_and(|v| offer.name == v.as_str());
        status.offers = status.offers.filter(selected);
        match peer {
            Some(peer) => {
                status.peer = Some(peer);
                self.users.apply_status(status, peer);
            }
            None => self.users.update_local(status),
        }
    }

    /// Playback starts for the whole room once every user is ready and has
    /// the selected video loaded. Returns whether it started.
    fn start_if_all_ready(&mut self, now: Instant) -> bool {
        let start = self.video.is_some()
            && self.timeline.paused()
            && self.users.all_ready(self.video.as_ref());
        if start {
            debug!("All users are ready. Starting playback for the room");
            self.timeline.start(now);
        }
        start
    }
}

/// The line for `actor` selecting `video`.
fn selection_line(actor: &ArcStr, video: Option<Video>) -> Option<ActionMsg> {
    let intent = Intent::Select {
        video,
        position: Duration::ZERO,
    };
    let actor = actor.clone();
    IntentMsg { actor, intent }.action()
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

    const SECOND: Duration = Duration::from_secs(1);

    fn peer(n: u8) -> PeerId {
        PeerId::from_bytes(&[0x00, 4, n, n, n, n]).unwrap()
    }

    fn playlist(videos: &[&str]) -> Playlist {
        videos.iter().copied().collect()
    }

    fn intent(actor: &str, intent: Intent) -> IntentMsg {
        IntentMsg {
            actor: actor.into(),
            intent,
        }
    }

    fn status(name: &str, ready: bool, loaded: Option<&str>) -> Intent {
        Intent::Status(UserStatus {
            name: name.into(),
            ready,
            loaded: loaded.map(Video::from),
            ..Default::default()
        })
    }

    fn select(video: &str) -> Intent {
        Intent::Select {
            video: Some(Video::from(video)),
            position: Duration::ZERO,
        }
    }

    fn lines(lines: Vec<ActionMsg>) -> Vec<String> {
        lines.into_iter().map(|line| line.message).collect()
    }

    /// A room of alice, its authority, and bob, with ep1 to ep3 in the
    /// playlist.
    fn room(now: Instant) -> (RoomState, PeerId) {
        let mut state = RoomState::new(now);
        let bob = peer(2);
        state.users.add_connected(bob);
        state.apply(None, intent("alice", status("alice", false, None)), now);
        state.apply(Some(bob), intent("bob", status("bob", false, None)), now);
        let episodes = Intent::Playlist(playlist(&["ep1", "ep2", "ep3"]));
        state.apply(None, intent("alice", episodes), now);
        (state, bob)
    }

    /// The room with ep1 selected, which alice and bob have loaded.
    fn room_with_ep1(now: Instant) -> (RoomState, PeerId) {
        let (mut state, bob) = room(now);
        state.apply(None, intent("alice", select("ep1")), now);
        let loaded = Some("ep1");
        state.apply(None, intent("alice", status("alice", false, loaded)), now);
        state.apply(Some(bob), intent("bob", status("bob", false, loaded)), now);
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
        let told = state.apply(None, intent("alice", Intent::Speed(f64::NAN)), t0);
        assert!(told.is_empty());
        assert_eq!(state.timeline.speed(), 1.0);
    }

    #[test]
    fn a_users_pause_and_start_are_the_rooms_and_set_their_readiness() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);

        let told = state.apply(Some(bob), intent("bob", Intent::Start), t0);
        assert_eq!(lines(told), ["bob started playback"]);
        assert!(!state.timeline.paused());
        let ready = |state: &RoomState, name: &str| {
            let user = state.users.list().iter().find(|u| u.name == name);
            user.unwrap().ready
        };
        assert!(ready(&state, "bob") && !ready(&state, "alice"));

        state.apply(None, intent("alice", Intent::Start), t0 + SECOND);
        let told = state.apply(Some(bob), intent("bob", Intent::Pause), t0 + 4 * SECOND);
        assert_eq!(lines(told), ["bob paused playback"]);
        assert!(state.timeline.paused());
        assert_eq!(state.timeline.position_at(t0 + 9 * SECOND), 4 * SECOND);
        assert!(!ready(&state, "bob") && ready(&state, "alice"));
    }

    #[test]
    fn a_cache_pause_pauses_the_room_and_leaves_readiness_alone() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        state.apply(None, intent("alice", Intent::Start), t0);
        state.apply(Some(bob), intent("bob", Intent::Start), t0);

        state.apply(Some(bob), intent("bob", Intent::CachePause), t0 + SECOND);

        assert!(state.timeline.paused());
        assert!(state.users.list().iter().all(|u| u.ready));
    }

    #[test]
    fn a_new_selection_starts_paused_at_its_position() {
        let t0 = Instant::now();
        let (mut state, _) = room_with_ep1(t0);
        state.apply(None, intent("alice", Intent::Start), t0);
        let jumps = state.timeline.jumps();

        let told = state.apply(None, intent("alice", select("ep2")), t0 + 5 * SECOND);

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
        state.apply(None, intent("alice", Intent::Start), t0);

        state.apply(None, intent("alice", select("ep1")), t0 + 5 * SECOND);

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

        let told = state.apply(Some(bob), intent("bob", seek), t0);

        assert_eq!(lines(told), ["bob seeked to 300s"]);
        assert_eq!(state.video, Some(Video::from("ep2")));
        assert!(state.timeline.paused());
        assert_eq!(state.timeline.position_at(t0), 300 * SECOND);
    }

    #[test]
    fn the_room_starts_once_everyone_is_ready_and_has_the_video_loaded() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(None, intent("alice", status("alice", true, None)), t0);
        state.apply(Some(bob), intent("bob", status("bob", true, None)), t0);

        let told = state.apply(None, intent("alice", select("ep1")), t0);
        assert_eq!(lines(told), ["alice selected ep1"]);
        assert!(state.timeline.paused(), "nobody has loaded it yet");

        let loaded = status("alice", true, Some("ep1"));
        state.apply(None, intent("alice", loaded), t0);
        assert!(state.timeline.paused(), "bob has not loaded it yet");

        let stale = status("bob", true, Some("ep0"));
        state.apply(Some(bob), intent("bob", stale), t0);
        assert!(state.timeline.paused(), "bob has another video loaded");

        let loaded = status("bob", true, Some("ep1"));
        let told = state.apply(Some(bob), intent("bob", loaded), t0);
        assert_eq!(lines(told), ["bob started playback"]);
        assert!(!state.timeline.paused());

        let again = status("bob", true, Some("ep1"));
        let told = state.apply(Some(bob), intent("bob", again), t0);
        assert!(told.is_empty(), "a room that plays is not started");
    }

    #[test]
    fn an_explicit_start_does_not_wait_for_anyone() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(None, intent("alice", select("ep1")), t0);

        state.apply(Some(bob), intent("bob", Intent::Start), t0);

        assert!(!state.timeline.paused());
    }

    #[test]
    fn a_video_that_ended_for_two_users_moves_the_room_on_once() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        let ended = || Intent::Ended(Video::from("ep1"));

        let told = state.apply(None, intent("alice", ended()), t0);
        assert_eq!(lines(told), ["alice selected ep2"]);
        assert_eq!(state.video, Some(Video::from("ep2")));
        state.timeline.jump(40 * SECOND, t0);
        let jumps = state.timeline.jumps();

        let told = state.apply(Some(bob), intent("bob", ended()), t0);
        assert!(told.is_empty());
        assert_eq!(state.video, Some(Video::from("ep2")));
        assert_eq!(state.timeline.jumps(), jumps, "ep2 is not seeked back");
    }

    #[test]
    fn the_end_of_the_last_video_unselects_it() {
        let t0 = Instant::now();
        let (mut state, _) = room(t0);
        state.apply(None, intent("alice", select("ep3")), t0);

        let ended = Intent::Ended(Video::from("ep3"));
        let told = state.apply(None, intent("alice", ended), t0);

        assert_eq!(lines(told), ["alice unselected video"]);
        assert_eq!(state.video, None);
    }

    #[test]
    fn removing_the_selected_video_selects_its_successor() {
        let t0 = Instant::now();
        let (mut state, bob) = room(t0);
        state.apply(None, intent("alice", select("ep2")), t0);

        let edit = Intent::Playlist(playlist(&["ep1", "ep3"]));
        let told = state.apply(Some(bob), intent("bob", edit), t0);

        assert_eq!(lines(told), ["bob changed playlist", "host selected ep3"]);
        assert_eq!(state.video, Some(Video::from("ep3")));
        assert_eq!(state.playlist, playlist(&["ep1", "ep3"]));
    }

    #[test]
    fn an_offer_counts_only_of_the_selected_video_and_goes_with_it() {
        let t0 = Instant::now();
        let (mut state, bob) = room_with_ep1(t0);
        let offer = |video: &str| {
            Intent::Status(UserStatus {
                name: "bob".into(),
                offers: Some(SharedFile {
                    name: video.into(),
                    size: 1,
                }),
                ..Default::default()
            })
        };
        let offered = |state: &RoomState| {
            let offers = state.users.list().iter().filter_map(|u| u.offers.clone());
            offers.map(|file| file.name.to_string()).collect::<Vec<_>>()
        };

        state.apply(Some(bob), intent("bob", offer("ep2")), t0);
        assert!(offered(&state).is_empty(), "ep2 is not selected");

        state.apply(Some(bob), intent("bob", offer("ep1")), t0);
        assert_eq!(offered(&state), ["ep1"]);

        state.apply(None, intent("alice", select("ep2")), t0);
        assert!(offered(&state).is_empty(), "the offer was one of ep1");
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
