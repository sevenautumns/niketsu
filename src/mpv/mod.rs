use std::{ffi::CString, mem::MaybeUninit};

use crate::mpv::bindings::*;
use anyhow::Result;

pub mod bindings;
pub mod event;

#[derive(Debug)]
pub struct MpvHandle(*mut mpv_handle);

unsafe impl Send for MpvHandle {}

#[derive(Debug)]
pub struct Mpv(MpvHandle);

impl Mpv {
    pub fn new(mpv: *mut mpv_handle) -> Self {
        let mpv = MpvHandle(mpv);
        Self(mpv)
    }

    fn send_command(&self, cmd: &[&str]) -> Result<()> {
        let mut cmd_vec = vec![];
        let mut cmd_ptr_vec = vec![];
        for c in cmd {
            cmd_vec.push(CString::new(*c)?);
            cmd_ptr_vec.push(cmd_vec.last().unwrap().as_ptr())
        }
        cmd_ptr_vec.push(std::ptr::null());
        let ret = unsafe { mpv_command_async(self.0 .0, 0, cmd_ptr_vec.as_mut_ptr()) };
        //TODO handle error case
        Ok(())
    }

    // pub fn play_

    pub fn get_playback_position(&self) -> Result<()> {
        let duration = CString::new("playback-time")?;
        unsafe {
            let ret = mpv_get_property_async(
                self.0 .0,
                0,
                duration.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
            );
            // TODO check for error
            // Ok(data.assume_init())
            Ok(())
        }
    }

    pub fn set_playback_position(&self, pos: f64) -> Result<()> {
        let duration = CString::new("playback-time")?;
        unsafe {
            let ret = mpv_set_property_async(
                self.0 .0,
                0,
                duration.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                &pos as *const _ as *mut _,
            );
            // TODO check for error
        }
        Ok(())
    }
}
