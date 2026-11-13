//! How long the communicator waits for what. Values that bound one another
//! are derived from each other here.

use std::time::Duration;

/// The least time between two attempts to connect to the relay.
pub(crate) const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);

/// How long a connection attempt may take from the start until the relay
/// has authenticated this peer: waiting for the listeners, dialing the
/// relay and the auth request.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound on waiting for the listeners before dialing the relay. They
/// normally report within milliseconds.
pub(crate) const LISTENER_PATIENCE: Duration = Duration::from_secs(2);
const _: () = assert!(LISTENER_PATIENCE.as_millis() < CONNECT_TIMEOUT.as_millis());

/// The auth request runs inside the connection attempt, so it gets no more
/// than all of it.
pub(crate) const AUTH_TIMEOUT: Duration = CONNECT_TIMEOUT;

/// How long a client waits for a *direct* connection to the host (via DCUtR
/// hole punching or a mDNS LAN dial) before giving up and reconnecting. The
/// relay circuit only carries hole-punch coordination, so until this
/// succeeds the client is not considered connected.
pub(crate) const DIRECT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a file request may take. It is sent right away and waits for
/// the hole punch to the provider, so this is the punch budget of a file
/// share.
pub(crate) const FILE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a circuit over the host's relay may live. Such a circuit is
/// only the rendezvous for a file share's hole punch, so it only needs to
/// outlast the punch budget.
pub(crate) const HOST_CIRCUIT_DURATION: Duration = FILE_REQUEST_TIMEOUT.saturating_mul(2);

/// How long a direct message may wait for its answer.
pub(crate) const MESSAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a QUIC handshake may take.
pub(crate) const QUIC_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a QUIC connection may stay silent before it is dropped.
pub(crate) const QUIC_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often peers ping each other. Pings also keep QUIC connections from
/// going idle.
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(1);
const _: () = assert!(PING_INTERVAL.as_millis() < QUIC_IDLE_TIMEOUT.as_millis());

/// How long a ping may wait for its answer.
pub(crate) const PING_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the swarm keeps a connection that no protocol needs.
pub(crate) const IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
