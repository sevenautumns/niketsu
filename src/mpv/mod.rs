use std::convert::TryInto;
use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use iced::Subscription;
use log::*;
use strum::{AsRefStr, EnumString};
use tokio::sync::Mutex;
use url::Url;

use self::event::{MpvEvent, MpvEventPipe, PropertyValue};
use crate::file_table::Video;
use crate::mpv::bindings::*;
use crate::window::MainMessage;

pub mod bindings;
pub mod error;
pub mod event;

pub const MINIMUM_DIFF_SEEK: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
pub struct MpvHandle(*mut mpv_handle);

unsafe impl Send for MpvHandle {}

#[derive(Debug, EnumString, AsRefStr, Clone, Copy)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvProperty {
    PlaybackTime,
    Osc,
    Pause,
    Filename,
    KeepOpen,
    KeepOpenPause,
    ForceWindow,
    Idle,
    PercentPos,
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
            MpvProperty::PercentPos => mpv_format::MPV_FORMAT_DOUBLE,
            MpvProperty::InputDefaultBindings => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputVoKeyboard => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::InputMediaKeys => mpv_format::MPV_FORMAT_FLAG,
        }
    }
}

#[derive(Debug, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvCommand {
    Loadfile,
    Stop,
}

impl TryFrom<MpvCommand> for CString {
    type Error = anyhow::Error;

    fn try_from(cmd: MpvCommand) -> Result<Self> {
        Ok(CString::new(cmd.as_ref())?)
    }
}

#[derive(Debug)]
pub struct Mpv {
    handle: MpvHandle,
    file: Option<Video>,
    paused: bool,
    last_seek: Option<SeekEvent>,
    event_pipe: Arc<Mutex<MpvEventPipe>>,
}

