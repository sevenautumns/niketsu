#![allow(non_upper_case_globals)]

use anyhow::{bail, Result};
use std::{
    ffi::{c_void, CStr, CString},
    sync::Arc,
    task::{Poll, Waker},
};

use crate::mpv::bindings::*;
use arc_swap::ArcSwapOption;

use super::MpvHandle;

unsafe extern "C" fn on_mpv_event(_: *mut c_void) {
    if let Some(waker) = EVENT_WAKER.load_full() {
        waker.wake_by_ref();
    }
}

static EVENT_WAKER: ArcSwapOption<Waker> = ArcSwapOption::const_empty();

#[derive(Debug, Clone)]
pub enum MpvEvent {
    None(mpv_event),
    Shutdown(mpv_event),
    LogMessage(mpv_event, mpv_event_log_message),
    GetPropertyReply(mpv_event, MpvEventProperty),
    SetPropertyReply(mpv_event),
    CommandReply(mpv_event),
    StartFile(mpv_event, mpv_event_start_file),
    EndFile(mpv_event, mpv_event_end_file),
    FileLoaded(mpv_event),
    Idle(mpv_event),
    Tick(mpv_event),
    ClientMessage(mpv_event, mpv_event_client_message),
    VideoReconfig(mpv_event),
    AudioReconfig(mpv_event),
    Seek(mpv_event),
    PlaybackRestart(mpv_event),
    PropertyChange(mpv_event, MpvEventProperty),
    QueueOverflow(mpv_event),
    Hook(mpv_event, mpv_event_hook),
}

impl TryFrom<mpv_event> for MpvEvent {
    type Error = anyhow::Error;
    fn try_from(event: mpv_event) -> Result<Self> {
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Ok(Self::None(event)),
            mpv_event_id::MPV_EVENT_SHUTDOWN => Ok(Self::Shutdown(event)),
            mpv_event_id::MPV_EVENT_SET_PROPERTY_REPLY => Ok(Self::SetPropertyReply(event)),
            mpv_event_id::MPV_EVENT_COMMAND_REPLY => Ok(Self::CommandReply(event)),
            mpv_event_id::MPV_EVENT_FILE_LOADED => Ok(Self::FileLoaded(event)),
            mpv_event_id::MPV_EVENT_IDLE => Ok(Self::Idle(event)),
            mpv_event_id::MPV_EVENT_TICK => Ok(Self::Tick(event)),
            mpv_event_id::MPV_EVENT_VIDEO_RECONFIG => Ok(Self::VideoReconfig(event)),
            mpv_event_id::MPV_EVENT_AUDIO_RECONFIG => Ok(Self::AudioReconfig(event)),
            mpv_event_id::MPV_EVENT_SEEK => Ok(Self::Seek(event)),
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => Ok(Self::PlaybackRestart(event)),
            mpv_event_id::MPV_EVENT_QUEUE_OVERFLOW => Ok(Self::QueueOverflow(event)),
            mpv_event_id::MPV_EVENT_LOG_MESSAGE => {
                let message = unsafe { *(event.data as *mut mpv_event_log_message) };
                Ok(Self::LogMessage(event, message))
            }
            mpv_event_id::MPV_EVENT_GET_PROPERTY_REPLY => {
                let prop = unsafe { *(event.data as *mut mpv_event_property) };
                Ok(Self::GetPropertyReply(event, prop.try_into()?))
            }
            mpv_event_id::MPV_EVENT_START_FILE => {
                let file = unsafe { *(event.data as *mut mpv_event_start_file) };
                Ok(Self::StartFile(event, file))
            }
            mpv_event_id::MPV_EVENT_END_FILE => {
                let file = unsafe { *(event.data as *mut mpv_event_end_file) };
                Ok(Self::EndFile(event, file))
            }
            mpv_event_id::MPV_EVENT_CLIENT_MESSAGE => {
                let msg = unsafe { *(event.data as *mut mpv_event_client_message) };
                Ok(Self::ClientMessage(event, msg))
            }
            mpv_event_id::MPV_EVENT_PROPERTY_CHANGE => {
                let prop = unsafe { *(event.data as *mut mpv_event_property) };
                Ok(Self::PropertyChange(event, prop.try_into()?))
            }
            mpv_event_id::MPV_EVENT_HOOK => {
                let hook = unsafe { *(event.data as *mut mpv_event_hook) };
                Ok(Self::Hook(event, hook))
            }
            _ => bail!("Could not parse event: {event:?}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MpvEventProperty {
    None(String),
    String(String, String),
    OsdString(String),
    Flag(String, bool),
    Int64(String, i64),
    Double(String, f64),
    Node(String),
    NodeArray(String),
    NodeMap(String),
    ByteArray(String),
}

impl TryFrom<mpv_event_property> for MpvEventProperty {
    type Error = anyhow::Error;

    fn try_from(prop: mpv_event_property) -> std::result::Result<Self, Self::Error> {
        let name = unsafe {
            CStr::from_ptr(prop.name as *mut _)
                .to_owned()
                .into_string()?
        };
        match prop.format {
            mpv_format::MPV_FORMAT_NONE => Ok(Self::None(name)),
            mpv_format::MPV_FORMAT_OSD_STRING => Ok(Self::OsdString(name)),
            mpv_format::MPV_FORMAT_NODE => Ok(Self::Node(name)),
            mpv_format::MPV_FORMAT_NODE_ARRAY => Ok(Self::NodeArray(name)),
            mpv_format::MPV_FORMAT_NODE_MAP => Ok(Self::NodeMap(name)),
            mpv_format::MPV_FORMAT_BYTE_ARRAY => Ok(Self::ByteArray(name)),
            mpv_format::MPV_FORMAT_STRING => {
                let str = unsafe {
                    CStr::from_ptr(prop.data as *mut _)
                        .to_owned()
                        .into_string()?
                };
                Ok(Self::String(name, str))
            }
            mpv_format::MPV_FORMAT_FLAG => {
                let flag = unsafe { *(prop.data as *mut bool) };
                Ok(Self::Flag(name, flag))
            }
            mpv_format::MPV_FORMAT_INT64 => {
                let int = unsafe { *(prop.data as *mut i64) };
                Ok(Self::Int64(name, int))
            }
            mpv_format::MPV_FORMAT_DOUBLE => {
                let double = unsafe { *(prop.data as *mut f64) };
                Ok(Self::Double(name, double))
            }
            _ => bail!("Could not parse property: {prop:?}"),
        }
    }
}

#[derive(Debug)]
pub struct MpvEventPipe {
    mpv: MpvHandle,
}

impl MpvEventPipe {
    pub fn new(mpv: *mut mpv_handle) -> Self {
        let mpv = MpvHandle(mpv);
        unsafe {
            mpv_set_wakeup_callback(mpv.0, Some(on_mpv_event), std::ptr::null_mut());
        }
        Self { mpv }
    }
}

impl futures::stream::Stream for MpvEventPipe {
    type Item = MpvEvent;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        EVENT_WAKER.store(Some(Arc::new(cx.waker().clone())));

        let event = unsafe { *mpv_wait_event(self.mpv.0, 0.0) };
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Poll::Pending,
            _ => match event.try_into() {
                Ok(e) => Poll::Ready(Some(e)),
                // TODO do not send `None` in this case
                Err(_) => Poll::Ready(None),
            },
        }
    }
}
