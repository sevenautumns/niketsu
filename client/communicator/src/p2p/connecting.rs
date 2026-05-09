use anyhow::{Context, Result, bail};
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, gossipsub, identify};
use niketsu_core::playlist::file::PlaylistBrowser;
use niketsu_core::room::RoomName;
use tracing::{debug, info};

use super::{
    Behaviour, BehaviourEvent, Handler, TransportBehaviourEvent,
    auth::AuthEvent,
    client::ClientCommunicationHandler,
    host::HostCommunicationHandler,
};
use crate::messages::NiketsuMessage;

struct PeerInfo {
    relay: PeerId,
    host: Option<PeerId>,
}

pub(crate) struct ConnectingHandler {
    swarm: Swarm<Behaviour>,
    relay_addr: Multiaddr,
    room: RoomName,
    password: String,
    topic: gossipsub::IdentTopic,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
}

impl ConnectingHandler {
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
        topic: gossipsub::IdentTopic,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        Self {
            swarm,
            relay_addr,
            room,
            password,
            topic,
            core_receiver,
            message_sender,
        }
    }

    pub(crate) async fn run(mut self) -> Result<Handler> {
        self.swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        self.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        self.swarm.listen_on("/ip6/::/udp/0/quic-v1".parse()?)?;
        self.swarm.listen_on("/ip6/::/tcp/0".parse()?)?;

        info!("Dialing relay");
        self.swarm
            .dial(self.relay_addr.clone())
            .context("Failed to dial relay")?;

        let room2 = self.room.clone();
        let playlist_task =
            tokio::task::spawn(async move { PlaylistBrowser::get_first(&room2).await });

        let peer_info = self.connect().await?;
        let local_peer_id = *self.swarm.local_peer_id();

        let relay_with_peer = self
            .relay_addr
            .clone()
            .with_p2p(peer_info.relay)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let host = match peer_info.host {
            Some(host_peer_id) => {
                info!(%host_peer_id, "Dialing host via relay circuit");
                let peer_addr = relay_with_peer
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(host_peer_id));
                self.swarm.dial(peer_addr.clone())?;
                self.swarm
                    .behaviour_mut()
                    .file_share
                    .kademlia
                    .add_address(&host_peer_id, peer_addr);
                host_peer_id
            }
            None => {
                info!(%local_peer_id, "Listening on relay circuit as host");
                self.swarm
                    .listen_on(relay_with_peer.with(Protocol::P2pCircuit))
                    .context("Failed to listen on relay circuit")?;
                local_peer_id
            }
        };

        self.swarm
            .behaviour_mut()
            .messaging
            .gossipsub
            .subscribe(&self.topic)?;

        info!(%local_peer_id, "Connection setup complete");

        if host == local_peer_id {
            let playlist = playlist_task.await.ok().flatten().unwrap_or_default();
            Ok(Handler::Host(HostCommunicationHandler::new(
                self.swarm,
                self.topic,
                host,
                self.relay_addr,
                self.core_receiver,
                self.message_sender,
                self.room,
                playlist,
            )))
        } else {
            Ok(Handler::Client(ClientCommunicationHandler::new(
                self.swarm,
                self.topic,
                host,
                self.relay_addr,
                self.core_receiver,
                self.message_sender,
            )))
        }
    }

    async fn connect(&mut self) -> Result<PeerInfo> {
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Identify(identify::Event::Received {
                        peer_id,
                        info: identify::Info { observed_addr, .. },
                        ..
                    }),
                )) => {
                    debug!(%observed_addr, %peer_id, "Relay identified");
                    self.swarm.behaviour_mut().transport.auth.initiate(
                        peer_id,
                        self.room.clone(),
                        self.password.clone(),
                    );
                }
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Auth(AuthEvent::Complete { relay, host }),
                )) => return Ok(PeerInfo { relay, host }),
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Auth(AuthEvent::Failed(err)),
                )) => return Err(err),
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    bail!("Connection error during setup: {error}")
                }
                event => debug!(?event, "Event during connection setup"),
            }
        }
    }
}
