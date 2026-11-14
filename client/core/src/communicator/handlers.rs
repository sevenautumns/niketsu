//! What the core does with each message the communicator hands it.

use arcstr::ArcStr;
use tracing::trace;

use super::OutgoingMessage;
use super::messages::*;
use crate::sync::ClientSync;
use crate::ui::PlayerMessage;
use crate::{CoreModel, EventHandler};

impl EventHandler for ConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(is_host = self.is_host, "connected to the room");
        if self.is_host {
            model.become_host();
        } else {
            model.host = None;
        }
        model.joined_room = Some(model.config.room.clone());
        // round trips and states of an earlier connection say nothing about
        // this one: a new host counts its versions anew
        model.sync = ClientSync::default();
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
        trace!(order = ?self.order(), "received room state");
        model.follow_room(self);
    }
}

impl EventHandler for ActionMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(action = ?self.0, "received");
        model.show_action(&self.0);
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

/// The file this core can answer `request` with and its size, or why it
/// cannot.
fn shared_file(model: &CoreModel, request: &FileRequestMsg) -> Result<(ArcStr, u64), &'static str> {
    let Some(file_name) = model.video_provider.file_name() else {
        return Err("File provider is currently not sharing. Try again later");
    };
    if !file_name.as_str().eq(request.video.as_str()) {
        return Err("File provider is currently providing another file. Try again later");
    }
    let Some(size) = model.video_provider.size() else {
        return Err("File provider has no info on the file. Try again later");
    };
    Ok((file_name, size))
}

impl EventHandler for FileRequestMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(file_request = ?self, "received");
        model.ui.player_message(self.clone().into());
        let answer: OutgoingMessage = match shared_file(model, &self) {
            Ok((file_name, size)) => FileResponseMsg {
                uuid: self.uuid,
                actor: model.config.username.clone(),
                video: file_name.as_str().into(),
                size,
            }
            .into(),
            Err(reason) => FileDeclineMsg {
                uuid: self.uuid,
                reason: reason.into(),
            }
            .into(),
        };
        model.communicator.send(answer);
    }
}

impl EventHandler for FileResponseMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received file response message");
        model.ui.player_message(self.clone().into());
        let video = ArcStr::from(self.video.as_str());
        model.video_server.start_server(video, self.size);
    }
}

impl EventHandler for VideoProviderStoppedMsg {
    fn handle(self, model: &mut CoreModel) {
        model.video_server.stop_server();
    }
}
