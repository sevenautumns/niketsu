use niketsu_core::communicator::*;
use serde::{Deserialize, Serialize};

/// The room messages that travel between peers, over gossipsub or as a
/// direct request. File sharing has its own protocol (see `p2p/file_share`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    /// The host's room state, to the room or as a joiner's greeting.
    RoomState(RoomStateMsg),
    /// What the host did on a user's behalf, for the chat.
    Action(ActionMsg),
    /// A client asks the host for a change of the room.
    Intent(IntentMsg),
    UserMessage(UserMessageMsg),
    ForcedRename(ForcedRenameMsg),
    /// A client asks the host to take it into the room. Only the
    /// communicator sees it.
    Join,
}

/// An intent means nothing to the core as it is: it needs the peer it is
/// of, so the host hands it over as a `PeerIntentMsg`.
impl TryFrom<NiketsuMessage> for IncomingMessage {
    type Error = anyhow::Error;

    fn try_from(value: NiketsuMessage) -> anyhow::Result<Self> {
        Ok(match value {
            NiketsuMessage::RoomState(m) => m.into(),
            NiketsuMessage::Action(m) => m.into(),
            NiketsuMessage::UserMessage(m) => m.into(),
            NiketsuMessage::ForcedRename(m) => m.into(),
            m @ (NiketsuMessage::Intent(_) | NiketsuMessage::Join) => {
                anyhow::bail!("Received a message out of place: {m:?}")
            }
        })
    }
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
            DirectMessage::RoomState(msg) => msg.into(),
            DirectMessage::Rename(msg) => msg.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use niketsu_core::playlist::Playlist;
    use niketsu_core::user::UserStatus;

    use super::*;

    fn json_roundtrip(msg: NiketsuMessage) -> NiketsuMessage {
        let bytes = serde_json::to_vec(&msg).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

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
                beat: 7,
                jumps: 1,
                playlist: Playlist::from_iter(["ep1", "ep2"]),
                video: Some("ep1".into()),
                position: Duration::from_millis(1500),
                paused: false,
                speed: 1.5,
                file_loaded: true,
                users: UserStatusListMsg {
                    room_name: "room".into(),
                    users: [UserStatus {
                        name: "bob".into(),
                        ready: true,
                    }]
                    .into(),
                },
            }),
            NiketsuMessage::Intent(intent(Intent::Seek {
                video: "ep1".into(),
                position: Duration::from_millis(1500),
            })),
            NiketsuMessage::Intent(intent(Intent::Speed(1.5))),
            NiketsuMessage::Action(ActionMsg(intent(Intent::Pause))),
            NiketsuMessage::Action(ActionMsg(intent(Intent::Select {
                video: None,
                position: Duration::ZERO,
            }))),
            NiketsuMessage::ForcedRename(ForcedRenameMsg {
                name: "bob_2".into(),
            }),
            NiketsuMessage::Join,
        ];
        for msg in messages {
            assert_eq!(json_roundtrip(msg.clone()), msg);
            assert_eq!(cbor_roundtrip(msg.clone()), msg);
        }
    }

    #[test]
    fn the_wire_tag_names_the_message() {
        let msg = NiketsuMessage::Action(ActionMsg(intent(Intent::Pause)));
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "action");
        assert_eq!(json["actor"], "alice");
    }
}
