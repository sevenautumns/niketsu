use niketsu_core::communicator::*;
use serde::{Deserialize, Serialize};

/// The room messages, each a direct request between the host and a
/// member. File sharing has its own protocol (see `p2p/file_share`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    /// The host's room state, to every member.
    RoomState(RoomStateMsg),
    /// What the host did on a user's behalf, for the chat.
    Action(ActionMsg),
    /// A client asks the host for a change of the room.
    Intent(IntentMsg),
    /// A line of chat: from a client to the host, and from the host to
    /// every member but its author.
    UserMessage(UserMessageMsg),
    ForcedRename(ForcedRenameMsg),
    /// A client asks the host to take it into the room. Only the
    /// communicator sees it.
    Join,
}

/// `From<$msg> for NiketsuMessage`, one variant per message.
macro_rules! wire_from {
    ($($msg:ty => $variant:ident),* $(,)?) => {
        $(impl From<$msg> for NiketsuMessage {
            fn from(value: $msg) -> Self {
                Self::$variant(value)
            }
        })*
    };
}

wire_from! {
    RoomStateMsg => RoomState,
    ActionMsg => Action,
    IntentMsg => Intent,
    UserMessageMsg => UserMessage,
    ForcedRenameMsg => ForcedRename,
}

impl From<DirectMessage> for NiketsuMessage {
    fn from(value: DirectMessage) -> Self {
        match value {
            DirectMessage::Rename(msg) => msg.into(),
        }
    }
}

impl From<ClientRequest> for NiketsuMessage {
    fn from(value: ClientRequest) -> Self {
        match value {
            ClientRequest::Intent(msg) => msg.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use niketsu_core::playlist::Playlist;
    use niketsu_core::user::UserStatus;

    use super::*;

    /// The codec of direct requests.
    fn cbor_roundtrip(msg: NiketsuMessage) -> NiketsuMessage {
        let bytes = cbor4ii::serde::to_vec(Vec::new(), &msg).unwrap();
        cbor4ii::serde::from_slice(&bytes).unwrap()
    }

    fn intent(intent: Intent) -> IntentMsg {
        IntentMsg {
            actor: "alice".into(),
            intent,
        }
    }

    #[test]
    fn room_messages_survive_the_wire() {
        let messages = [
            NiketsuMessage::RoomState(RoomStateMsg {
                version: 3,
                jumps: 1,
                playlist_version: 2,
                playlist: Some(Playlist::from_iter(["ep1", "ep2"])),
                video: Some("ep1".into()),
                position: Duration::from_millis(1500),
                paused: false,
                speed: 1.5,
                users: UserStatusListMsg {
                    room_name: "room".into(),
                    users: [UserStatus {
                        name: "bob".into(),
                        ready: true,
                        loaded: Some("ep1".into()),
                        ..Default::default()
                    }]
                    .into(),
                },
            }),
            NiketsuMessage::Intent(intent(Intent::CachePause)),
            NiketsuMessage::Intent(intent(Intent::Ended("ep1".into()))),
            NiketsuMessage::Intent(intent(Intent::Seek {
                video: "ep1".into(),
                position: Duration::from_millis(1500),
            })),
            NiketsuMessage::RoomState(RoomStateMsg::default()),
            NiketsuMessage::Intent(intent(Intent::Speed(1.5))),
            NiketsuMessage::Intent(intent(Intent::Playlist(Playlist::from_iter(["ep1"])))),
            NiketsuMessage::Action(ActionMsg {
                actor: "alice".into(),
                message: "alice paused playback".into(),
            }),
            NiketsuMessage::UserMessage(UserMessageMsg {
                actor: "alice".into(),
                message: "hello".into(),
            }),
            NiketsuMessage::ForcedRename(ForcedRenameMsg {
                name: "bob_2".into(),
            }),
            NiketsuMessage::Join,
        ];
        for msg in messages {
            assert_eq!(cbor_roundtrip(msg.clone()), msg);
        }
    }
}
