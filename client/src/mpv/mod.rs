use std::convert::TryInto;
use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use iced::Subscription;
use log::*;
use strum::{AsRefStr, EnumString};
use tokio::sync::Mutex;

use self::event::{MpvEvent, MpvEventPipe, PropertyValue};
use crate::fs::FileDatabase;
use crate::mpv::bindings::*;
use crate::video::{PlayingFile, SeekEvent, SeekEventExt, Video};
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
    playing: Option<PlayingFile>,
    paused: bool,
    seeking: bool,
    event_pipe: Arc<Mutex<MpvEventPipe>>,
}

impl Mpv {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let event_pipe = Arc::new(Mutex::new(MpvEventPipe::new(handle)));
        Self {
            handle,
            event_pipe,
            playing: None,
            paused: true,
            seeking: false,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn seeking(&self) -> bool {
        self.seeking
    }

    pub fn react_to(&mut self, event: MpvEvent) -> Result<Option<MpvResultingAction>> {
        match event {
            MpvEvent::Shutdown => return Ok(Some(MpvResultingAction::Exit)),
            MpvEvent::Seek => {
                trace!("Seek started");
                self.seeking = true;
            }
            MpvEvent::FileLoaded => {
                trace!("File loaded");
                if let Some(playing) = self.playing.as_mut() {
                    playing.heartbeat = true;
                }
                if let Some(PlayingFile { last_seek, .. }) = self.playing.clone() {
                    trace!("Set paused to {} after file loaded", self.paused);
                    self.pause(self.paused)?;
                    if let Some(pos) = last_seek.pos() {
                        self.set_playback_position(pos)?;
                        return Ok(None);
                    }
                }
            }
            MpvEvent::PropertyChanged(prop, value) => {
                trace!("Property Changed: {prop:?} {value:?}");
                if let MpvProperty::Pause = prop {
                    if let PropertyValue::Flag(p) = value {
                        if p.ne(&self.paused) {
                            self.paused = p;
                            match p {
                                true => {
                                    if self.get_eof_reached()? {
                                        self.playing = None;
                                        return Ok(Some(MpvResultingAction::PlayNext));
                                    }
                                    return Ok(Some(MpvResultingAction::Pause));
                                }
                                false => return Ok(Some(MpvResultingAction::Start)),
                            }
                        }
                    }
                }
            }
            MpvEvent::PlaybackRestart => {
                trace!("Playback restarted");
                self.seeking = false;
                if self.playing.is_some() {
                    let new_pos = self.get_playback_position()?;
                    if let Some(playing) = &mut self.playing {
                        if let Some(SeekEvent { when, pos }) = playing.last_seek {
                            let mut expected = pos;
                            if self.paused {
                                if expected.ne(&new_pos) {
                                    playing.last_seek = Some(SeekEvent::new(new_pos));
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
                                playing.last_seek = Some(SeekEvent::new(new_pos));
                                return Ok(Some(MpvResultingAction::Seek(new_pos)));
                            }
                            return Ok(None);
                        }
                        playing.last_seek = Some(SeekEvent::new(new_pos));
                        return Ok(Some(MpvResultingAction::Seek(new_pos)));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    pub fn init(&self) -> Result<()> {
        self.set_ocs(true)?;
        self.set_keep_open(true)?;
        self.set_keep_open_pause(true)?;
        // self.set_cache_pause(false)?;
        self.set_idle_mode(true)?;
        self.set_force_window(true)?;
        self.set_config(true)?;
        self.set_input_default_bindings(true)?;
        self.set_input_vo_keyboard(true)?;
        self.set_input_media_keys(true)?;

        let ret = unsafe { mpv_initialize(self.handle.0) };
        let ret = Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?);

        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::Filename)?;

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

    pub fn reload(&mut self, db: &FileDatabase) -> Result<()> {
        if let Some(playing) = &self.playing {
            self.load(
                playing.video.clone(),
                playing.last_seek.pos(),
                self.paused,
                db,
            )?;
        }
        Ok(())
    }

    pub fn may_reload(&mut self, db: &FileDatabase) -> Result<()> {
        if let Some(PlayingFile {
            video: Video::File { path, .. },
            ..
        }) = &self.playing
        {
            if path.is_none() {
                return self.reload(db);
            }
        }
        Ok(())
    }

    pub fn load(
        &mut self,
        mut video: Video,
        seek: Option<Duration>,
        paused: bool,
        db: &FileDatabase,
    ) -> Result<()> {
        let last_seek = seek.map(SeekEvent::new);
        trace!("Set pause to {paused} during video load");
        self.pause(paused)?;
        let path = match &mut video {
            Video::File { name, path } => {
                if path.is_none() {
                    if let Ok(Some(file)) = db.find_file(name) {
                        *path = Some(file.path);
                    } else {
                        self.playing = Some(PlayingFile {
                            video,
                            last_seek,
                            heartbeat: false,
                        });
                        return Ok(());
                    }
                }
                CString::new(path.as_ref().unwrap().as_os_str().to_str().unwrap())?
            }
            Video::Url(url) => CString::new(url.as_str())?,
        };

        self.playing = Some(PlayingFile {
            video,
            last_seek,
            heartbeat: false,
        });

        let cmd = MpvCommand::Loadfile.try_into()?;
        self.send_command(&[&cmd, &path])?;
        Ok(())
    }

    pub fn seek(
        &mut self,
        video: Video,
        seek: Duration,
        paused: bool,
        db: &FileDatabase,
    ) -> Result<()> {
        trace!("Received seek: video: {video:?}, {seek:?}, paused: {paused}");
        if Some(video.clone()).ne(&self.playing.as_ref().map(|p| p.video.clone())) {
            trace!("Received seek includes new video");
            return self.load(video, Some(seek), paused, db);
        }
        trace!("Set pause to {paused} during seek request");
        self.pause(paused)?;
        if let Some(PlayingFile { last_seek, .. }) = &mut self.playing {
            *last_seek = Some(SeekEvent::new(seek));
            self.set_playback_position(seek)?;
        }
        Ok(())
    }

    pub fn playing(&self) -> Option<PlayingFile> {
        if let Some(playing) = &self.playing {
            return Some(playing.clone());
        }
        None
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

    pub fn set_cache_pause(&self, flag: bool) -> Result<()> {
        self.set_property(MpvProperty::CachePause, PropertyValue::Flag(flag))
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

    pub fn get_eof_reached(&self) -> Result<bool> {
        self.get_property_flag(MpvProperty::EofReached)
    }

    pub fn set_playback_position(&mut self, pos: Duration) -> Result<()> {
        self.set_property(
            MpvProperty::PlaybackTime,
            PropertyValue::Double(pos.as_secs_f64()),
        )?;
        if let Some(PlayingFile { last_seek, .. }) = &mut self.playing {
            *last_seek = Some(SeekEvent::new(pos));
        }
        Ok(())
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.handle.0) };
    }
}

#[derive(Debug, Clone)]
pub enum MpvResultingAction {
    PlayNext,
    Seek(Duration),
    ReOpenFile,
    Pause,
    Start,
    Exit,
}
