//! How a peer shows that it belongs to the room.
//!
//! A peer's *member tag* is a digest of the room, its password and the
//! peer's id: whoever knows room and password can compute the tag of any
//! peer, nobody else can. A peer presents its own tag, and the receiver
//! checks it against the peer libp2p authenticated, the remote of a
//! request or the signed source of a broadcast. A tag is worth nothing to
//! anyone but its peer, so it can travel in the clear and be reused.

use libp2p::PeerId;
use niketsu_core::room::RoomName;
use serde::{Deserialize, Serialize};
use sha256::digest;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemberTag(String);

impl MemberTag {
    fn of(room: &str, password: &str, peer: &PeerId) -> Self {
        // Length prefixes keep room and password apart, the prefix keeps the
        // tag apart from the topic and the relay's password digest.
        Self(digest(format!(
            "niketsu-member\0{}:{room}{}:{password}{peer}",
            room.len(),
            password.len()
        )))
    }
}

/// A message and the member tag of the peer that sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tagged<T> {
    pub tag: MemberTag,
    pub msg: T,
}

/// What this peer knows of the room to tag its own messages and check
/// those of others.
#[derive(Debug, Clone)]
pub(crate) struct Membership {
    room: RoomName,
    password: String,
    local: MemberTag,
}

impl Membership {
    pub fn new(room: RoomName, password: String, local: &PeerId) -> Self {
        let local = MemberTag::of(&room, &password, local);
        Self {
            room,
            password,
            local,
        }
    }

    pub fn tag<T>(&self, msg: T) -> Tagged<T> {
        Tagged {
            tag: self.local.clone(),
            msg,
        }
    }

    /// Whether `tag` is the one of `peer` in this room.
    pub fn admits(&self, peer: &PeerId, tag: &MemberTag) -> bool {
        let expected = MemberTag::of(&self.room, &self.password, peer);
        expected.0.as_bytes().ct_eq(tag.0.as_bytes()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn membership(room: &str, password: &str, peer: &PeerId) -> Membership {
        Membership::new(room.into(), password.into(), peer)
    }

    #[test]
    fn a_tag_admits_its_peer_in_its_room() {
        let alice = PeerId::random();
        let tagged = membership("room", "secret", &alice).tag(());
        let host = membership("room", "secret", &PeerId::random());
        assert!(host.admits(&alice, &tagged.tag));
    }

    #[test]
    fn a_tag_admits_no_other_peer() {
        let alice = PeerId::random();
        let tagged = membership("room", "secret", &alice).tag(());
        let host = membership("room", "secret", &PeerId::random());
        assert!(!host.admits(&PeerId::random(), &tagged.tag));
    }

    #[test]
    fn a_tag_admits_its_peer_in_no_other_room() {
        let alice = PeerId::random();
        let tagged = membership("room", "secret", &alice).tag(());
        for (room, password) in [("other", "secret"), ("room", "guess"), ("room|", "secret")] {
            let host = membership(room, password, &PeerId::random());
            assert!(!host.admits(&alice, &tagged.tag), "{room} {password}");
        }
    }

    #[test]
    fn room_and_password_do_not_run_into_each_other() {
        let alice = PeerId::random();
        let tagged = membership("ab", "c", &alice).tag(());
        assert!(!membership("a", "bc", &alice).admits(&alice, &tagged.tag));
    }
}
