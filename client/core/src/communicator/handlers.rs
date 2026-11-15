//! What the core does with each message the communicator hands it.

use tracing::trace;

use super::messages::*;
use crate::room::Role;
use crate::ui::PlayerMessage;
use crate::{CoreModel, EventHandler};

impl EventHandler for ConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(is_host = self.is_host, "connected to the room");
        model.connected(self.is_host, self.peer);
        model.ui.is_host(self.is_host);
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl EventHandler for DisconnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(reason = %self.reason, "in no room");
        model.ui.player_message(PlayerMessage::from(self));
        model.disconnected();
    }
}

impl EventHandler for PeerConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, "peer connected");
        model.greet_peer(self.peer);
    }
}

impl EventHandler for PeerDisconnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, "peer disconnected");
        model.peer_left(self.peer);
    }
}

impl EventHandler for PeerIntentMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, intent = ?self.intent, "peer intent");
        model.handle_intent(Some(self.peer), self.intent);
    }
}

impl EventHandler for HostRoundTripMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(rtt = ?self.rtt, "host round trip");
        if let Role::Follower(sync) = &mut model.role {
            sync.delay.push(self.rtt);
        }
    }
}

impl EventHandler for RoomStateMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(version = self.version, "received room state");
        model.take_room_state(self);
    }
}

impl EventHandler for ActionMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(action = ?self, "received");
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl EventHandler for UserMessageMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(user_message = ?self, "received");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl EventHandler for NoticeMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(notice = ?self, "received");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl EventHandler for ForcedRenameMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(name = %self.name, "renamed by the host");
        model.config.username.clone_from(&self.name);
        model.ui.username_change(self.name.clone());
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl EventHandler for StreamEndedMsg {
    fn handle(self, model: &mut CoreModel) {
        model.video_server.stop_server();
    }
}
