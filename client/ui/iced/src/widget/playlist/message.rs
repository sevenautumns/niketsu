use enum_dispatch::enum_dispatch;
use iced::Task;
use niketsu_core::playlist::Video;
use niketsu_core::ui::UiModel;
use tracing::debug;

use super::{FileInteraction, PlaylistWidgetState, VideoIndex};
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

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
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        PlaylistWidgetMessageTrait::handle(self, &mut model.playlist_widget_state, &model.model);
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct DoubleClick {
    pub video: Video,
}

impl PlaylistWidgetMessageTrait for DoubleClick {
    fn handle(self, _state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!(video = ?self.video, "filetable doubleclick");
        model.change_video(self.video)
    }
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub video: Video,
}

impl PlaylistWidgetMessageTrait for Delete {
    fn handle(self, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!(video = ?self.video, "filetable delete file");
        state.delete_video(&self.video);
        model.change_playlist(state.playlist.clone());
    }
}

#[derive(Debug, Clone)]
pub struct Move {
    pub video: Video,
    pub pos: usize,
}

impl PlaylistWidgetMessageTrait for Move {
    fn handle(self, state: &mut PlaylistWidgetState, model: &UiModel) {
        debug!(video = ?self.video, pos = %self.pos, "filetable move file");
        state.move_video(&self.video, self.pos);
        model.change_playlist(state.playlist.clone());
    }
}

#[derive(Debug, Clone)]
pub struct Interaction {
    pub video: Option<VideoIndex>,
    pub interaction: FileInteraction,
}

impl PlaylistWidgetMessageTrait for Interaction {
    fn handle(self, state: &mut PlaylistWidgetState, _: &UiModel) {
        debug!(video = ?self.video, interaction = ?self.interaction);
        state.file_interaction(self.video.clone(), self.interaction.clone());
    }
}
