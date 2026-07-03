//! Swarm-level tests for [`DirectOnly`]: three in-process swarms (relay, host,
//! dialer) over the memory transport, mirroring the production layout of an
//! un-wrapped relay client next to `DirectOnly`-wrapped messaging behaviours.
//!
//! The unit tests in [`super::direct_only`] feed hand-crafted `FromSwarm`
//! events into the wrapper, so they encode — rather than verify — what the
//! real swarm does. This test pins those assumptions against live libp2p
//! machinery:
//!
//! - relayed connections are recognized by `P2pCircuit` appearing in the
//!   addresses the swarm passes to `handle_established_*_connection`,
//! - the swarm-global `num_established` counts include the circuits hidden
//!   from the wrapped behaviour (the premise of the count adjustment),
//! - real behaviours — gossipsub's subscription handshake, request-response's
//!   connection bookkeeping and debug assertions — survive a relayed-then-
//!   direct connection lifecycle in both directions, with substreams actually
//!   flowing through the `Either` handler plumbing.

use std::time::Duration;

use futures::StreamExt;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::upgrade::Version;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{Multiaddr, StreamProtocol, Transport, gossipsub, noise, relay, yamux};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use super::direct_only::DirectOnly;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Serialize, Deserialize)]
struct Ping(u32);

#[derive(Debug, Serialize, Deserialize)]
struct Pong(u32);

/// Same shape as the production behaviour: hole-punch transport plumbing
/// (here just the relay client) un-wrapped, application protocols inside
/// [`DirectOnly`].
#[derive(NetworkBehaviour)]
struct TestBehaviour {
    relay_client: relay::client::Behaviour,
    messaging: DirectOnly<Messaging>,
}

#[derive(NetworkBehaviour)]
struct Messaging {
    gossipsub: gossipsub::Behaviour,
    request_response: request_response::cbor::Behaviour<Ping, Pong>,
}

fn client_swarm() -> Swarm<TestBehaviour> {
    libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            Ok(MemoryTransport::default()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(key)?)
                .multiplex(yamux::Config::default()))
        })
        .expect("memory transport")
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .expect("relay client")
        .with_behaviour(|key, relay_client| {
            Ok(TestBehaviour {
                relay_client,
                messaging: DirectOnly::new(Messaging {
                    gossipsub: gossipsub::Behaviour::new(
                        gossipsub::MessageAuthenticity::Signed(key.clone()),
                        gossipsub::Config::default(),
                    )?,
                    request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/direct-only-test/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default(),
                    ),
                }),
            })
        })
        .expect("behaviour")
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build()
}

fn relay_swarm() -> Swarm<relay::Behaviour> {
    libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            Ok(MemoryTransport::default()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(key)?)
                .multiplex(yamux::Config::default()))
        })
        .expect("memory transport")
        .with_behaviour(|key| {
            relay::Behaviour::new(key.public().to_peer_id(), relay::Config::default())
        })
        .expect("behaviour")
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build()
}

/// Starts a listener and waits for its first reported address. For a circuit
/// address this blocks until the relay accepts the reservation.
async fn listen(swarm: &mut Swarm<impl NetworkBehaviour>, addr: Multiaddr) -> Multiaddr {
    let listener = swarm.listen_on(addr).expect("listen_on failed");
    timeout(TIMEOUT, async {
        loop {
            if let SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } = swarm.select_next_some().await
                && listener_id == listener
            {
                return address;
            }
        }
    })
    .await
    .expect("no listen address within timeout")
}

