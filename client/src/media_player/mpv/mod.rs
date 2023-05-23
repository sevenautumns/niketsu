use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use bindings::{
    mpv_command_async, mpv_create, mpv_error, mpv_format, mpv_get_property, mpv_handle,
    mpv_initialize, mpv_observe_property, mpv_set_property, mpv_terminate_destroy,
};
use futures::StreamExt;
use log::error;
use parking_lot::Mutex;
use strum::{AsRefStr, EnumString};
use tokio::sync::mpsc::UnboundedSender as MpscSender;

use self::event::{MpvEventPipe, PropertyValue};
use super::MediaPlayer;
use crate::client::PlayerMessage;
use crate::media_player::mpv::event::ProcesableMpvEvent;
pub mod bindings;
pub mod error;
pub mod event;

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
}

impl Default for MpvStatus {
    fn default() -> Self {
        Self {
            paused: true,
            seeking: false,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mpv {
    status: Arc<Mutex<MpvStatus>>,
    handle: MpvHandle,
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.handle.0) };
        exit(0)
    }
}

impl Mpv {
    fn init(self) -> Result<Self> {
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

    fn post_init(&self) -> Result<()> {
        self.pause()?;
        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::Filename)?;
        self.observe_property(MpvProperty::Speed)
    }

    fn set_ocs(&self, ocs: bool) -> Result<()> {
        self.set_property(MpvProperty::Osc, PropertyValue::Flag(ocs))
    }

    fn set_keep_open(&self, keep_open: bool) -> Result<()> {
        self.set_property(MpvProperty::KeepOpen, PropertyValue::Flag(keep_open))
    }

    fn set_keep_open_pause(&self, keep_open_pause: bool) -> Result<()> {
        self.set_property(
            MpvProperty::KeepOpenPause,
            PropertyValue::Flag(keep_open_pause),
        )
    }

    fn set_idle_mode(&self, idle_mode: bool) -> Result<()> {
        self.set_property(MpvProperty::Idle, PropertyValue::Flag(idle_mode))
    }

    fn set_force_window(&self, force_window: bool) -> Result<()> {
        self.set_property(MpvProperty::ForceWindow, PropertyValue::Flag(force_window))
    }

    fn set_config(&self, config: bool) -> Result<()> {
        self.set_property(MpvProperty::Config, PropertyValue::Flag(config))
    }

    fn set_input_default_bindings(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputDefaultBindings, PropertyValue::Flag(flag))
    }

    fn set_input_vo_keyboard(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputVoKeyboard, PropertyValue::Flag(flag))
    }

    fn set_cache_pause(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::CachePause, PropertyValue::Flag(flag))
    }

    fn set_input_media_keys(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputMediaKeys, PropertyValue::Flag(flag))
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

    fn send_command(&self, cmd: &[&CString]) -> Result<()> {
        let mut cmd_ptr = vec![];
        for c in cmd {
            cmd_ptr.push(c.as_ptr())
        }
        cmd_ptr.push(std::ptr::null());
        let ret = unsafe { mpv_command_async(self.handle.0, 0, cmd_ptr.as_mut_ptr()) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
    }

    pub(super) fn eof_reached(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::EofReached)
    }
}

impl MediaPlayer for Mpv {
    fn new(client_sender: MpscSender<PlayerMessage>) -> Result<Self> {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let mut event_pipe = MpvEventPipe::new(handle);
        let status = Arc::new(Mutex::new(MpvStatus::default()));
        let mpv = Self { status, handle };
        let event_pipe_mpv = mpv.clone();

        // TODO move this to its own function
        tokio::spawn(async move {
            loop {
                if let Some(event) = event_pipe.next().await {
                    if let Some(event) = event.process(&event_pipe_mpv) {
                        if let Err(err) = client_sender.send(event.into()) {
                            error!("Client sender unexpectedly ended: {err:?}");
                            return;
                        }
                    }
                } else {
                    error!("Mpv event pipeline unexpectedly ended");
                    return;
                }
            }
        });

        mpv.init()
    }

    fn pause(&self) -> Result<()> {
        self.status.lock().paused = true;
        self.set_property(MpvProperty::Pause, PropertyValue::Flag(true))
    }

    fn is_paused(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::Pause)
    }

    fn start(&self) -> Result<()> {
        self.status.lock().paused = false;
        self.set_property(MpvProperty::Pause, PropertyValue::Flag(false))
    }

    fn set_speed(&self, speed: f64) -> Result<()> {
        self.status.lock().speed = speed;
        self.set_property(MpvProperty::Speed, PropertyValue::Double(speed))
    }

    fn get_speed(&self) -> Result<f64> {
        self.get_property_f64(MpvProperty::Speed)
    }

    fn set_position(&self, pos: Duration) -> Result<()> {
        self.status.lock().seeking = true;
        self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        )
    }

    fn get_position(&self) -> Result<Duration> {
        let duration = self.get_property_f64(MpvProperty::PlaybackTime)?;
        Ok(Duration::from_secs_f64(duration))
    }

    fn open(&self, path: String, paused: bool, pos: Duration) -> Result<()> {
        let cmd = MpvCommand::Loadfile.try_into()?;
        let path = CString::new(path)?;
        let replace = CString::new("replace")?;
        let options = CString::new(format!("start={},pause={}", pos.as_secs_f64(), paused))?;

        self.send_command(&[&cmd, &path, &replace, &options])
    }

    fn is_seeking(&self) -> Result<bool> {
        Ok(self.status.lock().seeking)
    }
}
