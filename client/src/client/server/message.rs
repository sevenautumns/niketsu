use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use enum_dispatch::enum_dispatch;
use log::{debug, error, trace, warn};

use super::NiketsuMessage;
use crate::client::message::ClientMessageTrait;
use crate::client::{CoreRunner, LogResult};
use crate::playlist::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::video::{PlayingFile, Video};

#[enum_dispatch(ClientMessageTrait)]
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Received,
    ServerError,
    WsStreamEnded,
    Connected,
    SendFinished,
}

#[derive(Debug, Clone)]
pub struct Received(pub NiketsuMessage);

impl ClientMessageTrait for Received {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        // TODO break down this match
        match self.0 {
            NiketsuMessage::Ping { uuid } => {
                debug!("Socket: received ping {uuid}");
                client.ws.sender().send(NiketsuMessage::Ping { uuid })?;
                Ok(())
            }
            NiketsuMessage::VideoStatus {
                filename,
                position,
                paused,
                speed,
            } => {
                trace!("{filename:?}, {position:?}, {paused:?}, {speed:?}");
                Ok(())
            }
            NiketsuMessage::StatusList { rooms } => {
                debug!("Socket: received rooms: {rooms:?}");
                client.rooms_widget.rcu(|r| {
                    let mut rs = RoomsWidgetState::clone(r);
                    rs.replace_rooms(rooms.clone());
                    rs
                });
                Ok(())
            }
            NiketsuMessage::Pause { username, .. } => {
                debug!("Socket: received pause");
                client.player.pause()?;
                client.messages.push_paused(username);
                Ok(())
            }
            NiketsuMessage::Start { username, .. } => {
                debug!("Socket: received start");
                client.player.start()?;
                client.messages.push_started(username);
                Ok(())
            }
            NiketsuMessage::Seek {
                filename,
                position,
                username,
                paused,
                desync,
                speed,
            } => {
                debug!("Socket: received seek {position:?}");
                if !client.player.is_seeking()? {
                    client
                        .player
                        .load(PlayingFile {
                            video: Video::from_string(filename.clone()),
                            paused,
                            speed,
                            pos: position,
                        })
                        .log();
                    client
                        .messages
                        .push_seek(position, filename, desync, username);
                }
                Ok(())
            }
            NiketsuMessage::Select { filename, username } => {
                debug!("Socket: received select: {filename:?}");
                match filename.clone() {
                    Some(filename) => client
                        .player
                        .load(PlayingFile {
                            video: Video::from_string(filename),
                            paused: true,
                            speed: client.player.get_speed()?,
                            pos: Duration::ZERO,
                        })
                        .log(),
                    None => client.player.unload(),
                }
                client.messages.push_select(filename, username);
                Ok(())
            }
            NiketsuMessage::UserMessage { message, username } => {
                trace!("Socket: received: {username}: {message}");
                client.messages.push_user_chat(message, username);
                Ok(())
            }
            NiketsuMessage::Playlist { playlist, username } => {
                trace!("Socket: received playlist: {username}");
                client.playlist_widget.rcu(|p| {
                    let mut plist = PlaylistWidgetState::clone(p);
                    plist.replace_videos(playlist.clone());
                    plist
                });
                client.messages.push_playlist_changed(username);
                Ok(())
            }
            NiketsuMessage::Status { ready, username } => {
                warn!("Received: {username}: {ready:?}");
                Ok(())
            }
            NiketsuMessage::Join { room, username, .. } => {
                warn!("Received: {room}: {username}");
                Ok(())
            }
            NiketsuMessage::ServerMessage { message, error } => {
                trace!("Socket: received server message: {error}: {message}");
                client.messages.push_server_chat(message, error);
                Ok(())
            }
            NiketsuMessage::PlaybackSpeed { speed, username } => {
                trace!("Socket: received playback speed: {username}, {speed}");
                client.player.set_speed(speed).log();
                client.messages.push_playback_speed(speed, username);
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerError(pub Arc<Error>);

impl ClientMessageTrait for ServerError {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        warn!("Connection Error: {}", self.0);
        client.messages.push_connection_error(self.0.to_string());
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WsStreamEnded;

impl ClientMessageTrait for WsStreamEnded {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        error!("Websocket ended");
        client.messages.push_disconnected();
        client.ws = client.ws.reboot();
        client.ws_sender.store(Arc::new(client.ws.sender().clone()));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Connected;

impl ClientMessageTrait for Connected {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        trace!("Socket: connected");
        client.ws.sender().send(NiketsuMessage::Join {
            password: client.config.password.clone(),
            room: client.config.room.clone(),
            username: client.config.username.clone(),
        })?;
        client.messages.push_connected();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SendFinished(pub Arc<Result<()>>);

impl ClientMessageTrait for SendFinished {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        trace!("{:?}", self.0);
        Ok(())
    }
}
