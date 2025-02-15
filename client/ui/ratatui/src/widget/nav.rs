use delegate::delegate;
use ratatui::widgets::{ListState, ScrollbarState};

use super::ListStateWrapper;

#[derive(Debug, Default, Clone)]
pub struct ListNavigationState {
    list_state: ListStateWrapper,
    list_len: usize,
    vertical_scroll_state: ScrollbarState,
    lower_offset: usize,
    upper_offset: usize,
}

pub struct Selection {
    pub lower: usize,
    pub upper: usize,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.upper.saturating_sub(self.lower)
    }
}

impl ListNavigationState {
    pub fn set_vertical_scroll_state(&mut self) {
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn vertical_scroll_state(&self) -> ScrollbarState {
        self.vertical_scroll_state
    }

    pub fn next(&mut self) {
        self.reset_offset();
        self.list_state.overflowing_next(self.list_len);
        self.set_vertical_scroll_state();
    }

    pub fn previous(&mut self) {
        self.reset_offset();
        self.list_state.overflowing_previous(self.list_len);
        self.set_vertical_scroll_state();
    }

    pub fn jump_next(&mut self, offset: usize) {
        self.list_state.jump_next(offset);
        self.set_vertical_scroll_state();
    }

    pub fn jump_previous(&mut self, offset: usize) {
        self.list_state.limited_jump_previous(offset, self.list_len);
        self.set_vertical_scroll_state();
    }

    pub fn jump_start(&mut self) {
        self.list_state.select(Some(0));
        self.set_vertical_scroll_state();
    }

    pub fn jump_end(&mut self) {
        self.list_state
            .select(Some(self.list_len.saturating_sub(1)));
        self.set_vertical_scroll_state();
    }

    pub fn reset_offset(&mut self) {
        self.lower_offset = 0;
        self.upper_offset = 0;
    }

    pub fn increase_selection_offset(&mut self) {
        if let Some(i) = self.list_state.selected() {
            if self.upper_offset.saturating_add(i) < self.list_len.saturating_sub(1) {
                self.upper_offset += 1;
            }
        };
    }

    pub fn set_list_len(&mut self, list_len: usize) {
        self.list_len = list_len;
    }

    pub fn selection_range(&self) -> Option<Selection> {
        self.selected().map(|index| Selection {
            lower: index,
            upper: index + self.upper_offset,
        })
    }

    delegate! {
        to self.list_state {
            pub fn selected(&self) -> Option<usize>;
            pub fn select(&mut self, index: Option<usize>);
            pub fn inner(&mut self) -> &mut ListState;
            pub fn limit(&mut self, len: usize);
        }
    }
}
