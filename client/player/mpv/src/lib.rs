use std::ffi::{c_void, CStr, CString};
use std::mem::MaybeUninit;
use std::process::exit;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use log::debug;
use niketsu_core::file_database::FileStore;
use niketsu_core::log;
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait};
use niketsu_core::playlist::Video;
use strum::{AsRefStr, EnumString};

use self::bindings::*;
use self::event::{MpvEventPipe, MpvEventTrait, PropertyValue};

mod bindings;
mod error;
mod event;

#[derive(Debug, Clone, Copy)]
pub struct MpvHandle(pub *mut mpv_handle);

unsafe impl Send for MpvHandle {}

unsafe impl Sync for MpvHandle {}

#[derive(Debug, EnumString, AsRefStr, Clone, Copy)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvProperty {
    PlaybackTime,
    Osc,
    Pause,
    Speed,
    Filename,
    KeepOpen,
    KeepOpenPause,
    CachePause,
    ForceWindow,
    Idle,
    EofReached,
    Config,
    InputDefaultBindings,
    InputVoKeyboard,
    InputMediaKeys,
}

impl TryFrom<MpvProperty> for CString {
    type Error = anyhow::Error;

    fn try_from(prop: MpvProperty) -> Result<Self> {
        Ok(CString::new(prop.as_ref())?)
    }
}

impl MpvProperty {
    pub fn format(&self) -> mpv_format {
        match self {
            MpvProperty::PlaybackTime => mpv_format::MPV_FORMAT_DOUBLE,
            MpvProperty::Osc => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Pause => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Filename => mpv_format::MPV_FORMAT_STRING,
            MpvProperty::KeepOpen => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::ForceWindow => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Idle => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Config => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::KeepOpenPause => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::EofReached => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputDefaultBindings => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputVoKeyboard => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputMediaKeys => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::CachePause => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Speed => mpv_format::MPV_FORMAT_DOUBLE,
        }
    }
}

#[derive(Debug, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvCommand {
    Loadfile,
    Stop,
    ShowText,
}

impl From<MpvCommand> for CString {
    fn from(cmd: MpvCommand) -> Self {
        CString::new(cmd.as_ref()).expect("Got invalid UTF-8")
    }
}

#[derive(Debug, Clone)]
pub struct MpvStatus {
    paused: bool,
    seeking: bool,
    speed: f64,
    file: Option<Video>,
    file_loaded: bool,
    load_position: Duration,
}

impl Default for MpvStatus {
    fn default() -> Self {
        Self {
            paused: true,
            seeking: false,
            speed: 1.0,
            file: None,
            file_loaded: false,
            load_position: Duration::ZERO,
        }
    }
}

#[derive(Debug)]
pub struct Mpv {
    status: MpvStatus,
    event_pipe: MpvEventPipe,
    handle: MpvHandle,
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.handle.0) };
        exit(0)
    }
}

impl Mpv {
    pub fn new() -> Result<Self> {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let event_pipe = MpvEventPipe::new(handle);
        let status = MpvStatus::default();
        let mpv = Self {
            status,
            handle,
            event_pipe,
        };

        mpv.init()
    }

    fn init(mut self) -> Result<Self> {
        self.pre_init()?;
        self.init_handle()?;
        self.post_init()?;

        Ok(self)
    }

    fn pre_init(&self) -> Result<()> {
        self.set_ocs(true)?;
        self.set_keep_open(true)?;
        self.set_keep_open_pause(true)?;
        self.set_cache_pause(false)?;
        self.set_idle_mode(true)?;
        self.set_force_window(true)?;
        self.set_config(true)?;
        self.set_input_default_bindings(true)?;
        self.set_input_vo_keyboard(true)?;
        self.set_input_media_keys(true)
    }

    fn init_handle(&self) -> Result<()> {
        let ret = unsafe { mpv_initialize(self.handle.0) };
        let ret = TryInto::<mpv_error>::try_into(ret)?;
        ret.try_into()
    }

