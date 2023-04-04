use std::{ffi::CString, mem::MaybeUninit, time::Duration};

use crate::mpv::bindings::*;
use anyhow::Result;

use std::convert::TryInto;
use strum::AsRefStr;

use self::event::MpvEventPipe;

pub mod bindings;
pub mod error;
pub mod event;

#[derive(Debug, Clone, Copy)]
pub struct MpvHandle(*mut mpv_handle);

unsafe impl Send for MpvHandle {}

#[derive(Debug, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvProperty {
    PlaybackTime,
    Osc,
}

impl TryFrom<MpvProperty> for CString {
    type Error = anyhow::Error;

    fn try_from(prop: MpvProperty) -> Result<Self> {
        Ok(CString::new(prop.as_ref())?)
    }
}

#[derive(Debug, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvCommand {
    Loadfile,
}

impl TryFrom<MpvCommand> for CString {
    type Error = anyhow::Error;

    fn try_from(cmd: MpvCommand) -> Result<Self> {
        Ok(CString::new(cmd.as_ref())?)
    }
}

#[derive(Debug)]
pub struct Mpv(MpvHandle);

impl Mpv {
    // TODO remove clippy here, when we allow for configuration
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let ctx = unsafe { mpv_create() };
        let mpv = MpvHandle(ctx);
        Self(mpv)
    }

    pub fn init(&self) -> Result<()> {
        let ret = unsafe { mpv_initialize(self.0 .0) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
    }

    pub fn event_pipe(&self) -> MpvEventPipe {
        MpvEventPipe::new(self.0)
    }

    fn send_command(&self, cmd: &[&CString]) -> Result<()> {
        let mut cmd_ptr = vec![];
        for c in cmd {
            cmd_ptr.push(c.as_ptr())
        }
        cmd_ptr.push(std::ptr::null());
        let ret = unsafe { mpv_command_async(self.0 .0, 0, cmd_ptr.as_mut_ptr()) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
    }

    pub fn load_file(&self, file: &str) -> Result<()> {
        let cmd = MpvCommand::Loadfile.try_into()?;
        let file = CString::new(file)?;
        self.send_command(&[&cmd, &file])
    }

    pub fn set_ocs(&self, set: bool) -> Result<()> {
        let osc: CString = MpvProperty::Osc.try_into()?;
        let mut flag = set as isize;
        unsafe {
            let ret = mpv_set_property(
                self.0 .0,
                osc.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                &mut flag as *mut _ as *mut _,
            );
            Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
        }
    }

    pub fn get_playback_position(&self) -> Result<Duration> {
        let duration: CString = MpvProperty::PlaybackTime.try_into()?;
        let mut data: MaybeUninit<f64> = MaybeUninit::uninit();
        unsafe {
            let ret = mpv_get_property(
                self.0 .0,
                duration.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                data.as_mut_ptr() as *mut _,
            );
            TryInto::<mpv_error>::try_into(ret)?.try_into()?;
            let seconds = data.assume_init();
            Ok(Duration::from_secs_f64(seconds))
        }
    }

    pub fn set_playback_position(&self, pos: Duration) -> Result<()> {
        let duration: CString = MpvProperty::PlaybackTime.try_into()?;
        let pos = pos.as_secs_f64();
        unsafe {
            let ret = mpv_set_property(
                self.0 .0,
                duration.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                &pos as *const _ as *mut _,
            );
            Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.0 .0) };
    }
}
