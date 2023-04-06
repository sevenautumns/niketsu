#![allow(non_upper_case_globals)]

use std::ffi::c_void;
use std::sync::Arc;
use std::task::{Poll, Waker};

use arc_swap::ArcSwapOption;
use log::trace;

use super::MpvHandle;
use crate::mpv::bindings::*;
use crate::window::MainMessage;

unsafe extern "C" fn on_mpv_event(_: *mut c_void) {
    if let Some(waker) = EVENT_WAKER.load().as_ref() {
        waker.wake_by_ref();
    }
}

static EVENT_WAKER: ArcSwapOption<Waker> = ArcSwapOption::const_empty();

#[derive(Debug, Clone)]
pub enum MpvEvent {
    None,
    Shutdown,
    StartFile(mpv_event_start_file),
    EndFile(mpv_event_end_file),
    FileLoaded,
    Idle,
    Seek,
    PlaybackRestart,
    Unparsed,
}

impl From<mpv_event> for MpvEvent {
    fn from(event: mpv_event) -> Self {
        trace!("Mpv event: {event:?}");
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Self::None,
            mpv_event_id::MPV_EVENT_SHUTDOWN => Self::Shutdown,
            mpv_event_id::MPV_EVENT_FILE_LOADED => Self::FileLoaded,
            mpv_event_id::MPV_EVENT_IDLE => Self::Idle,
            mpv_event_id::MPV_EVENT_SEEK => Self::Seek,
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => Self::PlaybackRestart,
            mpv_event_id::MPV_EVENT_START_FILE => {
                // TODO check if this cast/deref is safe
                let file = unsafe { *(event.data as *mut mpv_event_start_file) };
                Self::StartFile(file)
            }
            mpv_event_id::MPV_EVENT_END_FILE => {
                // TODO check if this cast/deref is safe
                let file = unsafe { *(event.data as *mut mpv_event_end_file) };
                Self::EndFile(file)
            }
            _ => Self::Unparsed,
        }
    }
}

impl From<MpvEvent> for MainMessage {
    fn from(event: MpvEvent) -> Self {
        Self::Mpv(event)
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
        let s = self.get_mut();

        match s.check_for_event() {
            MpvEvent::None => Poll::Pending,
            e => Poll::Ready(Some(e)),
        }
    }
}