    fn post_init(&mut self) -> Result<()> {
        self.pause();
        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::Filename)?;
        self.observe_property(MpvProperty::Speed)
    }

    fn set_ocs(&self, ocs: bool) -> Result<()> {
        self.set_property(MpvProperty::Osc, ocs.into())
    }

    fn set_keep_open(&self, keep_open: bool) -> Result<()> {
        self.set_property(MpvProperty::KeepOpen, keep_open.into())
    }

    fn set_keep_open_pause(&self, keep_open_pause: bool) -> Result<()> {
        self.set_property(MpvProperty::KeepOpenPause, keep_open_pause.into())
    }

    fn set_idle_mode(&self, idle_mode: bool) -> Result<()> {
        self.set_property(MpvProperty::Idle, idle_mode.into())
    }

    fn set_force_window(&self, force_window: bool) -> Result<()> {
        self.set_property(MpvProperty::ForceWindow, force_window.into())
    }

    fn set_config(&self, config: bool) -> Result<()> {
        self.set_property(MpvProperty::Config, config.into())
    }

    fn set_input_default_bindings(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputDefaultBindings, flag.into())
    }

    fn set_input_vo_keyboard(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputVoKeyboard, flag.into())
    }

    fn set_cache_pause(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::CachePause, flag.into())
    }

    fn set_input_media_keys(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputMediaKeys, flag.into())
    }

    fn set_property(&self, prop: MpvProperty, value: PropertyValue) -> Result<()> {
        let prop: CString = prop.try_into()?;
        unsafe {
            let ret = mpv_set_property(
                self.handle.0,
                prop.as_ptr(),
                value.format(),
                value.as_mut_ptr(),
            );
            TryInto::<mpv_error>::try_into(ret)?.try_into()
        }
    }

    fn observe_property(&self, prop: MpvProperty) -> Result<()> {
        let format = prop.format();
        let prop: CString = prop.try_into()?;
        let ret = unsafe { mpv_observe_property(self.handle.0, 0, prop.as_ptr(), format) };
        TryInto::<mpv_error>::try_into(ret)?.try_into()
    }

    fn get_property_f64(&self, prop: MpvProperty) -> Result<f64> {
        let prop: CString = prop.try_into()?;
        let mut data: MaybeUninit<f64> = MaybeUninit::uninit();
        unsafe {
            let ret = mpv_get_property(
                self.handle.0,
                prop.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                data.as_mut_ptr() as *mut c_void,
            );
            TryInto::<mpv_error>::try_into(ret)?.try_into()?;
            Ok(data.assume_init())
        }
    }

    fn get_property_flag(&self, prop: MpvProperty) -> Result<bool> {
        let prop: CString = prop.try_into()?;
        let mut data: MaybeUninit<bool> = MaybeUninit::uninit();
        unsafe {
            let ret = mpv_get_property(
                self.handle.0,
                prop.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                data.as_mut_ptr() as *mut c_void,
            );
            TryInto::<mpv_error>::try_into(ret)?.try_into()?;
            Ok(data.assume_init())
        }
    }

    fn build_cmd(cmd: &[&CStr]) -> Vec<*const libc::c_char> {
        let mut cmd_ptr = vec![];
        for c in cmd {
            cmd_ptr.push(c.as_ptr())
        }
        cmd_ptr.push(std::ptr::null());
        cmd_ptr
    }

    fn send_command(&self, cmd: &[&CStr]) -> Result<()> {
        let mut cmd = Self::build_cmd(cmd);
        let ret = unsafe { mpv_command_async(self.handle.0, 0, cmd.as_mut_ptr()) };
        TryInto::<mpv_error>::try_into(ret)?.try_into()
    }

    fn eof_reached(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::EofReached)
    }

    fn replace_video(&mut self, path: CString) {
        let cmd: CString = MpvCommand::Loadfile.into();

        // TODO is this seeking good? apparently we would get a seek otherwise
        self.status.seeking = true;

        let replace = CString::new("replace").expect("Got invalid UTF-8");
        let start = self.status.load_position.as_secs_f64();
        let options = format!("start={start}");
        let options = CString::new(options).expect("Got invalid UTF-8");
        let res = self.send_command(&[&cmd, &path, &replace, &options]);
        self.status.file_loaded = true;
        log!(res)
    }
}

#[async_trait]
impl MediaPlayerTrait for Mpv {
    fn start(&mut self) {
        self.status.paused = false;
        if self.status.file.is_some() {
            log!(self.set_property(MpvProperty::Pause, false.into()))
        }
    }

    fn pause(&mut self) {
        self.status.paused = true;
        if self.status.file.is_some() {
            let res = self.set_property(MpvProperty::Pause, true.into());
            log!(res)
        }
    }

    fn is_paused(&self) -> Option<bool> {
        self.status.file.as_ref()?;
        let paused = log!(
            self.get_property_flag(MpvProperty::Pause),
            Default::default()
        );
        Some(paused)
    }

    fn set_speed(&mut self, speed: f64) {
        self.status.speed = speed;
        let res = self.set_property(MpvProperty::Speed, PropertyValue::Double(speed));
        log!(res)
    }

    fn get_speed(&self) -> f64 {
        let speed = self.get_property_f64(MpvProperty::Speed);
        log!(speed, 1.0)
    }

    fn set_position(&mut self, pos: Duration) {
        if self.status.seeking {
            debug!("Already seeking, ignoring set_position: {pos:?}");
            return;
        }
        if self.status.file.is_none() {
            debug!("No file is playing, ignoring set_position: {pos:?}");
            return;
        }
        self.status.seeking = true;
        let res = self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        );
        log!(res)
    }

    fn get_position(&mut self) -> Option<Duration> {
        self.status.file.as_ref()?;
        self.get_property_f64(MpvProperty::PlaybackTime)
            .ok()
            .map(Duration::from_secs_f64)
    }

    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore) {
        self.status.paused = true;
        self.status.file_loaded = false;
        self.status.file = Some(load.clone());
        self.status.load_position = pos;

        self.maybe_reload_video(db);
    }

    // TODO allow for an unload which sets status.file to None
    // keep one which does not touch status.file
    fn unload_video(&mut self) {
        self.status.file_loaded = false;

        let cmd: CString = MpvCommand::Loadfile.into();
        let path = CString::new("null://").expect("Got invalid UTF-8");
        let replace = CString::new("replace").expect("Got invalid UTF-8");
        let res = self.send_command(&[&cmd, &path, &replace]);
        log!(res)
    }

    fn playing_video(&self) -> Option<Video> {
        self.status.file.clone()
    }

    fn maybe_reload_video(&mut self, db: &FileStore) {
        if self.status.file_loaded {
            return;
        }
        let Some(load) = &self.status.file else {
            self.unload_video();
            return;
        };
        let Some(path) = load.to_path_str(db) else {
            debug!("Get path from: {:?}", load);
            self.unload_video();
            return;
        };

        let path = CString::new(path).expect("Got invalid UTF-8");
        self.replace_video(path);
    }

    fn video_loaded(&self) -> bool {
        self.status.file_loaded
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        loop {
            let Some(event) = self.event_pipe.next().await else {
                continue;
            };
            if let Some(event) = event.process(self) {
                return event;
            }
        }
    }
}

pub trait MpvBool {
    fn into_mpv_bool(self) -> &'static str;
}

impl MpvBool for bool {
    fn into_mpv_bool(self) -> &'static str {
        if self {
            return "yes";
        }
        "no"
    }
}
