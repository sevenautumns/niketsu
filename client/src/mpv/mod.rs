use std::convert::TryInto;
use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::process::exit;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use futures::StreamExt;
use log::*;
use strum::{AsRefStr, EnumString};
use tokio::sync::mpsc::UnboundedSender as MpscSender;

use self::event::{MpvEvent, MpvEventPipe, PropertyValue};
use crate::client::{ClientInner, LogResult, PlayerMessage};
use crate::fs::FileDatabase;
use crate::mpv::bindings::*;
use crate::user::ThisUser;
use crate::video::{PlayingFile, SeekEvent, SeekEventExt, Video};
use crate::ws::ServerMessage;

pub mod bindings;
pub mod error;
pub mod event;

pub const MINIMUM_DIFF_SEEK: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
pub struct MpvHandle(*mut mpv_handle);

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

#[derive(Debug)]
pub struct Mpv {
    handle: MpvHandle,
    playing: Option<PlayingFile>,
    paused: bool,
    pausing: Option<Instant>,
    speed: f64,
    seeking: bool,
}

impl Mpv {
    pub fn new(client_sender: MpscSender<PlayerMessage>) -> Self {
        let ctx = unsafe { mpv_create() };
        let handle = MpvHandle(ctx);
        let mut event_pipe = MpvEventPipe::new(handle);

        tokio::spawn(async move {
            loop {
                if let Some(event) = event_pipe.next().await {
                    if let Err(err) = client_sender.send(PlayerMessage::Mpv(event)) {
                        error!("Client sender unexpectedly ended: {err:?}");
                        exit(1)
                    }
                } else {
                    error!("Mpv event pipeline unexpectedly ended");
                    exit(1)
                }
            }
        });

        Self {
            handle,
            playing: None,
            paused: true,
            speed: 1.0,
            seeking: false,
            pausing: None,
        }
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn seeking(&self) -> bool {
        self.seeking
    }

    pub fn init(&mut self) -> Result<()> {
        self.set_ocs(true)?;
        self.set_keep_open(true)?;
        self.set_keep_open_pause(true)?;
        self.set_cache_pause(false)?;
        self.set_idle_mode(true)?;
        self.set_force_window(true)?;
        self.set_config(true)?;
        self.set_input_default_bindings(true)?;
        self.set_input_vo_keyboard(true)?;
        self.set_input_media_keys(true)?;

        let ret = unsafe { mpv_initialize(self.handle.0) };
        let ret = Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?);

        self.pause(true)?;
        self.observe_property(MpvProperty::Pause)?;
        self.observe_property(MpvProperty::Filename)?;
        self.observe_property(MpvProperty::Speed)?;

        ret
    }

