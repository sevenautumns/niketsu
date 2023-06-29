#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::process::exit;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Poll, Waker};

use actix::{Handler, Message};
use arc_swap::ArcSwapOption;
use enum_dispatch::enum_dispatch;
use log::{debug, trace};

use super::actor::MpvActor;
use super::{Mpv, MpvProperty};
use crate::client::server::{NiketsuPause, NiketsuPlaybackSpeed, NiketsuSeek, NiketsuStart};
use crate::file_system::actor::FileDatabaseModel;
use crate::media_player::event::{
    PlayerExit, PlayerPaused, PlayerPlaybackEnded, PlayerPositionChanged, PlayerSpeedChanged,
    PlayerStarted,
};
use crate::media_player::mpv::bindings::*;
use crate::media_player::{MediaPlayer, MediaPlayerEvent, MpvHandle};
use crate::user::control::UserReady;

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
    MpvPlaybackRestart,
    Unparsed,
}

#[derive(Debug, Clone, Copy)]
pub struct MpvNone;

impl MpvEventTrait for MpvNone {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

#[derive(Debug, Clone, Copy, Message)]
#[rtype(result = "()")]
pub struct MpvShutdown;

impl MpvEventTrait for MpvShutdown {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Some(PlayerExit.into())
    }
}

impl Handler<MpvShutdown> for MpvActor {
    type Result = ();

    fn handle(&mut self, _: MpvShutdown, _: &mut Self::Context) -> Self::Result {
        exit(0)
    }
}

#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
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
        if mpv.eof_reached().unwrap_or_default() {
            return Some(PlayerPlaybackEnded.into());
        }
        if paused {
            return Some(PlayerPaused.into());
        }
        Some(PlayerStarted.into())
    }

    fn process_speed(self, speed: f64, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if mpv.status.speed == speed {
            return Option::None;
        }
        mpv.status.speed = speed;
        Some(PlayerSpeedChanged(speed).into())
    }
}

impl MpvEventTrait for MpvPropertyChanged {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        match (self.property, self.value.clone()) {
            (MpvProperty::Pause, PropertyValue::Flag(paused)) => {
                self.process_pause(paused > 0, mpv)
            }
            (MpvProperty::Speed, PropertyValue::Double(speed)) => self.process_speed(speed, mpv),
            _ => Option::None,
        }
    }
}

impl MpvActor {
    fn check_and_process_pause(&mut self, paused: bool) {
        if self.status.paused == paused {
            return;
        }
        if self.eof_reached().unwrap_or_default() {
            return self.process_eof_reached();
        }
        if paused {
            return self.process_paused();
        }
        self.process_started()
    }

    fn process_eof_reached(&mut self) {
        debug!("Mpv process: eof reached");
        self.status.paused = true;
    }

    fn process_paused(&mut self) {
        debug!("Mpv process: pause");
        if let Some(file) = &mut self.file {
            file.paused = true;
            self.user.do_send(UserReady::NotReady);
            self.server.do_send(NiketsuPause::default().into());
        }
    }

    fn process_started(&mut self) {
        debug!("Mpv process: start");
        if let Some(file) = &mut self.file {
            file.paused = false;
            self.user.do_send(UserReady::Ready);
            self.server.do_send(NiketsuStart::default().into());
        }
    }

    fn check_and_process_speed(&mut self, speed: f64) {
        if self.status.speed == speed {
            return;
        }
        self.process_speed_changed(speed);
    }

    fn process_speed_changed(&mut self, speed: f64) {
        debug!("Mpv process: playback speed");
        self.status.speed = speed;
        if let Some(file) = &mut self.file {
            file.speed = speed;
            self.server.do_send(NiketsuPlaybackSpeed::new(speed).into())
        };
    }
}

impl Handler<MpvPropertyChanged> for MpvActor {
    type Result = ();

    fn handle(&mut self, msg: MpvPropertyChanged, _: &mut Self::Context) -> Self::Result {
        match (msg.property, msg.value) {
            (MpvProperty::Pause, PropertyValue::Flag(paused)) => {
                self.check_and_process_pause(paused > 0)
            }
            (MpvProperty::Speed, PropertyValue::Double(speed)) => {
                self.check_and_process_speed(speed)
            }
            _ => {}
        };
    }
}

#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct MpvFileLoaded;

impl MpvEventTrait for MpvFileLoaded {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        trace!("File loaded");
        Option::None
    }
}

impl Handler<MpvFileLoaded> for MpvActor {
    type Result = ();

    fn handle(&mut self, _: MpvFileLoaded, _: &mut Self::Context) -> Self::Result {
        trace!("File loaded");
    }
}

#[derive(Debug, Clone)]
pub struct MpvSeek;

impl MpvEventTrait for MpvSeek {
    fn process(self, _: &mut Mpv) -> Option<MediaPlayerEvent> {
        Option::None
    }
}

#[derive(Debug, Clone, Copy, Message)]
#[rtype(result = "()")]
pub struct MpvPlaybackRestart;

impl MpvEventTrait for MpvPlaybackRestart {
    fn process(self, mpv: &mut Mpv) -> Option<MediaPlayerEvent> {
        if mpv.status.seeking {
            mpv.status.seeking = false;
            return Option::None;
        }
        let pos = mpv.get_position().unwrap_or_default();
        Some(PlayerPositionChanged(pos).into())
    }
}

impl MpvActor {
    fn process_seeked(&mut self) {
        let pos = self.get_position().unwrap_or_default();
        debug!("Mpv process: seek {pos:?}");
        if let Some(playing) = &self.file {
            self.server.do_send(
                NiketsuSeek {
                    filename: playing.video.as_str().to_string(),
                    position: pos,
                    username: Default::default(),
                    paused: playing.paused,
                    desync: false,
                    speed: playing.speed,
                }
                .into(),
            )
        }
    }
}

impl Handler<MpvPlaybackRestart> for MpvActor {
    type Result = ();

    fn handle(&mut self, _: MpvPlaybackRestart, _: &mut Self::Context) -> Self::Result {
        if self.status.seeking {
            self.status.seeking = false;
            return;
        }
        self.process_seeked();
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
        trace!("Mpv event: {event:?}");
        match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => Self::MpvNone(MpvNone),
            mpv_event_id::MPV_EVENT_SHUTDOWN => Self::MpvShutdown(MpvShutdown),
            mpv_event_id::MPV_EVENT_FILE_LOADED => Self::MpvFileLoaded(MpvFileLoaded),
            mpv_event_id::MPV_EVENT_SEEK => Self::MpvSeek(MpvSeek),
            mpv_event_id::MPV_EVENT_PLAYBACK_RESTART => {
                Self::MpvPlaybackRestart(MpvPlaybackRestart)
            }
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
