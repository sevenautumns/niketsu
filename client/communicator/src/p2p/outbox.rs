//! What one peer sends another in the room: the host to each member, a
//! client to the host.

use std::collections::VecDeque;

use crate::messages::NiketsuMessage;

/// The room's messages to one peer, which reach it in the order sent: one
/// request is on its way at a time, and the next goes out once the peer
/// has answered it. A room state, which only the host sends, is a full
/// snapshot, so a newer one takes the place of one still waiting, at the
/// back of the queue; every other message waits for its turn. A room state
/// carries the playlist until the member has answered one that carried the
/// playlist of its `playlist_version`.
///
/// `R` names a request on its way.
#[derive(Debug)]
pub(crate) struct Outbox<R> {
    on_its_way: Option<Sent<R>>,
    waiting: VecDeque<NiketsuMessage>,
    /// The `playlist_version` of the last playlist the member took.
    playlist: Option<u64>,
}

#[derive(Debug)]
struct Sent<R> {
    request: R,
    /// The `playlist_version` of the playlist the request carries.
    playlist: Option<u64>,
}

impl<R> Default for Outbox<R> {
    fn default() -> Self {
        Self {
            on_its_way: None,
            waiting: VecDeque::new(),
            playlist: None,
        }
    }
}

impl<R: PartialEq> Outbox<R> {
    /// Queues `msg`, and hands the next message to `send` unless one is on
    /// its way.
    pub(crate) fn push(&mut self, msg: NiketsuMessage, send: impl FnOnce(NiketsuMessage) -> R) {
        if matches!(msg, NiketsuMessage::RoomState(_)) {
            self.waiting
                .retain(|waiting| !matches!(waiting, NiketsuMessage::RoomState(_)));
        }
        self.waiting.push_back(msg);
        self.flush(send);
    }

    /// The member took `request`, if it is the one on its way; then the
    /// next message goes to `send`. Whether it was.
    pub(crate) fn answered(&mut self, request: &R, send: impl FnOnce(NiketsuMessage) -> R) -> bool {
        let Some(sent) = self.on_its_way.take_if(|sent| sent.request == *request) else {
            return false;
        };
        if sent.playlist.is_some() {
            self.playlist = sent.playlist;
        }
        self.flush(send);
        true
    }

    /// Whether `request` is the one on its way.
    pub(crate) fn is_on_its_way(&self, request: &R) -> bool {
        self.on_its_way
            .as_ref()
            .is_some_and(|sent| sent.request == *request)
    }

