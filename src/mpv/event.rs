#![allow(non_upper_case_globals)]

use std::ffi::{c_void, CStr, CString};
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Poll, Waker};

use arc_swap::ArcSwapOption;
use log::trace;

use super::{MpvHandle, MpvProperty};
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
    StartFile,
    EndFile,
    PropertyChanged(MpvProperty, PropertyValue),
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
            mpv_event_id::MPV_EVENT_START_FILE => Self::StartFile,
            mpv_event_id::MPV_EVENT_END_FILE => Self::EndFile,
            mpv_event_id::MPV_EVENT_PROPERTY_CHANGE => {
                let name;
                unsafe {
                    let prop = *(event.data as *mut mpv_event_property);
                    match CStr::from_ptr(prop.name).to_str() {
                        Ok(prop) => match MpvProperty::from_str(prop) {
                            Ok(prop) => name = prop,
                            Err(_) => return Self::Unparsed,
                        },
                        Err(_) => return Self::Unparsed,
                    }
                    match PropertyValue::from_ptr(prop.data, prop.format) {
                        Some(v) => Self::PropertyChanged(name, v),
                        None => return Self::Unparsed,
                    }
                }
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

#[derive(Debug, Clone)]
pub enum PropertyValue {
    Double(f64),
    Flag(bool),
    String(CString),
}

impl From<bool> for PropertyValue {
    fn from(value: bool) -> Self {
        PropertyValue::Flag(value)
    }
}

impl From<CString> for PropertyValue {
    fn from(value: CString) -> Self {
        PropertyValue::String(value)
    }
}

impl From<f64> for PropertyValue {
    fn from(value: f64) -> Self {
        PropertyValue::Double(value)
    }
}

impl PropertyValue {
    pub fn as_mut_ptr(&self) -> *mut c_void {
        match self {
            PropertyValue::Double(double) => double as *const f64 as *mut c_void,
            PropertyValue::Flag(flag) => flag as *const bool as *mut c_void,
            PropertyValue::String(string) => string as *const CString as *mut c_void,
        }
    }

    pub unsafe fn from_ptr(data: *mut c_void, typ: mpv_format) -> Option<Self> {
        match typ {
            mpv_format::MPV_FORMAT_STRING => {
                let mut string = CString::default();
                CStr::from_ptr(data as *mut i8).clone_into(&mut string);
                Some(PropertyValue::String(string))
            }
            mpv_format::MPV_FORMAT_FLAG => Some(PropertyValue::Flag(*(data as *mut bool))),
            mpv_format::MPV_FORMAT_DOUBLE => Some(PropertyValue::Double(*(data as *mut f64))),
            _ => None,
        }
    }

    pub fn format(&self) -> mpv_format {
        match self {
            PropertyValue::Double(_) => mpv_format::MPV_FORMAT_DOUBLE,
            PropertyValue::Flag(_) => mpv_format::MPV_FORMAT_FLAG,
            PropertyValue::String(_) => mpv_format::MPV_FORMAT_STRING,
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
        let s = self.get_mut();

        match s.check_for_event() {
            MpvEvent::None => Poll::Pending,
            e => Poll::Ready(Some(e)),
        }
    }
}
