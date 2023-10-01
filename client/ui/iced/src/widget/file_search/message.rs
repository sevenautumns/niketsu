use std::time::Instant;

use dyn_clone::DynClone;
use niketsu_core::ui::UiModel;

use super::FileSearchWidgetState;
use crate::message::Message;
use crate::widget::playlist::MAX_DOUBLE_CLICK_INTERVAL;

pub trait FileSearchWidgetMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, model: &UiModel);
}

dyn_clone::clone_trait_object!(FileSearchWidgetMessage);

#[derive(Debug, Clone)]
pub struct Input {
    pub query: String,
}

impl FileSearchWidgetMessage for Input {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, model: &UiModel) {
        state.query = self.query.clone();
        state.search = Some(model.file_database.get_inner_arc().fuzzy_search(self.query))
    }
}

impl From<Input> for Message {
    fn from(value: Input) -> Self {
        Message::FileSearchWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Activate;

impl FileSearchWidgetMessage for Activate {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, _: &UiModel) {
        state.active = true
    }
}

impl From<Activate> for Message {
    fn from(value: Activate) -> Self {
        Message::FileSearchWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Close;

impl FileSearchWidgetMessage for Close {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, _: &UiModel) {
        state.active = false
    }
}

impl From<Close> for Message {
    fn from(value: Close) -> Self {
        Message::FileSearchWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct Click {
    pub index: usize,
}

impl Click {
    fn double_click(self, state: &mut FileSearchWidgetState, model: &UiModel) {
        if let Some(last) = state.last_click {
            state.last_click = None;
            if MAX_DOUBLE_CLICK_INTERVAL
                .saturating_sub(last.elapsed())
                .is_zero()
            {
                return;
            }
            if let Some(video) = state.results.get(state.cursor_index) {
                let mut playlist = model.playlist.get_inner();
                playlist.push((&video.entry.file_name_arc()).into());
                model.change_playlist(playlist)
            }
        }
    }
}

impl FileSearchWidgetMessage for Click {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, model: &UiModel) {
        if state.cursor_index != self.index || state.last_click.is_none() {
            state.last_click = Some(Instant::now());
            state.cursor_index = self.index;
        } else {
            self.double_click(state, model)
        }
    }
}

impl From<Click> for Message {
    fn from(value: Click) -> Self {
        Message::FileSearchWidget(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct SearchFinished;

impl FileSearchWidgetMessage for SearchFinished {
    fn handle(self: Box<Self>, state: &mut FileSearchWidgetState, _: &UiModel) {
        if let Some(results) = state.search.take().and_then(|mut s| s.poll()) {
            state.results = results.into_iter().take(100).collect()
        }
    }
}

impl From<SearchFinished> for Message {
    fn from(value: SearchFinished) -> Self {
        Message::FileSearchWidget(Box::new(value))
    }
}
