//! The relay a room's host serves its clients, the rendezvous for hole
//! punches between them. Only the host serves one: the swarm starts with it
//! toggled off, and connection setup switches it on once this peer turns
//! out to host the room.
//!
//! Connections established before that keep a handler that speaks no relay
//! protocol, which is fine: they are the ones to the public relay, and
//! [`relay::Behaviour`] takes the swarm events of connections it never
//! served (as of libp2p-relay 0.21.1; re-check when upgrading). It learns
//! the host's external addresses from the swarm events after it is
//! switched on, and hands them out with each reservation, so it has to be
//! switched on before the reservation on the public relay confirms the
//! host's address.

use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{PeerId, relay};

use crate::timing::{HOST_CIRCUIT_DURATION, HOST_RESERVATION_DURATION};

/// The host's relay, switched on.
pub(super) fn serve(local_peer_id: PeerId) -> Toggle<relay::Behaviour> {
    // The host's relay circuits (client↔client via
    // `/p2p/HOST/p2p-circuit/p2p/PEER`) are only a hole-punch rendezvous:
    // DirectOnly keeps messaging and file_share off relayed connections, so
    // nothing but identify/DCUtR/ping ever flows here, for no longer than a
    // file request waits for its punch (see HOST_CIRCUIT_DURATION). The byte
    // cap guards against peers that don't play by DirectOnly's rules.
    let config = relay::Config {
        max_reservations: 32,
        max_reservations_per_peer: 1,
        max_circuits: 64,
        max_circuits_per_peer: 4,
        reservation_duration: HOST_RESERVATION_DURATION,
        max_circuit_duration: HOST_CIRCUIT_DURATION,
        max_circuit_bytes: 128 * 1024,
        ..Default::default()
    };
    Some(relay::Behaviour::new(local_peer_id, config)).into()
}
