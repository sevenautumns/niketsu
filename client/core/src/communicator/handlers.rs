//! What the core does with each message the communicator hands it.

use tracing::trace;

use super::messages::*;
use crate::sync::ClientSync;
use crate::ui::PlayerMessage;
use crate::{CoreModel, EventHandler};

impl EventHandler for ConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(is_host = self.is_host, "connected to the room");
        if self.is_host {
            model.become_host(self.peer);
        } else {
            model.host = None;
        }
        model.joined_room = Some(model.config.room.clone());
        // round trips and states of an earlier connection say nothing about
        // this one: a new host counts its versions anew
        model.sync = ClientSync::default();
        // the host knows no offer until the status names it
        model.publish_status();
        model.ui.is_host(self.is_host);
        model.ui.player_message(PlayerMessage::from(self));
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
        model.sync.delay.push(self.rtt);
    }
}

impl EventHandler for RoomStateMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(version = self.version, "received room state");
        model.follow_room(self);
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
