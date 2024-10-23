use std::time::Instant;

use enum_dispatch::enum_dispatch;
use iced::Task;
use niketsu_core::ui::UiModel;

use super::FileSearchWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;
use crate::widget::playlist::MAX_DOUBLE_CLICK_INTERVAL;

#[enum_dispatch]
pub trait FileSearchWidgetMessageTrait {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message>;
}

#[enum_dispatch(FileSearchWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum FileSearchWidgetMessage {
    Input,
    Activate,
    Close,
    Click,
    Select,
    Insert,
    SearchFinished,
}

impl MessageHandler for FileSearchWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        FileSearchWidgetMessageTrait::handle(
            self,
            &mut model.file_search_widget_state,
            &model.model,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Input {
    pub query: String,
}

impl FileSearchWidgetMessageTrait for Input {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        state.query.clone_from(&self.query);
        state.search = Some(model.file_database.get_inner_arc().fuzzy_search(self.query));
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct Activate;

impl FileSearchWidgetMessageTrait for Activate {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        state.search = Some(
            model
                .file_database
                .get_inner_arc()
                .fuzzy_search(state.query.clone()),
        );
        state.active = true;
        iced::widget::text_input::focus(iced::widget::text_input::Id::new("file_search_query"))
    }
}

#[derive(Debug, Clone)]
pub struct Close;

impl FileSearchWidgetMessageTrait for Close {
    fn handle(self, state: &mut FileSearchWidgetState, _: &UiModel) -> Task<Message> {
        state.active = false;
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct Click {
    pub index: usize,
}

impl FileSearchWidgetMessageTrait for Click {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        if state.cursor_index != self.index || state.last_click.is_none() {
            Select::from(self).handle(state, model)
        } else {
            Insert::from(self).handle(state, model)
        }
    }
}

impl From<Click> for Insert {
    fn from(value: Click) -> Self {
        Insert { index: value.index }
    }
}

impl From<Click> for Select {
    fn from(value: Click) -> Self {
        Select { index: value.index }
    }
}

#[derive(Debug, Clone)]
pub struct Select {
    pub index: usize,
}

impl FileSearchWidgetMessageTrait for Select {
    fn handle(self, state: &mut FileSearchWidgetState, _: &UiModel) -> Task<Message> {
        state.last_click = Some(Instant::now());
        state.cursor_index = self.index;
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct Insert {
    pub index: usize,
}

impl FileSearchWidgetMessageTrait for Insert {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        if let Some(last) = state.last_click {
            state.last_click = None;
            if MAX_DOUBLE_CLICK_INTERVAL
                .saturating_sub(last.elapsed())
                .is_zero()
            {
                return Task::none();
            }
            if let Some(video) = state.results.get(state.cursor_index) {
                let mut playlist = model.playlist.get_inner();
                playlist.push((&video.entry.file_name_arc()).into());
                model.change_playlist(playlist)
            }
        }
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct SearchFinished;

impl FileSearchWidgetMessageTrait for SearchFinished {
    fn handle(self, state: &mut FileSearchWidgetState, _: &UiModel) -> Task<Message> {
        if let Some(results) = state.search.take().and_then(|mut s| s.poll()) {
            state.results = results.into_iter().take(100).collect();
            state.cursor_index = state
                .cursor_index
                .checked_rem(state.results.len())
                .unwrap_or_default();
        }
        Task::none()
    }
}
