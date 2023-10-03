use enum_dispatch::enum_dispatch;
use iced::Command;
use log::debug;
use niketsu_core::playlist::PlaylistVideo;

use super::{FileInteraction, PlaylistWidgetState};
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;
use crate::UiModel;

#[enum_dispatch]
pub trait PlaylistWidgetMessageTrait {
    fn handle(self, state: &mut PlaylistWidgetState, model: &UiModel);
}

#[enum_dispatch(PlaylistWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum PlaylistWidgetMessage {
    DoubleClick,
    Delete,
    Move,
    Interaction,
}

impl MessageHandler for PlaylistWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        PlaylistWidgetMessageTrait::handle(self, &mut model.playlist_widget_state, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct DoubleClick {
    pub video: PlaylistVideo,
}

impl PlaylistWidgetMessageTrait for DoubleClick {
    fn handle(self, _state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable doubleclick: {:?}", self.video);
        model.change_video(self.video)
    }
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub video: PlaylistVideo,
}

impl PlaylistWidgetMessageTrait for Delete {
    fn handle(self, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable delete file: {:?}", self.video);
        state.delete_video(&self.video);
        model.change_playlist(state.playlist.clone());
    }
}

#[derive(Debug, Clone)]
pub struct Move {
    pub video: PlaylistVideo,
    pub pos: usize,
}

impl PlaylistWidgetMessageTrait for Move {
    fn handle(self, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!("FileTable move file: {:?}, {}", self.video, self.pos);
        state.move_video(&self.video, self.pos);
        model.change_playlist(state.playlist.clone());
    }
}

#[derive(Debug, Clone)]
pub struct Interaction {
    pub video: Option<PlaylistVideo>,
    pub interaction: FileInteraction,
}

impl PlaylistWidgetMessageTrait for Interaction {
    fn handle(self, state: &mut PlaylistWidgetState, _: &UiModel) {
        debug!(
            "FileTable file interaction: {:?}, {:?}",
            self.video, self.interaction
        );
        state.file_interaction(self.video.clone(), self.interaction.clone());
    }
}
