use anyhow::Result;
use enum_dispatch::enum_dispatch;
use iced::Command;
use log::{debug, warn};

use super::FileInteraction;
use crate::client::server::{NiketsuPlaylist, NiketsuSelect};
use crate::client::ui::MpvSelect;
use crate::iced_window::message::IcedMessage;
use crate::iced_window::running::message::RunningWindowMessage;
use crate::iced_window::running::RunningWindow;
use crate::iced_window::{MainMessage, MainWindow};
use crate::playlist::PlaylistWidgetState;
use crate::video::Video;

#[enum_dispatch(RunningWindowMessage)]
#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    DoubleClick,
    Delete,
    Move,
    Interaction,
}

impl IcedMessage for PlaylistMessage {
    fn handle(self, win: &mut MainWindow) -> Result<Command<MainMessage>> {
        if let Some(win) = win.get_running() {
            <PlaylistMessage as RunningWindowMessage>::handle(self, win)
        } else {
            warn!("Got RunningWindow message outside RunningWindow");
            Ok(Command::none())
        }
    }
}

#[derive(Debug, Clone)]
pub struct DoubleClick {
    pub video: Video,
}

impl RunningWindowMessage for DoubleClick {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        debug!("FileTable doubleclick: {:?}", self.video);
        let client = win.client();
        client.ws().send(NiketsuSelect {
            filename: self.video.as_str().to_string().into(),
            username: client.user().load().name(),
        })?;
        client.send_ui_message(MpvSelect(self.video).into());
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub video: Video,
}

impl RunningWindowMessage for Delete {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        debug!("FileTable delete file: {:?}", self.video);
        let client = win.client();
        client.playlist().rcu(|p| {
            let mut playlist = PlaylistWidgetState::clone(p);
            playlist.delete_video(&self.video);
            playlist
        });
        let playlist = client
            .playlist()
            .load()
            .videos()
            .drain(..)
            .map(|v| v.as_str().to_string())
            .collect();
        client.ws().send(NiketsuPlaylist {
            playlist,
            username: client.user().load().name(),
        })?;
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct Move {
    pub video: Video,
    pub pos: usize,
}

impl RunningWindowMessage for Move {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        debug!("FileTable move file: {:?}, {}", self.video, self.pos);
        let client = win.client();
        client.playlist().rcu(|p| {
            let mut playlist = PlaylistWidgetState::clone(p);
            playlist.move_video(self.video.clone(), self.pos);
            playlist
        });
        let playlist = client
            .playlist()
            .load()
            .videos()
            .drain(..)
            .map(|v| v.as_str().to_string())
            .collect();
        client.ws().send(NiketsuPlaylist {
            playlist,
            username: client.user().load().name(),
        })?;
        Ok(Command::none())
    }
}

#[derive(Debug, Clone)]
pub struct Interaction {
    pub video: Option<Video>,
    pub interaction: FileInteraction,
}

impl RunningWindowMessage for Interaction {
    fn handle(self, win: &mut RunningWindow) -> Result<Command<MainMessage>> {
        debug!(
            "FileTable file interaction: {:?}, {:?}",
            self.video, self.interaction
        );
        let client = win.client();
        client.playlist().rcu(|p| {
            let mut playlist = PlaylistWidgetState::clone(p);
            playlist.file_interaction(self.video.clone(), self.interaction.clone());
            playlist
        });
        Ok(Command::none())
    }
}
