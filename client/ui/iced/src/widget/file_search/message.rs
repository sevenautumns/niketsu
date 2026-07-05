use std::time::Instant;

use enum_dispatch::enum_dispatch;
use iced::Task;
use iced::keyboard::key::Named;
use niketsu_core::file_database::FileEntry;
use niketsu_core::ui::UiModel;
use niketsu_core::util::FuzzyResult;

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
    KeyInput,
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

/// Runs a fuzzy search on the file database and reports back with a
/// [`SearchFinished`] message once it completes.
fn search_task(query: String, model: &UiModel) -> Task<Message> {
    let search = model
        .file_database
        .get_inner_arc()
        .fuzzy_search(query.clone());
    Task::perform(search, move |results| {
        Message::from(FileSearchWidgetMessage::from(SearchFinished {
            query: query.clone(),
            results,
        }))
    })
}

#[derive(Debug, Clone)]
pub struct Input {
    pub query: String,
}

impl FileSearchWidgetMessageTrait for Input {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        state.query.clone_from(&self.query);
        search_task(self.query, model)
    }
}

#[derive(Debug, Clone)]
pub struct Activate;

impl FileSearchWidgetMessageTrait for Activate {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        state.active = true;
        Task::batch([
            search_task(state.query.clone(), model),
            iced::widget::operation::focus(iced::widget::Id::new("file_search_query")),
        ])
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
        let double_click = !MAX_DOUBLE_CLICK_INTERVAL
            .saturating_sub(state.last_click.map(|i| i.elapsed()).unwrap_or_default())
            .is_zero();

        if state.cursor_index != self.index || state.last_click.is_none() {
            Select::from(self).handle(state, model)
        } else if double_click {
            state.last_click = None;
            Insert::from(self).handle(state, model)
        } else {
            state.last_click = None;
            Task::none()
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
        if let Some(video) = state.results.get(state.cursor_index) {
            let mut playlist = model.playlist.get_inner();
            playlist.push((&video.entry.file_name_arc()).into());
            model.change_playlist(playlist)
        }
        Task::none()
    }
}

/// Keyboard navigation while the file search is open.
#[derive(Debug, Clone)]
pub struct KeyInput {
    pub key: Named,
    pub captured: bool,
}

impl FileSearchWidgetMessageTrait for KeyInput {
    fn handle(self, state: &mut FileSearchWidgetState, model: &UiModel) -> Task<Message> {
        match self.key {
            Named::ArrowUp if !state.results.is_empty() => {
                let index = (state.cursor_index + state.results.len() - 1) % state.results.len();
                Select { index }.handle(state, model)
            }
            Named::ArrowDown if !state.results.is_empty() => {
                let index = (state.cursor_index + 1) % state.results.len();
                Select { index }.handle(state, model)
            }
            // When the query input is focused, Enter already arrives via on_submit.
            Named::Enter if !self.captured => Insert {
                index: state.cursor_index,
            }
            .handle(state, model),
            Named::Escape => Close.handle(state, model),
            _ => Task::none(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchFinished {
    pub query: String,
    pub results: Vec<FuzzyResult<FileEntry>>,
}

impl FileSearchWidgetMessageTrait for SearchFinished {
    fn handle(self, state: &mut FileSearchWidgetState, _: &UiModel) -> Task<Message> {
        // A newer query may have started in the meantime; drop stale results.
        if self.query == state.query {
            state.results = self.results.into_iter().take(100).collect();
            state.cursor_index = state
                .cursor_index
                .checked_rem(state.results.len())
                .unwrap_or_default();
        }
        Task::none()
    }
}
