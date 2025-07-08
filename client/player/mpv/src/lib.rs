use std::ffi::{CStr, CString, c_void};
use std::mem::MaybeUninit;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use niketsu_core::file_database::{FilePathSearch, FileStore};
use niketsu_core::log;
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait};
use niketsu_core::playlist::Video;
use strum::{AsRefStr, EnumString};
use tracing::debug;

use self::bindings::*;
use self::event::{MpvEventPipe, MpvEventTrait, PropertyValue};

mod bindings;
mod error;
mod event;

#[derive(Debug, Clone, Copy)]
pub struct MpvHandle(pub *mut mpv_handle);

unsafe impl Send for MpvHandle {}

unsafe impl Sync for MpvHandle {}

const CACHE_THRESHOLD: Duration = Duration::from_secs(20);

#[derive(Debug, EnumString, AsRefStr, Clone, Copy)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvProperty {
    PlaybackTime,
    Osc,
    Pause,
    Speed,
    Filename,
    Duration,
    KeepOpen,
    CachePause,
    ForceWindow,
    Idle,
    EofReached,
    DemuxerCacheDuration,
    Config,
    InputDefaultBindings,
    InputVoKeyboard,
    InputMediaKeys,
    PausedForCache,
    CachePauseWait,
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
            MpvProperty::EofReached => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputDefaultBindings => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputVoKeyboard => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputMediaKeys => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::CachePause => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Speed => mpv_format::MPV_FORMAT_DOUBLE,
            MpvProperty::DemuxerCacheDuration => mpv_format::MPV_FORMAT_DOUBLE,
            MpvProperty::Duration => mpv_format::MPV_FORMAT_DOUBLE,
            MpvProperty::PausedForCache => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::CachePauseWait => mpv_format::MPV_FORMAT_DOUBLE,
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

#[derive(Debug, Clone, Copy)]
pub enum FileLoadStatus {
    NotLoaded,
    Loading,
    Loaded,
}

#[derive(Debug, Clone)]
pub struct MpvStatus {
    paused: bool,
    seeking: bool,
    speed: f64,
    file: Option<Video>,
    file_load_status: FileLoadStatus,
    load_position: Duration,
}

impl MpvStatus {
    pub fn reset(&mut self) {
        *self = Self {
            speed: self.speed,
            ..Default::default()
        }
    }
}

impl Default for MpvStatus {
    fn default() -> Self {
        Self {
            paused: true,
            seeking: false,
            speed: 1.0,
            file: None,
            file_load_status: FileLoadStatus::NotLoaded,
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
        self.set_config(true)?;
        self.set_settings()?;
        self.init_handle()?;
        self.set_settings()?;
        self.post_init()?;

        Ok(self)
    }

    fn set_settings(&self) -> Result<()> {
        self.set_ocs(true)?;
        self.set_keep_open(false)?;
        self.set_cache_pause(true)?;
        self.set_idle_mode(true)?;
        self.set_force_window(true)?;
        self.set_input_default_bindings(true)?;
        self.set_input_vo_keyboard(true)?;
        self.set_cache_pause_wait(CACHE_THRESHOLD)?;
        self.set_input_media_keys(true)
    }

    fn init_handle(&self) -> Result<()> {
        let ret = unsafe { mpv_initialize(self.handle.0) };
        let ret = TryInto::<mpv_error>::try_into(ret)?;
        ret.ok()
    }

    fn post_init(&mut self) -> Result<()> {
        self.set_property(MpvProperty::Pause, true.into())?;
        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::PausedForCache)?;
        self.observe_property(MpvProperty::Filename)?;
        self.observe_property(MpvProperty::Speed)
    }

    fn set_ocs(&self, ocs: bool) -> Result<()> {
        self.set_property(MpvProperty::Osc, ocs.into())
    }

    fn set_keep_open(&self, keep_open: bool) -> Result<()> {
        self.set_property(MpvProperty::KeepOpen, keep_open.into())
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

    fn set_cache_pause_wait(&self, time: Duration) -> Result<()> {
        self.set_property(
            MpvProperty::CachePauseWait,
            PropertyValue::Double(time.as_secs_f64()),
        )
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
            mpv_error::try_from(ret)?.ok()
        }
    }

