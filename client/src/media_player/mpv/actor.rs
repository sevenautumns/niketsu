use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::time::Duration;

use actix::{Actor, AsyncContext, Context, Recipient, WrapFuture};
use anyhow::{bail, Result};
use futures::StreamExt;
use log::error;

use super::bindings::*;
use super::event::{MpvEvent, MpvEventPipe, PropertyValue};
use super::{MpvCommand, MpvHandle, MpvProperty, MpvStatus};
use crate::client::server::NiketsuMessage;
use crate::file_system::actor::FileDatabaseData;
use crate::user::control::UserReady;
use crate::video::PlayingFile;

#[derive(Debug)]
pub struct MpvActor {
    pub(super) status: MpvStatus,
    pub(super) event_pipe: Option<MpvEventPipe>,
    pub(super) handle: MpvHandle,
    pub(super) file: Option<PlayingFile>,
    pub(super) file_loaded: bool,
    pub(super) db: FileDatabaseData,
    pub(super) server: Recipient<NiketsuMessage>,
    pub(super) user: Recipient<UserReady>,
}

impl Actor for MpvActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        let mut event_pipe = self
            .event_pipe
            .take()
            .expect("event_pipe should not be None");
        ctx.spawn(
            async move {
                loop {
                    if let Some(event) = event_pipe.next().await {
                        match event {
                            MpvEvent::MpvShutdown(e) => addr.do_send(e),
                            MpvEvent::MpvPropertyChanged(e) => addr.do_send(e),
                            MpvEvent::MpvFileLoaded(e) => addr.do_send(e),
                            MpvEvent::MpvPlaybackRestart(e) => addr.do_send(e),
                            _ => {}
                        }
                    } else {
                        error!("Mpv event pipeline unexpectedly ended");
                        break;
                    }
                }
            }
            .into_actor(self),
        );
    }
}

impl MpvActor {
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
        self.pause()?;
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

    pub(super) fn eof_reached(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::EofReached)
    }

    pub fn new() -> Result<Self> {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let event_pipe = MpvEventPipe::new(handle);
        let status = MpvStatus::default();
        let mpv = Self {
            status,
            handle,
            event_pipe: Some(event_pipe),
            file: None,
            file_loaded: false,
            server: todo!(),
            db: todo!(),
            user: todo!(),
        };

        mpv.init()
    }

    pub(super) fn pause(&mut self) -> Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        if file.paused {
            return Ok(());
        }
        file.paused = true;
        self.pause_cmd()
    }

    fn pause_cmd(&mut self) -> Result<()> {
        self.status.paused = true;
        self.set_property(MpvProperty::Pause, true.into())
    }

    pub(super) fn is_paused(&self) -> Result<bool> {
        if let Some(file) = &self.file {
            return Ok(file.paused);
        }
        self.get_property_flag(MpvProperty::Pause)
    }

    pub(super) fn start(&mut self) -> Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        if !file.paused {
            return Ok(());
        }
        file.paused = false;
        self.start_cmd()
    }

    fn start_cmd(&mut self) -> Result<()> {
        self.status.paused = false;
        self.set_property(MpvProperty::Pause, false.into())
    }

    pub(super) fn set_speed(&mut self, speed: f64) -> Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        if file.speed == speed {
            return Ok(());
        }
        file.speed = speed;
        self.set_speed_cmd(speed)
    }

    fn set_speed_cmd(&mut self, speed: f64) -> Result<()> {
        self.status.speed = speed;
        self.set_property(MpvProperty::Speed, PropertyValue::Double(speed))
    }

    pub(super) fn get_speed(&self) -> Result<f64> {
        if let Some(file) = &self.file {
            return Ok(file.speed);
        }
        self.get_property_f64(MpvProperty::Speed)
    }

    fn set_position(&mut self, pos: Duration) -> Result<()> {
        self.status.seeking = true;
        self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        )
    }

    pub(super) fn get_position(&self) -> Result<Duration> {
        let duration = self.get_property_f64(MpvProperty::PlaybackTime)?;
        Ok(Duration::from_secs_f64(duration))
    }

    pub(super) fn load(&mut self, play: PlayingFile) -> Result<()> {
        let Some(file) = &self.file else {
            return self.open(play);
        };
        if file.video.as_str().eq(play.video.as_str()) {
            if self.is_seeking()? {
                return Ok(());
            }
            return self.seek(play);
        }
        self.open(play)
    }

    fn open(&mut self, play: PlayingFile) -> Result<()> {
        self.file = Some(play.clone());
        if let Some(path) = play.video.to_path_str_new(&self.db) {
            self.file_loaded = true;
            self.open_cmd(path, play.paused, play.pos)
        } else {
            self.file_loaded = false;
            Ok(())
        }
    }

    fn open_cmd(&self, path: String, paused: bool, pos: Duration) -> Result<()> {
        let cmd = MpvCommand::Loadfile.try_into()?;
        let path = CString::new(path)?;
        let replace = CString::new("replace")?;
        let start = pos.as_secs_f64();
        let options = CString::new(format!("start={start},pause={paused}"))?;

        self.send_command(&[&cmd, &path, &replace, &options])
    }

    fn seek(&mut self, play: PlayingFile) -> Result<()> {
        if self.file.is_none() {
            return Ok(());
        }
        if play.paused {
            self.pause()?;
        } else {
            self.start()?;
        }
        self.set_speed(play.speed)?;
        self.set_position(play.pos)
    }

    pub(super) fn unload(&mut self) {
        self.file = None;
        self.file_loaded = false;
    }

    pub(super) fn is_seeking(&self) -> Result<bool> {
        Ok(self.status.seeking)
    }
}
