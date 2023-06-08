use dyn_clone::DynClone;
use log::debug;

use super::{FileInteraction, PlaylistWidgetState};
use crate::core::playlist::PlaylistVideo;
use crate::iced_ui::message::Message;
use crate::iced_ui::UiModel;

pub trait PlaylistWidgetMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, state: &mut PlaylistWidgetState, model: &UiModel);
}

dyn_clone::clone_trait_object!(PlaylistWidgetMessage);

#[derive(Debug, Clone)]
pub struct DoubleClick {
    pub video: PlaylistVideo,
}

impl PlaylistWidgetMessage for DoubleClick {
    fn handle(self: Box<Self>, _state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable doubleclick: {:?}", self.video);
        model.video_change(self.video)
    }
}

impl From<DoubleClick> for Message {
    fn from(value: DoubleClick) -> Self {
        Message::PlaylistWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub video: PlaylistVideo,
}

impl PlaylistWidgetMessage for Delete {
    fn handle(self: Box<Self>, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable delete file: {:?}", self.video);
        state.delete_video(&self.video);
        model.playlist_change(state.playlist.clone());
    }
}

impl From<Delete> for Message {
    fn from(value: Delete) -> Self {
        Message::PlaylistWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Move {
    pub video: PlaylistVideo,
    pub pos: usize,
}

impl PlaylistWidgetMessage for Move {
    fn handle(self: Box<Self>, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable move file: {:?}, {}", self.video, self.pos);
        state.move_video(&self.video, self.pos);
        model.playlist_change(state.playlist.clone());
    }
}

impl From<Move> for Message {
    fn from(value: Move) -> Self {
        Message::PlaylistWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Interaction {
    pub video: Option<PlaylistVideo>,
    pub interaction: FileInteraction,
}

impl PlaylistWidgetMessage for Interaction {
    fn handle(self: Box<Self>, state: &mut PlaylistWidgetState, _: &UiModel) {
        debug!(
            "FileTable file interaction: {:?}, {:?}",
            self.video, self.interaction
        );
        state.file_interaction(self.video.clone(), self.interaction.clone());
    }
}

impl From<Interaction> for Message {
    fn from(value: Interaction) -> Self {
        Message::PlaylistWidget(Box::new(value))
    }
}
