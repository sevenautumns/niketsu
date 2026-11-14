//! How long the relay waits for what.

use std::time::Duration;

/// How long a circuit between two peers may live. A circuit is only the
/// rendezvous for a hole punch, so it has to outlast the time a client
/// gives a punch and nothing more. The client checks that against its own
/// deadline.
pub const CIRCUIT_DURATION: Duration = Duration::from_secs(60);

/// How long a QUIC handshake may take.
pub(crate) const QUIC_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a QUIC connection may stay silent before it is dropped.
pub(crate) const QUIC_IDLE_TIMEOUT: Duration = Duration::from_secs(3);

/// How often the relay pings its peers. Pings also keep QUIC connections
/// from going idle.
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(2);
const _: () = assert!(PING_INTERVAL.as_millis() < QUIC_IDLE_TIMEOUT.as_millis());

/// How long a ping may wait for its answer.
pub(crate) const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the swarm keeps a connection that no protocol needs.
pub(crate) const IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