    fn flush(&mut self, send: impl FnOnce(NiketsuMessage) -> R) {
        if self.on_its_way.is_some() {
            return;
        }
        let Some(mut msg) = self.waiting.pop_front() else {
            return;
        };
        let mut playlist = None;
        if let NiketsuMessage::RoomState(state) = &mut msg {
            if self.playlist == Some(state.playlist_version) {
                state.playlist = None;
            } else if state.playlist.is_some() {
                playlist = Some(state.playlist_version);
            }
        }
        let request = send(msg);
        self.on_its_way = Some(Sent { request, playlist });
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::communicator::{ActionMsg, RoomStateMsg, UserMessageMsg};
    use niketsu_core::playlist::Playlist;

    use super::*;

    /// An outbox and what it sent, each request named by its index.
    #[derive(Default)]
    struct Member {
        outbox: Outbox<usize>,
        sent: Vec<NiketsuMessage>,
    }

    impl Member {
        fn push(&mut self, msg: NiketsuMessage) {
            let sent = &mut self.sent;
            self.outbox.push(msg, |msg| {
                sent.push(msg);
                sent.len() - 1
            });
        }

        /// The member answers the request on its way.
        fn answer(&mut self) {
            let request = self.sent.len() - 1;
            let sent = &mut self.sent;
            let answered = self.outbox.answered(&request, |msg| {
                sent.push(msg);
                sent.len() - 1
            });
            assert!(answered, "nothing was on its way");
        }
    }

    fn line(message: &str) -> NiketsuMessage {
        NiketsuMessage::UserMessage(UserMessageMsg {
            actor: "someone".into(),
            message: message.into(),
        })
    }

    fn action(message: &str) -> NiketsuMessage {
        NiketsuMessage::Action(ActionMsg {
            actor: "someone".into(),
            message: message.into(),
        })
    }

    fn state(version: u64) -> NiketsuMessage {
        NiketsuMessage::RoomState(RoomStateMsg {
            version,
            ..Default::default()
        })
    }

    /// A state of `version` with the playlist set at `playlist_version`.
    fn with_playlist(version: u64, playlist_version: u64) -> NiketsuMessage {
        NiketsuMessage::RoomState(RoomStateMsg {
            version,
            playlist_version,
            playlist: Some(Playlist::from_iter([
                format!("ep{playlist_version}").as_str()
            ])),
            ..Default::default()
        })
    }

    fn carries_playlist(msg: &NiketsuMessage) -> bool {
        matches!(msg, NiketsuMessage::RoomState(state) if state.playlist.is_some())
    }

    #[test]
    fn one_message_is_on_its_way_and_the_rest_follow_in_order() {
        let mut member = Member::default();
        member.push(line("one"));
        member.push(action("two"));
        member.push(line("three"));
        assert_eq!(member.sent, [line("one")], "nothing before the answer");

        member.answer();
        assert_eq!(member.sent, [line("one"), action("two")]);
        member.answer();
        assert_eq!(member.sent, [line("one"), action("two"), line("three")]);

        member.answer();
        member.push(line("four"));
        assert_eq!(member.sent.last(), Some(&line("four")), "sent at once");
    }

    #[test]
    fn a_waiting_state_gives_way_to_a_newer_one_and_nothing_else_does() {
        let mut member = Member::default();
        member.push(state(1));
        member.push(state(2));
        member.push(line("between"));
        member.push(action("after the line"));
        member.push(state(3));
        member.push(state(4));
        while member.sent.len() < 4 {
            member.answer();
        }
        assert_eq!(
            member.sent,
            [
                state(1),
                line("between"),
                action("after the line"),
                state(4)
            ],
            "the state on its way stays, the waiting ones give way to the newest, which \
             keeps its place after the lines sent before it"
        );
        member.answer();
        assert_eq!(member.sent.len(), 4, "nothing else waits");
    }

    #[test]
    fn the_playlist_goes_with_every_state_until_the_member_took_it() {
        let mut member = Member::default();
        member.push(with_playlist(1, 1));
        member.push(with_playlist(2, 1));
        member.answer();
        member.push(with_playlist(3, 1));
        member.answer();
        member.push(with_playlist(4, 4));
        member.answer();
        member.push(with_playlist(5, 4));
        member.answer();
        let carried: Vec<bool> = member.sent.iter().map(carries_playlist).collect();
        assert_eq!(
            carried,
            [true, false, false, true, false],
            "a state waiting while the playlist was on its way goes without it, \
             a new playlist goes once"
        );
    }

    #[test]
    fn a_new_member_gets_the_playlist_with_its_first_state() {
        let mut old = Member::default();
        old.push(with_playlist(1, 1));
        old.answer();
        old.push(with_playlist(2, 1));
        let mut new = Member::default();
        new.push(with_playlist(2, 1));
        assert!(!carries_playlist(&old.sent[1]));
        assert!(carries_playlist(&new.sent[0]));
    }

    /// An answer to another request, say of the member's earlier session,
    /// takes nothing off the way: what waits stays waiting, and the
    /// playlist on its way does not count as taken.
    #[test]
    fn only_the_answer_to_the_request_on_its_way_counts() {
        let mut member = Member::default();
        member.push(with_playlist(1, 1));
        member.push(with_playlist(2, 1));
        let earlier = 7;
        assert!(!member.outbox.answered(&earlier, |_| unreachable!()));
        assert!(member.outbox.is_on_its_way(&0));
        assert!(!member.outbox.is_on_its_way(&earlier));
        assert_eq!(member.sent.len(), 1);
        assert_eq!(member.outbox.playlist, None);
    }
}
