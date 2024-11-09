#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Poll, Waker};

use arc_swap::ArcSwapOption;
use enum_dispatch::enum_dispatch;
use niketsu_core::player::*;
use tracing::trace;

use super::bindings::mpv_event;
use super::{Mpv, MpvHandle, MpvProperty};
use crate::bindings::*;
use crate::FileLoadStatus;

unsafe extern "C" fn on_mpv_event(_: *mut c_void) {
    if let Some(waker) = EVENT_WAKER.load().as_ref() {
        waker.wake_by_ref();
    }
}

static EVENT_WAKER: ArcSwapOption<Waker> = ArcSwapOption::const_empty();

#[enum_dispatch]
pub trait MpvEventTrait {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent>;
}

#[enum_dispatch(MpvEventTrait)]
#[derive(Debug, Clone)]
pub enum MpvEvent {
    MpvNone,
    MpvShutdown,
    MpvPropertyChanged,
    MpvFileLoaded,
    MpvSeek,
    MpvEndFile,
    Unparsed,
}

#[derive(Debug, Clone, Copy)]
pub struct MpvNone;

impl MpvEventTrait for MpvNone {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MpvShutdown;

impl MpvEventTrait for MpvShutdown {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Some(PlayerExit.into())
    }
}

#[derive(Debug, Clone)]
pub struct MpvPropertyChanged {
    property: MpvProperty,
    value: PropertyValue,
}

impl MpvPropertyChanged {
    fn process_pause(self, paused: bool, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if mpv.status.paused == paused {
            return Option::None;
        }
        mpv.status.paused = paused;
        if paused {
            return Some(PlayerPause.into());
        }
        Some(PlayerStart.into())
    }

    fn process_speed(self, speed: f64, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if mpv.status.speed == speed {
            return Option::None;
        }
        mpv.status.speed = speed;
        Some(PlayerSpeedChange::new(speed).into())
    }

    fn process_paused_for_cache(self, paused: bool, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if !paused {
            return None;
        }
        mpv.status.paused = true;
        mpv.pause();
        Some(PlayerCachePause.into())
    }
}

impl MpvEventTrait for MpvPropertyChanged {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        match (self.property, self.value.clone()) {
            (MpvProperty::Pause, PropertyValue::Flag(paused)) => {
                self.process_pause(paused > 0, mpv)
            }
            (MpvProperty::Speed, PropertyValue::Double(speed)) => self.process_speed(speed, mpv),
            (MpvProperty::PausedForCache, PropertyValue::Flag(paused)) => {
                self.process_paused_for_cache(paused > 0, mpv)
            }
            _ => Option::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MpvFileLoaded;

impl MpvEventTrait for MpvFileLoaded {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        // TODO // a race condition is thinkable where an old file is loaded,
        // TODO // when a new file was already added
        mpv.status.file_load_status = FileLoadStatus::Loaded;
        if mpv.status.paused {
            mpv.pause();
        } else {
            mpv.start();
        }
        trace!("file loaded");
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MpvSeek;

impl MpvEventTrait for MpvSeek {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if mpv.status.seeking {
            mpv.status.seeking = false;
            return Option::None;
        }
        if let Some(pos) = mpv.get_position() {
            return Some(PlayerPositionChange::new(pos).into());
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MpvEndFile;

impl MpvEventTrait for MpvEndFile {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        let file = mpv.status.file.take()?;
        mpv.status.reset();
        Some(PlayerFileEnd(file).into())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Unparsed;

impl MpvEventTrait for Unparsed {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

impl From<mpv_event> for MpvEvent {
    fn from(event: mpv_event) -> Self {
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Self::MpvNone(MpvNone),
            mpv_event_id::MPV_EVENT_SHUTDOWN => Self::MpvShutdown(MpvShutdown),
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => Self::MpvFileLoaded(MpvFileLoaded),
            mpv_event_id::MPV_EVENT_SEEK => Self::MpvSeek(MpvSeek),
            mpv_event_id::MPV_EVENT_END_FILE => unsafe {
                let prop = *(event.data as *mut mpv_event_end_file);
                if matches!(prop.reason, mpv_end_file_reason::MPV_END_FILE_REASON_EOF) {
                    return Self::MpvEndFile(MpvEndFile);
                }
                Self::Unparsed(Unparsed)
            },
            mpv_event_id::MPV_EVENT_PROPERTY_CHANGE => {
                let name;
                unsafe {
                    let prop = *(event.data as *mut mpv_event_property);
                    match CStr::from_ptr(prop.name).to_str() {
                        Ok(prop) => match MpvProperty::from_str(prop) {
                            Ok(prop) => name = prop,
                            Err(_) => return Self::Unparsed(Unparsed),
                        },
                        Err(_) => return Self::Unparsed(Unparsed),
                    }
                    match PropertyValue::from_ptr(prop.data, prop.format) {
                        Some(value) => MpvPropertyChanged {
                            property: name,
                            value,
                        }
                        .into(),
                        Option::None => Self::Unparsed(Unparsed),
                    }
                }
            }
            _ => Self::Unparsed(Unparsed),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PropertyValue {
    Double(f64),
    Flag(i64),
    String(CString),
}

impl From<bool> for PropertyValue {
    fn from(value: bool) -> Self {
        PropertyValue::Flag(value as u8 as i64)
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
            PropertyValue::Flag(flag) => flag as *const i64 as *mut c_void,
            PropertyValue::String(string) => string as *const CString as *mut c_void,
        }
    }

    ///
    /// # Safety
    /// `typ` must represent the type behind the `data` pointer
    pub unsafe fn from_ptr(data: *mut c_void, typ: mpv_format) -> Option<Self> {
        match typ {
            mpv_format::MPV_FORMAT_STRING => {
                let mut string = CString::default();
                CStr::from_ptr(data as *mut c_char).clone_into(&mut string);
                Some(PropertyValue::String(string))
            }
            mpv_format::MPV_FORMAT_FLAG => Some(PropertyValue::Flag(*(data as *mut i64))),
            mpv_format::MPV_FORMAT_DOUBLE => Some(PropertyValue::Double(*(data as *mut f64))),
            _ => Option::None,
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
            MpvEvent::MpvNone(_) => Poll::Pending,
            e => Poll::Ready(Some(e)),
        }
    }
}