    fn observe_property(&self, prop: MpvProperty) -> Result<()> {
        let format = prop.format();
        let prop: CString = prop.try_into()?;
        let ret = unsafe { mpv_observe_property(self.handle.0, 0, prop.as_ptr(), format) };
        mpv_error::try_from(ret)?.ok()
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
            mpv_error::try_from(ret)?.ok()?;
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
            mpv_error::try_from(ret)?.ok()?;
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
        mpv_error::try_from(ret)?.ok()
    }

    fn replace_video(&mut self, path: CString) {
        let cmd: CString = MpvCommand::Loadfile.into();

        // TODO is this seeking good? apparently we would get a seek otherwise
        self.status.seeking = true;

        let start = self.status.load_position.as_secs_f64();
        let options = format!("start={start}");
        let options = CString::new(options).expect("Got invalid UTF-8");
        let res = self.send_command(&[&cmd, &path, c"replace", c"0", &options]);
        self.status.file_load_status = FileLoadStatus::Loading;
        log!(res)
    }

    fn get_duration(&mut self) -> Option<Duration> {
        self.status.file.as_ref()?;

        self.get_property_f64(MpvProperty::Duration)
            .ok()
            .map(Duration::from_secs_f64)
    }

    fn get_cache(&mut self) -> Option<Duration> {
        self.status.file.as_ref()?;

        self.get_property_f64(MpvProperty::DemuxerCacheDuration)
            .ok()
            .map(Duration::from_secs_f64)
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
        let paused = log!(self.get_property_flag(MpvProperty::Pause), true);
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
            debug!(?pos, "already seeking, ignoring set_position");
            return;
        }
        if self.status.file.is_none() {
            debug!(?pos, "no file is playing, ignoring set_position");
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

        // If the file is not loaded yet, return the expected load position
        if !matches!(self.status.file_load_status, FileLoadStatus::Loaded) {
            return Some(self.status.load_position);
        }

        self.get_property_f64(MpvProperty::PlaybackTime)
            .ok()
            .map(Duration::from_secs_f64)
    }

    fn cache_available(&mut self) -> bool {
        if self.status.file.as_ref().is_none() {
            return false;
        };
        let Some(cache) = self.get_cache() else {
            return false;
        };
        if cache >= CACHE_THRESHOLD {
            return true;
        }
        let Some(duration) = self.get_duration() else {
            return false;
        };
        let Some(position) = self.get_position() else {
            return false;
        };
        duration <= position + cache + Duration::from_secs(1)
    }

    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore) {
        self.status.paused = true;
        self.status.file_load_status = FileLoadStatus::NotLoaded;
        self.status.file = Some(load.clone());
        self.status.load_position = pos;

        self.maybe_reload_video(db);
    }

    // TODO allow for an unload which sets status.file to None
    // keep one which does not touch status.file
    fn unload_video(&mut self) {
        self.status.file_load_status = FileLoadStatus::NotLoaded;

        let cmd: CString = MpvCommand::Loadfile.into();
        let res = self.send_command(&[&cmd, c"null://", c"replace"]);
        log!(res)
    }

    fn playing_video(&self) -> Option<Video> {
        self.status.file.clone()
    }

    fn maybe_reload_video(&mut self, f: &dyn FilePathSearch) {
        if !matches!(self.status.file_load_status, FileLoadStatus::NotLoaded) {
            return;
        }
        let Some(load) = &self.status.file else {
            self.unload_video();
            return;
        };
        let Some(path) = load.to_path_str(f) else {
            debug!(video = ?load, "get path from video");
            self.unload_video();
            return;
        };

        let path = CString::new(path).expect("got invalid utf-8");
        self.replace_video(path);
    }

    fn video_loaded(&self) -> bool {
        matches!(self.status.file_load_status, FileLoadStatus::Loaded)
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