    fn observe_property(&self, prop: MpvProperty) -> Result<()> {
        let format = prop.format();
        let prop: CString = prop.try_into()?;
        let ret = unsafe { mpv_observe_property(self.handle.0, 0, prop.as_ptr(), format) };
        Ok(TryInto::<mpv_error>::try_into(ret)?.try_into()?)
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

    pub fn unload(&mut self) {
        self.playing = None;
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
                let path = match path {
                    Some(path) => path.clone(),
                    None => {
                        if let Ok(Some(file)) = db.find_file(name) {
                            file.path
                        } else {
                            self.playing = Some(PlayingFile {
                                video,
                                last_seek,
                                heartbeat: false,
                            });
                            return Ok(());
                        }
                    }
                };
                if let Some(path) = path.as_os_str().to_str() {
                    CString::new(path)?
                } else {
                    bail!("Can not convert \"{path:?}\" to os_string")
                }
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

    pub fn show_text(&self, text: &str) -> Result<()> {
        let cmd = MpvCommand::ShowText.try_into()?;
        self.send_command(&[&cmd, &CString::new(text)?])?;
        Ok(())
    }

    pub fn seek(
        &mut self,
        video: Video,
        seek: Duration,
        paused: bool,
        speed: f64,
        db: &FileDatabase,
    ) -> Result<()> {
        trace!("Received seek: video: {video:?}, {seek:?}, paused: {paused}, speed: {speed}");
        self.set_playback_speed(speed)?;
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
        let cur_pause = self.get_pause_state();
        if cur_pause != pause {
            self.pausing = Some(Instant::now());
            self.paused = pause;
            return self.set_property(MpvProperty::Pause, PropertyValue::Flag(pause));
        }
        Ok(())
    }

    pub fn get_playback_position(&self) -> Result<Duration> {
        let duration = self.get_property_f64(MpvProperty::PlaybackTime)?;
        Ok(Duration::from_secs_f64(duration))
    }

    pub fn get_playback_speed(&mut self) -> Result<f64> {
        self.get_property_f64(MpvProperty::Speed)
    }

    pub fn get_pause_state(&mut self) -> bool {
        self.get_property_flag(MpvProperty::Pause).unwrap_or(true)
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

    pub fn set_playback_speed(&mut self, speed: f64) -> Result<()> {
        self.set_property(MpvProperty::Speed, PropertyValue::Double(speed))?;
        self.speed = speed;
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
    PlayNext(Video),
    Seek(Duration),
    ReOpenFile,
    PlaybackSpeed(f64),
    Pause,
    Start,
    Exit,
}

impl MpvResultingAction {
    pub fn handle(self, client: &mut ClientInner) -> Result<()> {
        match self {
            MpvResultingAction::PlayNext(video) => {
                debug!("Mpv process: play next");
                if let Some(next) = client.playlist_widget.load().next_video(&video) {
                    client.ws.load().send(ServerMessage::Select {
                        filename: next.as_str().to_string().into(),
                        username: client.user.load().name(),
                    })?;
                    client.mpv.load(next, None, true, &client.db).log();
                    Ok(())
                } else {
                    client.ws.load().send(ServerMessage::Select {
                        filename: None,
                        username: client.user.load().name(),
                    })?;
                    Ok(())
                }
            }
            MpvResultingAction::Seek(position) => {
                debug!("Mpv process: seek {position:?}");
                if let Some(playing) = client.mpv.playing() {
                    client.ws.load().send(ServerMessage::Seek {
                        filename: playing.video.as_str().to_string(),
                        position,
                        username: client.user.load().name(),
                        paused: client.mpv.get_pause_state(),
                        desync: false,
                        speed: client.mpv.speed(),
                    })?;
                    return Ok(());
                }
                Ok(())
            }
            MpvResultingAction::ReOpenFile => {
                debug!("Mpv process: re-open file");
                client.mpv.reload(&client.db).log();
                Ok(())
            }
            MpvResultingAction::Pause => {
                debug!("Mpv process: pause");
                let mut state = None;
                client.user.rcu(|u| {
                    let mut user = ThisUser::clone(u);
                    state = user.set_ready(false);
                    user
                });
                if let Some(state) = state {
                    client.ws.load().send(state)?;
                }
                client.ws.load().send(ServerMessage::Pause {
                    username: client.user.load().name(),
                })?;
                Ok(())
            }
            MpvResultingAction::Start => {
                debug!("Mpv process: start");
                let mut state = None;
                client.user.rcu(|u| {
                    let mut user = ThisUser::clone(u);
                    state = user.set_ready(true);
                    user
                });
                if let Some(state) = state {
                    client.ws.load().send(state)?;
                }
                client.ws.load().send(ServerMessage::Start {
                    username: client.user.load().name(),
                })?;
                Ok(())
            }
            MpvResultingAction::PlaybackSpeed(speed) => {
                debug!("Mpv process: playback speed");
                client.ws.load().send(ServerMessage::PlaybackSpeed {
                    username: client.user.load().name(),
                    speed,
                })?;
                Ok(())
            }
            MpvResultingAction::Exit => {
                debug!("Mpv process: exit");
                exit(0)
            }
        }
    }
}

impl ClientInner {
    pub fn react_to_mpv(&mut self, event: MpvEvent) -> Result<()> {
        match event {
            MpvEvent::Shutdown => return MpvResultingAction::Exit.handle(self),
            MpvEvent::Seek => {
                trace!("Seek started");
                self.mpv.seeking = true;
            }
            MpvEvent::FileLoaded => {
                trace!("File loaded");
                if let Some(playing) = self.mpv.playing.as_mut() {
                    playing.heartbeat = true;
                }
                if let Some(PlayingFile { last_seek, .. }) = self.mpv.playing.clone() {
                    trace!("Set paused to {} after file loaded", self.mpv.paused);
                    let paused = self.mpv.paused;
                    self.mpv.pause(paused)?;
                    if let Some(pos) = last_seek.pos() {
                        self.mpv.set_playback_position(pos)?;
                        return Ok(());
                    }
                }
            }
            MpvEvent::PropertyChanged(when, prop, value) => {
                trace!("Property Changed: {prop:?} {value:?}");
                match prop {
                    MpvProperty::Pause => {
                        if let Some(pause_time) = self.mpv.pausing {
                            // If the property changed before the time we last received a pause/start instruction,
                            // ignore it, because it can not be correct
                            if when <= pause_time {
                                return Ok(());
                            }
                            self.mpv.pausing = None;
                        }
                        let paused = self.mpv.get_pause_state();
                        if paused != self.mpv.paused {
                            self.mpv.paused = paused;
                            // Should we be paused
                            if self.mpv.paused {
                                // and file ended
                                if self.mpv.get_eof_reached()? {
                                    // and we are playing
                                    if let Some(playing) = self.mpv.playing.take() {
                                        // play next
                                        return MpvResultingAction::PlayNext(playing.video)
                                            .handle(self);
                                    }
                                }
                                return MpvResultingAction::Pause.handle(self);
                            }
                            return MpvResultingAction::Start.handle(self);
                        }
                    }
                    MpvProperty::Speed => {
                        if let Ok(speed) = self.mpv.get_playback_speed() {
                            if speed != self.mpv.speed {
                                self.mpv.speed = speed;
                                return MpvResultingAction::PlaybackSpeed(speed).handle(self);
                            }
                        }
                    }
                    _ => {}
                }
            }
            MpvEvent::PlaybackRestart => {
                trace!("Playback restarted");
                self.mpv.seeking = false;
                if self.mpv.playing.is_some() {
                    let new_pos = self.mpv.get_playback_position()?;
                    let paused = self.mpv.paused;
                    if let Some(playing) = &mut self.mpv.playing {
                        if let Some(SeekEvent { when, pos }) = playing.last_seek {
                            let mut expected = pos;
                            if paused {
                                if expected.ne(&new_pos) {
                                    playing.last_seek = Some(SeekEvent::new(new_pos));
                                    return MpvResultingAction::Seek(new_pos).handle(self);
                                }
                                return Ok(());
                            }
                            expected = expected.saturating_add(when.elapsed());
                            let diff = match expected < new_pos {
                                true => new_pos - expected,
                                false => expected - new_pos,
                            };
                            if diff > MINIMUM_DIFF_SEEK {
                                playing.last_seek = Some(SeekEvent::new(new_pos));
                                return MpvResultingAction::Seek(new_pos).handle(self);
                            }
                            return Ok(());
                        }
                        playing.last_seek = Some(SeekEvent::new(new_pos));
                        return MpvResultingAction::Seek(new_pos).handle(self);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
