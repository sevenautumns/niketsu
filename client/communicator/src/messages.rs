use niketsu_core::communicator::*;
use serde::{Deserialize, Serialize};

/// The room messages, each a frame on the stream between the host and a
/// member (see `p2p/stream`). File sharing has its own protocol (see
/// `p2p/file_share`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    /// The host's room state, to every member.
    RoomState(RoomStateMsg),
    /// A line of the room's chat, from the host to every member but the
    /// author of a chat line.
    Line(LineMsg),
    /// A client asks the host for a change of the room. A field rather
    /// than a newtype: the tag of a message needs a map to go in.
    Intent { intent: Intent },
}

impl From<Intent> for NiketsuMessage {
    fn from(intent: Intent) -> Self {
        Self::Intent { intent }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use libp2p::PeerId;
    use niketsu_core::playlist::Playlist;
    use niketsu_core::user::User;

    use super::*;

    /// The codec of a frame.
    fn cbor_roundtrip(msg: NiketsuMessage) -> NiketsuMessage {
        let bytes = cbor4ii::serde::to_vec(Vec::new(), &msg).unwrap();
        cbor4ii::serde::from_slice(&bytes).unwrap()
    }

    #[test]
    fn room_messages_survive_the_wire() {
        let bob = User {
            name: "bob".into(),
            ready: true,
            loaded: Some("ep1".into()),
            ..Default::default()
        };
        let messages = [
            NiketsuMessage::RoomState(RoomStateMsg {
                jumps: 1,
                playlist: Some(Playlist::from_iter(["ep1", "ep2"])),
                video: Some("ep1".into()),
                position: Duration::from_millis(1500),
                paused: false,
                speed: 1.5,
                users: BTreeMap::from([(PeerId::random(), bob.clone())]),
            }),
            Intent::CachePause.into(),
            Intent::Ended("ep1".into()).into(),
            Intent::Seek {
                video: "ep1".into(),
                position: Duration::from_millis(1500),
            }
            .into(),
            NiketsuMessage::RoomState(RoomStateMsg::default()),
            Intent::Speed(1.5).into(),
            Intent::Playlist(Playlist::from_iter(["ep1"])).into(),
            Intent::Status(bob).into(),
            Intent::Chat("hello".into()).into(),
            NiketsuMessage::Line(LineMsg {
                actor: "alice".into(),
                message: "alice paused playback".into(),
                kind: LineKind::Action,
            }),
            NiketsuMessage::Line(LineMsg {
                actor: "alice".into(),
                message: "hello".into(),
                kind: LineKind::Chat {
                    author: PeerId::random(),
                },
            }),
        ];
        for msg in messages {
            assert_eq!(cbor_roundtrip(msg.clone()), msg);
        }
    }
}
