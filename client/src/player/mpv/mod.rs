use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::process::exit;
use std::time::Duration;

use anyhow::{bail, Result};
use async_trait::async_trait;
use strum::{AsRefStr, EnumString};

use self::bindings::*;
use self::event::{MpvEventPipe, PropertyValue};
use crate::client::LogResult;
use crate::core::player::{MediaPlayer, MediaPlayerEvent, Video};

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

impl TryFrom<MpvCommand> for CString {
    type Error = anyhow::Error;

    fn try_from(cmd: MpvCommand) -> Result<Self> {
        Ok(CString::new(cmd.as_ref())?)
    }
}

#[derive(Debug, Clone)]
pub struct MpvStatus {
    paused: bool,
    seeking: bool,
    speed: f64,
    file: Option<Video>,
}

impl Default for MpvStatus {
    fn default() -> Self {
        Self {
            paused: true,
            seeking: false,
            speed: 1.0,
            file: None,
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
        // TODO implement TryFrom for mpv_error
        if mpv_error::MPV_ERROR_SUCCESS != ret {
            bail!("Mpv Error: {ret}");
        }
        Ok(())
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
            Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
        }
    }

    fn observe_property(&self, prop: MpvProperty) -> Result<()> {
        let format = prop.format();
        let prop: CString = prop.try_into()?;
        let ret = unsafe { mpv_observe_property(self.handle.0, 0, prop.as_ptr(), format) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
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

    fn build_cmd(cmd: &[&CString]) -> Vec<*const i8> {
        let mut cmd_ptr = vec![];
        for c in cmd {
            cmd_ptr.push(c.as_ptr())
        }
        cmd_ptr.push(std::ptr::null());
        cmd_ptr
    }

    fn send_command(&self, cmd: &[&CString]) -> Result<()> {
        let mut cmd = Self::build_cmd(cmd);
        let ret = unsafe { mpv_command_async(self.handle.0, 0, cmd.as_mut_ptr()) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
    }

    fn eof_reached(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::EofReached)
    }
}

#[async_trait]
impl MediaPlayer for Mpv {
    // TODO double check
    fn start(&mut self) {
        self.status.paused = false;
        self.set_property(MpvProperty::Pause, false.into()).log()
    }

    // TODO double check
    fn pause(&mut self) {
        self.status.paused = true;
        self.set_property(MpvProperty::Pause, true.into()).log()
    }

    // TODO double check
    fn is_paused(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::Pause)
    }

    // TODO double check
    fn set_speed(&mut self, speed: f64) {
        self.status.speed = speed;
        self.set_property(MpvProperty::Speed, PropertyValue::Double(speed))
            .log()
    }

    // TODO double check
    fn get_speed(&self) -> f64 {
        self.get_property_f64(MpvProperty::Speed).unwrap_or(1.0)
    }

    // TODO double check
    fn set_position(&mut self, pos: Duration) {
        self.status.seeking = true;
        self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        )
        .log()
    }

    // TODO double check
    fn get_position(&mut self) -> Duration {
        let duration = self
            .get_property_f64(MpvProperty::PlaybackTime)
            .unwrap_or_default();
        Duration::from_secs_f64(duration)
    }

    // TODO double check
    fn load_video(&mut self, video: Video) {
        // let cmd = MpvCommand::Loadfile.try_into()?;
        // let path = CString::new(video)?;
        // let replace = CString::new("replace")?;
        // let start = pos.as_secs_f64();
        // let options = CString::new(format!("start={start},pause={paused}"))?;

        // self.send_command(&[&cmd, &path, &replace, &options])
    }

    fn unload_video(&mut self) {
        todo!()
    }

    fn playing_video(&self) -> Option<Video> {
        self.status.file.clone()
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        todo!()
    }
}
