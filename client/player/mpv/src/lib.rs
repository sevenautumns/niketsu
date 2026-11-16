use std::ffi::{CStr, CString, c_int, c_void};
use std::mem::MaybeUninit;
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::StreamExt;
use niketsu_core::log_err;
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait, Source};
use strum::{AsRefStr, EnumString};
use tracing::{debug, error};

use self::bindings::*;
use self::event::{MpvEventPipe, MpvEventTrait, PropertyValue};
use self::seek::Seeks;

mod bindings;
mod error;
mod event;
#[cfg(test)]
mod probe;
mod seek;

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
    KeepOpen,
    CachePause,
    ForceWindow,
    Idle,
    EofReached,
    Config,
    InputDefaultBindings,
    InputVoKeyboard,
    InputMediaKeys,
    PausedForCache,
    CachePauseWait,
    Seeking,
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
            MpvProperty::PausedForCache => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::Seeking => mpv_format::MPV_FORMAT_FLAG,
            MpvProperty::CachePauseWait => mpv_format::MPV_FORMAT_DOUBLE,
        }
    }
}

#[derive(Debug, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum MpvCommand {
    Loadfile,
    Stop,
    Seek,
    ShowText,
}

impl From<MpvCommand> for CString {
    fn from(cmd: MpvCommand) -> Self {
        CString::new(cmd.as_ref()).expect("Got invalid UTF-8")
    }
}

/// What the app asked of mpv and how far mpv is with it. The play state
/// and the speed are the ones the app knows of, so a change the user made
/// in mpv shows as a difference.
#[derive(Debug)]
pub struct MpvStatus {
    paused: bool,
    speed: f64,
    /// The playlist entry mpv made of the app's load, until that ends. mpv
    /// numbers every load, which ties its events to the load they are of.
    entry: Option<i64>,
    /// The playlist entry mpv's events are about at the moment.
    started: Option<i64>,
    /// Whether the app's load has finished. From then on the player has a
    /// position of its own.
    loaded: bool,
    seeks: Seeks,
}

impl MpvStatus {
    /// Whether mpv's events are about the app's load, and not about one
    /// it replaced.
    fn current(&self) -> bool {
        self.entry.is_some() && self.entry == self.started
    }

    /// The app's load is over.
    fn end(&mut self) {
        self.entry = None;
        self.loaded = false;
        self.seeks = Seeks::default();
    }
}

