use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use enum_dispatch::enum_dispatch;
use log::{debug, error, trace, warn};

use super::ServerMessage;
use crate::client::message::ClientMessage;
use crate::client::server::ServerWebsocket;
use crate::client::{ClientInner, LogResult};
use crate::playlist::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::video::{PlayingFile, Video};

#[enum_dispatch(ClientMessage)]
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Received,
    ServerError,
    WsStreamEnded,
    Connected,
    SendFinished,
}

#[derive(Debug, Clone)]
pub struct Received(pub ServerMessage);

impl ClientMessage for Received {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        // TODO break down this match
        match self.0 {
            ServerMessage::Ping { uuid } => {
                debug!("Socket: received ping {uuid}");
                client.ws.load().send(ServerMessage::Ping { uuid })?;
                Ok(())
            }
            ServerMessage::VideoStatus {
                filename,
                position,
                paused,
                speed,
            } => {
                trace!("{filename:?}, {position:?}, {paused:?}, {speed:?}");
                Ok(())
            }
            ServerMessage::StatusList { rooms } => {
                debug!("Socket: received rooms: {rooms:?}");
                client.rooms_widget.rcu(|r| {
                    let mut rs = RoomsWidgetState::clone(r);
                    rs.replace_rooms(rooms.clone());
                    rs
                });
                Ok(())
            }
            ServerMessage::Pause { username, .. } => {
                debug!("Socket: received pause");
                client.player.pause()?;
                client.messages.push_paused(username);
                Ok(())
            }
            ServerMessage::Start { username, .. } => {
                debug!("Socket: received start");
                client.player.start()?;
                client.messages.push_started(username);
                Ok(())
            }
            ServerMessage::Seek {
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
            ServerMessage::Select { filename, username } => {
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
                    None => client.player.playing_file_mut(|file| *file = None),
                }
                client.messages.push_select(filename, username);
                Ok(())
            }
            ServerMessage::UserMessage { message, username } => {
                trace!("Socket: received: {username}: {message}");
                client.messages.push_user_chat(message, username);
                Ok(())
            }
            ServerMessage::Playlist { playlist, username } => {
                trace!("Socket: received playlist: {username}");
                client.playlist_widget.rcu(|p| {
                    let mut plist = PlaylistWidgetState::clone(p);
                    plist.replace_videos(playlist.clone());
                    plist
                });
                client.messages.push_playlist_changed(username);
                Ok(())
            }
            ServerMessage::Status { ready, username } => {
                warn!("Received: {username}: {ready:?}");
                Ok(())
            }
            ServerMessage::Join { room, username, .. } => {
                warn!("Received: {room}: {username}");
                Ok(())
            }
            ServerMessage::ServerMessage { message, error } => {
                trace!("Socket: received server message: {error}: {message}");
                client.messages.push_server_chat(message, error);
                Ok(())
            }
            ServerMessage::PlaybackSpeed { speed, username } => {
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

impl ClientMessage for ServerError {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        warn!("Connection Error: {}", self.0);
        client.messages.push_connection_error(self.0.to_string());
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WsStreamEnded;

impl ClientMessage for WsStreamEnded {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        error!("Websocket ended");
        client.messages.push_disconnected();
        let ws = client.ws.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            ws.rcu(|w| {
                let ws = ServerWebsocket::clone(w);
                ws.reboot()
            });
        });
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Connected;

impl ClientMessage for Connected {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        trace!("Socket: connected");
        client.ws.load().send(ServerMessage::Join {
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

impl ClientMessage for SendFinished {
    fn handle(self, _: &mut ClientInner) -> Result<()> {
        trace!("{:?}", self.0);
        Ok(())
    }
}
