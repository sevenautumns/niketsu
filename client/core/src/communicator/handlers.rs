//! What the core does with each message the communicator hands it.

use arcstr::ArcStr;
use tracing::{debug, trace};

use super::messages::*;
use super::{EndpointInfo, OutgoingMessage};
use crate::player::MediaPlayerTrait;
use crate::playlist::Video;
use crate::room::UserList;
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
        // round trips of an earlier connection say nothing about this one
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

impl EventHandler for PeerStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, status = ?self.status, "peer status");
        model.peer_status(self.peer, self.status);
    }
}

impl EventHandler for HostRoundTripMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(rtt = ?self.rtt, "host round trip");
        model.sync.delay.push(self.rtt);
    }
}

impl EventHandler for VideoStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received video status for reconciliation");

        if model.sync.is_seeking() {
            debug!("can not determine client position during seek");
            return;
        }

        // PlaybackSpeed only announces a change, so the heartbeat is the only
        // place the room's speed is repeated: a peer that joined a session
        // already running at another speed learns it here, and would otherwise
        // keep playing at its own speed and drift out of sync every few
        // seconds. The speed is independent of what is loaded, so it is
        // adopted even while the host has no video.
        model.player.set_speed(self.speed);

        let (Some(pos), Some(_)) = (self.position, self.video) else {
            trace!("video status sent without position or video: unloading video");
            model.player.unload_video();
            return;
        };

        // The host is only a valid play/position reference once it has the file
        // loaded. While it is still loading it reports a stale paused=false and
        // a frozen position from the previous video — following either would
        // start our (possibly faster-loaded) copy and then yank it back every
        // heartbeat. Hold paused and wait until the host is actually playing.
        if !self.file_loaded {
            model.player.pause();
            return;
        }

        //TODO check if current video is not the same as host?
        if let Some(paused) = model.player.is_paused() {
            match (paused, self.paused) {
                (true, false) => model.player.start(),
                (false, true) => model.player.pause(),
                _ => {}
            }
        }

        let pos = model.sync.host_position(pos, self.paused);
        model.player.reconcile(pos)
    }
}

impl EventHandler for UserStatusListMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received user status list");
        model.ui.user_list(UserList::from(self));
    }
}

impl EventHandler for StartMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received start");
        model.player.start();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl EventHandler for PauseMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received pause");
        model.player.pause();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl EventHandler for PlaybackSpeedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received speed change");
        model.player.set_speed(self.speed);
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl EventHandler for SeekMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(seek = ?self, "received");
        model.sync.seek_started();
        let playlist_video = Video::from(self.video.as_str());
        model.playlist.select_playing(&playlist_video);
        model.save_playlist();
        if model
            .player
            .playing_video()
            .is_some_and(|v| playlist_video.eq(&v))
        {
            model.player.set_position(self.position);
        } else {
            model.player.load_video(
                playlist_video.clone(),
                self.position,
                model.database.all_files(),
            );
            model.ui.video_change(Some(playlist_video));
        }
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl EventHandler for SelectMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(select = ?self, "received");
        model.receive_select(self);
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

impl EventHandler for PlaylistMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received playlist");
        model.receive_playlist(self);
    }
}

impl EventHandler for UserStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("username changed by the host");
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

impl EventHandler for HostHandoverMsg {
    fn handle(self, model: &mut CoreModel) {
        let becomes_host = self.new_host == model.config.username;
        model.ui.player_message(PlayerMessage::from(self));
        if becomes_host {
            model.communicator.connect(EndpointInfo {
                addr: model.config.addr(),
                room: model.config.room.clone(),
                password: model.config.password.clone(),
            });
        }
    }
}

impl EventHandler for VideoProviderStoppedMsg {
    fn handle(self, model: &mut CoreModel) {
        model.video_server.stop_server();
    }
}