impl Mpv {
    // TODO remove clippy here, when we allow for configuration
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let event_pipe = Arc::new(Mutex::new(MpvEventPipe::new(handle)));
        Self {
            handle,
            file: None,
            event_pipe,
            last_seek: None,
            paused: true,
        }
    }

    pub fn react_to(&mut self, event: MpvEvent) -> Result<Option<MpvResultingAction>> {
        match event {
            MpvEvent::Shutdown => Ok(Some(MpvResultingAction::Exit)),
            MpvEvent::PropertyChanged(prop, value) => match (prop, value) {
                (MpvProperty::Pause, PropertyValue::Flag(paused)) if paused != self.paused => {
                    self.paused = paused;
                    match paused {
                        true => {
                            let pos = self.get_pos_percent()?;
                            trace!("Pause percent: {pos}");
                            if pos >= 100f64 {
                                return Ok(Some(MpvResultingAction::PlayNext));
                            }
                            Ok(Some(MpvResultingAction::Pause))
                        }
                        false => Ok(Some(MpvResultingAction::Start)),
                    }
                }
                (MpvProperty::Filename, PropertyValue::String(file)) => {
                    let file = Video::from_string(file.to_str()?.to_string());
                    match &self.file {
                        Some(sf) if sf.ne(&file) => Ok(Some(MpvResultingAction::ReOpenFile)),
                        None => {
                            let cmd = MpvCommand::Stop.try_into()?;
                            self.send_command(&[&cmd])?;
                            Ok(None)
                        }
                        _ => Ok(None),
                    }
                }
                _ => Ok(None),
            },
            // MpvEvent::StartFile(_) => todo!(),
            // MpvEvent::EndFile(_) => todo!(),
            MpvEvent::FileLoaded => Ok(Some(MpvResultingAction::StartHeartbeat)),
            // MpvEvent::Seek => todo!(),
            MpvEvent::PlaybackRestart => {
                let new_pos = self.get_playback_position()?;
                if let Some(SeekEvent { when, pos }) = &self.last_seek {
                    let mut expected = *pos;
                    if self.paused {
                        if expected.ne(&new_pos) {
                            return Ok(Some(MpvResultingAction::Seek(new_pos)));
                        }
                        return Ok(None);
                    }
                    expected = expected.saturating_add(when.elapsed());
                    let diff = match expected < new_pos {
                        true => new_pos - expected,
                        false => expected - new_pos,
                    };
                    if diff > MINIMUM_DIFF_SEEK {
                        return Ok(Some(MpvResultingAction::Seek(new_pos)));
                    }
                    return Ok(None);
                }
                Ok(Some(MpvResultingAction::Seek(new_pos)))
            }
            _ => Ok(None),
        }
    }

    pub fn init(&self) -> Result<()> {
        // TODO open the mpv window here somehow
        // TODO remove config from here
        self.set_ocs(true)?;
        self.set_keep_open(true)?;
        self.set_keep_open_pause(true)?;
        self.set_idle_mode(true)?;
        self.set_force_window(true)?;
        self.set_config(true)?;
        self.set_input_default_bindings(true)?;
        self.set_input_vo_keyboard(true)?;
        self.set_input_media_keys(true)?;
        // TODO remove config from here

        let ret = unsafe { mpv_initialize(self.handle.0) };
        let ret = Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?);

        // TODO remove config from here
        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::Filename)?;
        // TODO remove config from here

        ret
    }

    fn observe_property(&self, prop: MpvProperty) -> Result<()> {
        let format = prop.format();
        let prop: CString = prop.try_into()?;
        let ret = unsafe { mpv_observe_property(self.handle.0, 0, prop.as_ptr(), format) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
    }

    pub fn subscribe(&self) -> Subscription<MainMessage> {
        iced::subscription::unfold(
            std::any::TypeId::of::<Self>(),
            self.event_pipe.clone(),
            |event_pipe| async move {
                let event = event_pipe.lock().await.next().await.map(|e| e.into());
                (event.unwrap(), event_pipe)
            },
        )
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

    fn load(&mut self, path: CString, video: Video, paused: bool) -> Result<()> {
        self.pause(paused)?;
        let cmd = MpvCommand::Loadfile.try_into()?;
        self.send_command(&[&cmd, &path])?;
        self.file = Some(video);
        Ok(())
    }

    pub fn load_file(&mut self, path: PathBuf, paused: bool) -> Result<()> {
        // TODO do not unwrap
        let filename = path
            .as_path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let video = Video::File(filename);
        // TODO do not unwrap
        let path_cstring = CString::new(path.as_os_str().to_str().unwrap())?;
        self.load(path_cstring, video, paused)
    }

    pub fn load_url(&mut self, url: Url, paused: bool) -> Result<()> {
        let video = Video::Url(url);
        let url_cstring = CString::new(video.as_str())?;
        self.load(url_cstring, video, paused)
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

    pub fn set_ocs(&self, ocs: bool) -> Result<()> {
        self.set_property(MpvProperty::Osc, PropertyValue::Flag(ocs))
    }

    pub fn set_keep_open(&self, keep_open: bool) -> Result<()> {
        self.set_property(MpvProperty::KeepOpen, PropertyValue::Flag(keep_open))
    }

    pub fn set_keep_open_pause(&self, keep_open_pause: bool) -> Result<()> {
        self.set_property(
            MpvProperty::KeepOpenPause,
            PropertyValue::Flag(keep_open_pause),
        )
    }

    pub fn set_idle_mode(&self, idle_mode: bool) -> Result<()> {
        self.set_property(MpvProperty::Idle, PropertyValue::Flag(idle_mode))
    }

    pub fn set_force_window(&self, force_window: bool) -> Result<()> {
        self.set_property(MpvProperty::ForceWindow, PropertyValue::Flag(force_window))
    }

    pub fn set_config(&self, config: bool) -> Result<()> {
        self.set_property(MpvProperty::Config, PropertyValue::Flag(config))
    }

    pub fn set_input_default_bindings(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputDefaultBindings, PropertyValue::Flag(flag))
    }

    pub fn set_input_vo_keyboard(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputVoKeyboard, PropertyValue::Flag(flag))
    }

    pub fn set_input_media_keys(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::InputMediaKeys, PropertyValue::Flag(flag))
    }

    pub fn pause(&mut self, pause: bool) -> Result<()> {
        self.paused = pause;
        self.set_property(MpvProperty::Pause, PropertyValue::Flag(pause))
    }

    pub fn get_playback_position(&self) -> Result<Duration> {
        let duration = self.get_property_f64(MpvProperty::PlaybackTime)?;
        Ok(Duration::from_secs_f64(duration))
    }

    pub fn get_pos_percent(&self) -> Result<f64> {
        self.get_property_f64(MpvProperty::PercentPos)
    }

    pub fn set_playback_position(&mut self, pos: Duration) -> Result<()> {
        self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        )?;
        self.last_seek = Some(SeekEvent::new(pos));
        Ok(())
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.handle.0) };
    }
}

#[derive(Debug, Clone)]
pub struct SeekEvent {
    when: Instant,
    pos: Duration,
}

impl SeekEvent {
    pub fn new(pos: Duration) -> Self {
        Self {
            when: Instant::now(),
            pos,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MpvResultingAction {
    PlayNext,
    Seek(Duration),
    ReOpenFile,
    StartHeartbeat,
    Pause,
    Start,
    Exit,
}
