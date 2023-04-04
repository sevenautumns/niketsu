#![allow(non_upper_case_globals)]

use std::{
    ffi::c_void,
    sync::Arc,
    task::{Poll, Waker},
};

use crate::mpv::bindings::*;
use arc_swap::ArcSwapOption;

use super::MpvHandle;

unsafe extern "C" fn on_mpv_event(_: *mut c_void) {
    if let Some(waker) = EVENT_WAKER.load().as_ref() {
        waker.wake_by_ref();
    }
}

static EVENT_WAKER: ArcSwapOption<Waker> = ArcSwapOption::const_empty();

#[derive(Debug, Clone)]
pub enum MpvEvent {
    None(mpv_event),
    Shutdown(mpv_event),
    StartFile(mpv_event, mpv_event_start_file),
    EndFile(mpv_event, mpv_event_end_file),
    FileLoaded(mpv_event),
    Idle(mpv_event),
    Seek(mpv_event),
    PlaybackRestart(mpv_event),
    Unparsed(mpv_event),
}

impl From<mpv_event> for MpvEvent {
    fn from(event: mpv_event) -> Self {
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Self::None(event),
            mpv_event_id::MPV_EVENT_SHUTDOWN => Self::Shutdown(event),
            mpv_event_id::MPV_EVENT_FILE_LOADED => Self::FileLoaded(event),
            mpv_event_id::MPV_EVENT_IDLE => Self::Idle(event),
            mpv_event_id::MPV_EVENT_SEEK => Self::Seek(event),
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => Self::PlaybackRestart(event),
            mpv_event_id::MPV_EVENT_START_FILE => {
                // TODO check if this cast/deref is safe
                let file = unsafe { *(event.data as *mut mpv_event_start_file) };
                Self::StartFile(event, file)
            }
            mpv_event_id::MPV_EVENT_END_FILE => {
                // TODO check if this cast/deref is safe
                let file = unsafe { *(event.data as *mut mpv_event_end_file) };
                Self::EndFile(event, file)
            }
            _ => Self::Unparsed(event),
        }
    }
}

#[derive(Debug)]
pub struct MpvEventPipe {
    mpv: MpvHandle,
}

impl MpvEventPipe {
    pub fn new(mpv: MpvHandle) -> Self {
        unsafe {
            mpv_set_wakeup_callback(mpv.0, Some(on_mpv_event), std::ptr::null_mut());
        }
        Self { mpv }
    }

    fn check_for_event(&mut self) -> MpvEvent {
        unsafe { (*mpv_wait_event(self.mpv.0, 0.0)).into() }
    }
}

impl futures::stream::Stream for MpvEventPipe {
    type Item = MpvEvent;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        EVENT_WAKER.store(Some(Arc::new(cx.waker().clone())));

        match self.get_mut().check_for_event() {
            MpvEvent::None(_) => Poll::Pending,
            e => Poll::Ready(Some(e)),
        }
    }
}
