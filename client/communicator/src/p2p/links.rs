//! Which connections exist to which peer.
//!
//! A peer is *linked* once a direct connection to it exists: room messages
//! and file sharing only run there (see `DirectOnly`). A relay circuit is
//! the rendezvous for the hole punch that leads to one, and of no use
//! beside one. The swarm cannot answer whether a peer is linked, because
//! its `is_connected` counts circuits too.

use std::collections::{HashMap, HashSet};

use libp2p::PeerId;
use libp2p::swarm::ConnectionId;

#[derive(Debug, Default)]
struct Link {
    circuits: HashSet<ConnectionId>,
    direct: HashSet<ConnectionId>,
}

/// What a new connection means for the link to its peer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// A circuit to a peer that is not linked: a hole punch can start.
    Rendezvous,
    /// A circuit to a peer that is linked already.
    SpareCircuit,
    /// The first direct connection: the peer is linked from here on. Its
    /// `circuits` have served their purpose.
    Linked { circuits: Vec<ConnectionId> },
    /// One more direct connection to a linked peer.
    AnotherDirect,
}

/// What a closed connection means for the link to its peer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Closed {
    /// The last direct connection: the peer is no longer linked.
    Unlinked,
    /// The last circuit to a peer that never got linked.
    RendezvousLost,
    /// The link is what it was.
    Unchanged,
}

#[derive(Debug, Default)]
pub(super) struct Links(HashMap<PeerId, Link>);

impl Links {
    pub(super) fn opened(&mut self, peer: PeerId, id: ConnectionId, relayed: bool) -> Opened {
        let link = self.0.entry(peer).or_default();
        if relayed {
            link.circuits.insert(id);
            return match link.direct.is_empty() {
                true => Opened::Rendezvous,
                false => Opened::SpareCircuit,
            };
        }
        let first = link.direct.is_empty();
        link.direct.insert(id);
        match first {
            true => Opened::Linked {
                circuits: link.circuits.iter().copied().collect(),
            },
            false => Opened::AnotherDirect,
        }
    }

    pub(super) fn closed(&mut self, peer: PeerId, id: ConnectionId) -> Closed {
        let Some(link) = self.0.get_mut(&peer) else {
            return Closed::Unchanged;
        };
        let was_direct = link.direct.remove(&id);
        let was_circuit = link.circuits.remove(&id);
        let closed = match (link.direct.is_empty(), link.circuits.is_empty()) {
            (true, _) if was_direct => Closed::Unlinked,
            (true, true) if was_circuit => Closed::RendezvousLost,
            _ => Closed::Unchanged,
        };
        if link.direct.is_empty() && link.circuits.is_empty() {
            self.0.remove(&peer);
        }
        closed
    }

    pub(super) fn is_linked(&self, peer: &PeerId) -> bool {
        self.0.get(peer).is_some_and(|l| !l.direct.is_empty())
    }

    /// Whether a circuit to `peer` waits for a direct connection.
    pub(super) fn awaits_direct(&self, peer: &PeerId) -> bool {
        self.0
            .get(peer)
            .is_some_and(|l| l.direct.is_empty() && !l.circuits.is_empty())
    }

    /// Whether any connection to `peer` exists.
    pub(super) fn is_connected(&self, peer: &PeerId) -> bool {
        self.0.contains_key(peer)
    }

    /// Whether `id` is a direct connection to `peer`.
    pub(super) fn is_direct(&self, peer: &PeerId, id: ConnectionId) -> bool {
        self.0.get(peer).is_some_and(|l| l.direct.contains(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: usize) -> ConnectionId {
        ConnectionId::new_unchecked(id)
    }

    #[test]
    fn a_punch_links_the_peer_and_retires_its_circuit() {
        let peer = PeerId::random();
        let mut links = Links::default();
        assert_eq!(links.opened(peer, conn(0), true), Opened::Rendezvous);
        assert!(links.awaits_direct(&peer));
        assert_eq!(
            links.opened(peer, conn(1), false),
            Opened::Linked {
                circuits: vec![conn(0)]
            }
        );
        assert!(links.is_linked(&peer));
        assert_eq!(links.closed(peer, conn(0)), Closed::Unchanged);
    }

    #[test]
    fn a_circuit_beside_a_direct_connection_is_spare() {
        let peer = PeerId::random();
        let mut links = Links::default();
        assert_eq!(
            links.opened(peer, conn(0), false),
            Opened::Linked { circuits: vec![] }
        );
        assert_eq!(links.opened(peer, conn(1), true), Opened::SpareCircuit);
    }

    #[test]
    fn a_peer_stays_linked_until_its_last_direct_connection_closes() {
        let peer = PeerId::random();
        let mut links = Links::default();
        links.opened(peer, conn(0), false);
        assert_eq!(links.opened(peer, conn(1), false), Opened::AnotherDirect);
        assert_eq!(links.closed(peer, conn(0)), Closed::Unchanged);
        assert!(links.is_direct(&peer, conn(1)));
        assert_eq!(links.closed(peer, conn(1)), Closed::Unlinked);
        assert!(!links.is_connected(&peer));
    }

    #[test]
    fn an_open_circuit_does_not_keep_a_peer_linked() {
        let peer = PeerId::random();
        let mut links = Links::default();
        links.opened(peer, conn(0), true);
        links.opened(peer, conn(1), false);
        assert_eq!(links.closed(peer, conn(1)), Closed::Unlinked);
        assert!(links.awaits_direct(&peer));
        assert_eq!(links.closed(peer, conn(0)), Closed::RendezvousLost);
    }

    #[test]
    fn a_rendezvous_that_never_led_anywhere_is_lost() {
        let peer = PeerId::random();
        let mut links = Links::default();
        links.opened(peer, conn(0), true);
        assert_eq!(links.closed(peer, conn(0)), Closed::RendezvousLost);
        assert_eq!(links.closed(peer, conn(0)), Closed::Unchanged);
    }
}
