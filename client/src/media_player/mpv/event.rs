#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Poll, Waker};

use arc_swap::ArcSwapOption;
use enum_dispatch::enum_dispatch;
use log::trace;

use super::{Mpv, MpvProperty};
use crate::media_player::event::{Exit, Paused, PlaybackEnded, PositionChanged, SpeedChanged, Started};
use crate::media_player::mpv::bindings::*;
use crate::media_player::{MediaPlayer, MediaPlayerEvent, MpvHandle};

unsafe extern "C" fn on_mpv_event(_: *mut c_void) {
    if let Some(waker) = EVENT_WAKER.load().as_ref() {
        waker.wake_by_ref();
    }
}

static EVENT_WAKER: ArcSwapOption<Waker> = ArcSwapOption::const_empty();

#[enum_dispatch]
pub trait ProcesableMpvEvent {
    fn process(self, mpv: &Mpv) -> Option<MediaPlayerEvent>;
}

#[enum_dispatch(ProcesableMpvEvent)]
#[derive(Debug, Clone)]
pub enum MpvEvent {
    None,
    Shutdown,
    PropertyChanged,
    FileLoaded,
    Seek,
    PlaybackRestart,
    Unparsed,
}

#[derive(Debug, Clone, Copy)]
pub struct None;

impl ProcesableMpvEvent for None {
    fn process(self, _: &Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Shutdown;

impl ProcesableMpvEvent for Shutdown {
    fn process(self, _: &Mpv) -> Option<MediaPlayerEvent> {
        Some(Exit.into())
    }
}

#[derive(Debug, Clone)]
pub struct PropertyChanged {
    property: MpvProperty,
    value: PropertyValue,
}

impl PropertyChanged {
    fn process_pause(self, paused: bool, mpv: &Mpv) -> Option<MediaPlayerEvent> {
        let mut status = mpv.status.lock();
        if status.paused == paused {
            return Option::None;
        }
        status.paused = paused;
        if mpv.eof_reached().unwrap_or_default() {
            return Some(PlaybackEnded.into());
        }
        if paused {
            return Some(Paused.into())
        }
        Some(Started.into())
    }

    fn process_speed(self, speed: f64, mpv: &Mpv) -> Option<MediaPlayerEvent> {
        let mut status = mpv.status.lock();
        if status.speed == speed {
            return Option::None;
        }
        status.speed = speed;
        Some(SpeedChanged(speed).into())
    }
}

impl ProcesableMpvEvent for PropertyChanged {
    fn process(self, mpv: &Mpv) -> Option<MediaPlayerEvent> {
        match (self.property, self.value.clone()) {
            (MpvProperty::Pause, PropertyValue::Flag(paused)) => self.process_pause(paused, mpv),
            (MpvProperty::Speed, PropertyValue::Double(speed)) => self.process_speed(speed, mpv),
            _ => Option::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileLoaded;

impl ProcesableMpvEvent for FileLoaded {
    fn process(self, _: &Mpv) -> Option<MediaPlayerEvent> {
        trace!("File loaded");
        Option::None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Seek;

impl ProcesableMpvEvent for Seek {
    fn process(self, mpv: &Mpv) -> Option<MediaPlayerEvent> {
        mpv.status.lock().seeking = true;
        Option::None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlaybackRestart;

impl ProcesableMpvEvent for PlaybackRestart {
    fn process(self, mpv: &Mpv) -> Option<MediaPlayerEvent> {
        let mut status = mpv.status.lock();
        if status.seeking {
            status.seeking = false;
            return Option::None;
        }
        drop(status);
        let pos = mpv.get_position().unwrap_or_default();
        Some(PositionChanged(pos).into())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Unparsed;

impl ProcesableMpvEvent for Unparsed {
    fn process(self, _: &Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

impl From<mpv_event> for MpvEvent {
    fn from(event: mpv_event) -> Self {
        trace!("Mpv event: {event:?}");
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Self::None(None),
            mpv_event_id::MPV_EVENT_SHUTDOWN => Self::Shutdown(Shutdown),
            mpv_event_id::MPV_EVENT_FILE_LOADED => Self::FileLoaded(FileLoaded),
            mpv_event_id::MPV_EVENT_SEEK => Self::Seek(Seek),
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => Self::PlaybackRestart(PlaybackRestart),
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
                        Some(value) => PropertyChanged {
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
            mpv_format::MPV_FORMAT_FLAG => Some(PropertyValue::Flag(*(data as *mut bool))),
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
            MpvEvent::None(_) => Poll::Pending,
            e => Poll::Ready(Some(e)),
        }
    }
}