impl Default for MpvStatus {
    fn default() -> Self {
        Self {
            paused: true,
            speed: 1.0,
            entry: None,
            started: None,
            loaded: false,
            seeks: Seeks::default(),
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
        if ctx.is_null() {
            bail!("mpv_create() failed");
        }
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

    /// An mpv without a window, sound or the user's configuration.
    #[cfg(test)]
    pub(crate) fn headless() -> Result<Self> {
        let ctx = unsafe { mpv_create() };
        if ctx.is_null() {
            bail!("mpv_create() failed");
        }
        let handle = MpvHandle(ctx);
        let mut mpv = Self {
            status: MpvStatus::default(),
            handle,
            event_pipe: MpvEventPipe::new(handle),
        };
        for (name, value) in [(c"vo", c"null"), (c"ao", c"null"), (c"idle", c"yes")] {
            let ret = unsafe { mpv_set_option_string(ctx, name.as_ptr(), value.as_ptr()) };
            mpv_error::try_from(ret)?.ok()?;
        }
        mpv.init_handle()?;
        mpv.post_init()?;
        Ok(mpv)
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
        let ret = value.with_ptr(|ptr| unsafe {
            mpv_set_property(self.handle.0, prop.as_ptr(), value.format(), ptr)
        });
        mpv_error::try_from(ret)?.ok()
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
        // MPV_FORMAT_FLAG writes a C int, not a single byte
        let mut data: MaybeUninit<c_int> = MaybeUninit::uninit();
        unsafe {
            let ret = mpv_get_property(
                self.handle.0,
                prop.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                data.as_mut_ptr() as *mut c_void,
            );
            mpv_error::try_from(ret)?.ok()?;
            Ok(data.assume_init() != 0)
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

    /// Runs a command once mpv has taken every earlier one. Returns the
    /// playlist entry it made, which a load does.
    fn send_command(&self, cmd: &[&CStr]) -> Result<Option<i64>> {
        let mut cmd = Self::build_cmd(cmd);
        let mut result = MaybeUninit::<mpv_node>::zeroed();
        let ret = unsafe { mpv_command_ret(self.handle.0, cmd.as_mut_ptr(), result.as_mut_ptr()) };
        mpv_error::try_from(ret)?.ok()?;
        unsafe {
            let entry = playlist_entry(result.assume_init_ref());
            mpv_free_node_contents(result.as_mut_ptr());
            Ok(entry)
        }
    }
}

/// The `playlist_entry_id` of what a load command returned.
///
/// # Safety
/// `result` must be a node mpv filled in and that was not freed.
unsafe fn playlist_entry(result: &mpv_node) -> Option<i64> {
    if result.format != mpv_format::MPV_FORMAT_NODE_MAP {
        return None;
    }
    unsafe {
        let list = &*result.u.list;
        (0..list.num as usize).find_map(|i| {
            let value = &*list.values.add(i);
            let named = CStr::from_ptr(*list.keys.add(i)) == c"playlist_entry_id";
            match named && value.format == mpv_format::MPV_FORMAT_INT64 {
                true => Some(value.u.int64),
                false => None,
            }
        })
    }
}

#[async_trait]
impl MediaPlayerTrait for Mpv {
    fn start(&mut self) {
        self.status.paused = false;
        log_err!(self.set_property(MpvProperty::Pause, false.into()))
    }

    fn pause(&mut self) {
        self.status.paused = true;
        log_err!(self.set_property(MpvProperty::Pause, true.into()))
    }

    fn is_paused(&self) -> Option<bool> {
        self.status.entry?;
        let paused = log_err!(self.get_property_flag(MpvProperty::Pause), true);
        Some(paused)
    }

    fn set_speed(&mut self, speed: f64) {
        self.status.speed = speed;
        let res = self.set_property(MpvProperty::Speed, PropertyValue::Double(speed));
        log_err!(res)
    }

    /// Seeks exactly, whatever the user's mpv configuration makes of a
    /// seek, so the landing can be told from a seek of the user.
    fn set_position(&mut self, pos: Duration) {
        if !self.status.loaded {
            debug!(?pos, "no file is loaded, ignoring set_position");
            return;
        }
        let cmd: CString = MpvCommand::Seek.into();
        let target = CString::new(pos.as_secs_f64().to_string()).expect("a number has no nul");
        match self.send_command(&[&cmd, &target, c"absolute+exact"]) {
            Ok(_) => self.status.seeks.ask(pos),
            Err(error) => error!(%error, ?pos, "mpv refused to seek"),
        }
    }

    fn get_position(&mut self) -> Option<Duration> {
        if !self.status.loaded {
            return None;
        }
        self.get_property_f64(MpvProperty::PlaybackTime)
            .ok()
            .map(Duration::from_secs_f64)
    }

    /// Loads paused. The load is finished when mpv restarts playback for
    /// its playlist entry, which [`PlayerLoaded`] reports.
    ///
    /// [`PlayerLoaded`]: niketsu_core::player::PlayerLoaded
    fn load(&mut self, source: Source, pos: Duration) {
        self.status.end();
        self.pause();
        let location = match &source {
            Source::File(path) => path.to_str().map(str::to_string),
            Source::Url(url) => Some(url.clone()),
        };
        let Some(location) = location.and_then(|location| CString::new(location).ok()) else {
            error!(?source, "mpv can not be given this source");
            return self.unload();
        };
        let cmd: CString = MpvCommand::Loadfile.into();
        let start = CString::new(format!("start={}", pos.as_secs_f64())).expect("no nul");
        match self.send_command(&[&cmd, &location, c"replace", c"0", &start]) {
            Ok(entry) => self.status.entry = entry,
            Err(error) => error!(%error, ?source, "mpv refused to load"),
        }
    }

    /// `stop` ends the file with the reason STOP, which is not reported.
    /// Idle mode and the forced window keep the window open.
    fn unload(&mut self) {
        self.status.end();
        let cmd: CString = MpvCommand::Stop.into();
        log_err!(self.send_command(&[&cmd]))
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