#[tokio::test]
async fn protocols_are_gated_to_direct_connections_on_a_real_swarm() {
    let mut relay = relay_swarm();
    let relay_peer_id = *relay.local_peer_id();
    let relay_addr = listen(&mut relay, "/memory/0".parse().unwrap()).await;
    // The relay advertises its external addresses inside reservation vouchers;
    // without one, clients reject the reservation (`NoAddressesInReservation`).
    relay.add_external_address(relay_addr.clone());
    tokio::spawn(async move {
        loop {
            relay.select_next_some().await;
        }
    });

    let topic = gossipsub::IdentTopic::new("direct-only-test");

    let mut host = client_swarm();
    let host_peer_id = *host.local_peer_id();
    host.behaviour_mut()
        .messaging
        .inner_mut()
        .gossipsub
        .subscribe(&topic)
        .unwrap();
    let host_direct_addr = listen(&mut host, "/memory/0".parse().unwrap()).await;
    let circuit_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    listen(&mut host, circuit_addr.clone()).await;

    let mut dialer = client_swarm();
    let dialer_peer_id = *dialer.local_peer_id();
    dialer
        .behaviour_mut()
        .messaging
        .inner_mut()
        .gossipsub
        .subscribe(&topic)
        .unwrap();
    dialer
        .dial(circuit_addr.with(Protocol::P2p(host_peer_id)))
        .unwrap();

    // Phase 1: only the relay circuit connects the two peers.
    timeout(TIMEOUT, async {
        let (mut dialer_connected, mut host_connected) = (false, false);
        while !(dialer_connected && host_connected) {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = event
                        && peer_id == host_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        dialer_connected = true;
                    }
                }
                event = host.select_next_some() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = event
                        && peer_id == dialer_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        host_connected = true;
                    }
                }
            }
        }
    })
    .await
    .expect("no relayed connection within timeout");

    // The circuit must be invisible to the wrapped behaviours: request-response
    // does not consider the peer connected, and a request parks (the dial it
    // issues aborts on its `Disconnected` condition, which `on_dial_failure`
    // ignores) instead of traveling over the relay.
    assert!(
        !dialer
            .behaviour_mut()
            .messaging
            .inner_mut()
            .request_response
            .is_connected(&host_peer_id)
    );
    dialer
        .behaviour_mut()
        .messaging
        .inner_mut()
        .request_response
        .send_request(&host_peer_id, Ping(7));

    // Gossipsub legitimately reports the relay itself (reachable over a
    // direct connection, speaking no gossipsub) as unsupported; anything else
    // means a protocol ran over the circuit.
    let allowed = |event: &MessagingEvent| {
        matches!(
            event,
            MessagingEvent::Gossipsub(gossipsub::Event::GossipsubNotSupported { peer_id })
                if *peer_id == relay_peer_id
        )
    };
    let quiet = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(quiet);
    loop {
        tokio::select! {
            _ = &mut quiet => break,
            event = dialer.select_next_some() => {
                if let SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(event)) = event
                    && !allowed(&event)
                {
                    panic!("messaging activity over the relay circuit on the dialer: {event:?}");
                }
            }
            event = host.select_next_some() => {
                if let SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(event)) = event
                    && !allowed(&event)
                {
                    panic!("messaging activity over the relay circuit on the host: {event:?}");
                }
            }
        }
    }

    // Phase 2: a direct connection unblocks everything. The parked request
    // drains onto it, and gossipsub runs its subscription handshake — which it
    // only does when `other_established` is 0, i.e. only if `DirectOnly`
    // deducted the hidden circuit from the count.
    dialer
        .dial(host_direct_addr.clone().with(Protocol::P2p(host_peer_id)))
        .unwrap();

    let mut direct_connection = None;
    let mut pong = None;
    let (mut dialer_subscribed, mut host_subscribed) = (false, false);
    timeout(TIMEOUT, async {
        while direct_connection.is_none()
            || pong.is_none()
            || !dialer_subscribed
            || !host_subscribed
        {
            tokio::select! {
                event = dialer.select_next_some() => match event {
                    SwarmEvent::ConnectionEstablished {
                        peer_id, connection_id, endpoint, num_established, ..
                    } if peer_id == host_peer_id => {
                        assert!(!endpoint.is_relayed());
                        // The swarm-global count includes the circuit that
                        // `DirectOnly` hides from the wrapped behaviours.
                        assert_eq!(num_established.get(), 2);
                        direct_connection = Some(connection_id);
                    }
                    SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                        MessagingEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, .. }),
                    )) => {
                        assert_eq!(peer_id, host_peer_id);
                        dialer_subscribed = true;
                    }
                    SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                        MessagingEvent::RequestResponse(request_response::Event::Message {
                            message: request_response::Message::Response { response, .. },
                            ..
                        }),
                    )) => pong = Some(response),
                    _ => {}
                },
                event = host.select_next_some() => match event {
                    SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                        MessagingEvent::RequestResponse(request_response::Event::Message {
                            message: request_response::Message::Request { request, channel, .. },
                            ..
                        }),
                    )) => {
                        host.behaviour_mut()
                            .messaging
                            .inner_mut()
                            .request_response
                            .send_response(channel, Pong(request.0))
                            .unwrap();
                    }
                    SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                        MessagingEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, .. }),
                    )) => {
                        assert_eq!(peer_id, dialer_peer_id);
                        host_subscribed = true;
                    }
                    _ => {}
                },
            }
        }
    })
    .await
    .expect("direct connection did not unblock messaging within timeout");
    assert_eq!(pong.unwrap().0, 7);
    assert!(
        dialer
            .behaviour_mut()
            .messaging
            .inner_mut()
            .request_response
            .is_connected(&host_peer_id)
    );

    // Phase 3: closing the direct connection while the circuit is still open
    // delivers `ConnectionClosed` with a nonzero swarm-global count. The
    // wrapped behaviours must still run their full peer cleanup (adjusted
    // count 0); request-response debug_asserts its bookkeeping against exactly
    // this, so a miscount panics the test here.
    let direct_connection = direct_connection.unwrap();
    assert!(dialer.close_connection(direct_connection));
    timeout(TIMEOUT, async {
        let (mut dialer_closed, mut host_closed) = (false, false);
        while !(dialer_closed && host_closed) {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { connection_id, num_established, .. } = event
                        && connection_id == direct_connection
                    {
                        assert_eq!(num_established, 1, "the circuit should still be open");
                        dialer_closed = true;
                    }
                }
                event = host.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, .. } = event
                        && peer_id == dialer_peer_id
                    {
                        assert!(!endpoint.is_relayed());
                        assert_eq!(num_established, 1, "the circuit should still be open");
                        host_closed = true;
                    }
                }
            }
        }
    })
    .await
    .expect("direct connection did not close within timeout");
    assert!(
        !dialer
            .behaviour_mut()
            .messaging
            .inner_mut()
            .request_response
            .is_connected(&host_peer_id)
    );

    // Phase 4: the circuit closes last. Its `ConnectionClosed` must be
    // withheld from the wrapped behaviours — several assert on a matching
    // established/closed pairing for every connection they are told about.
    dialer.disconnect_peer_id(host_peer_id).unwrap();
    timeout(TIMEOUT, async {
        loop {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, .. } = event
                        && peer_id == host_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        assert_eq!(num_established, 0);
                        break;
                    }
                }
                _ = host.select_next_some() => {}
            }
        }
    })
    .await
    .expect("relay circuit did not close within timeout");
}
