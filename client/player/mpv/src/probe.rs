//! A probe of real mpv: which events a seek and a load produce, and what
//! the position reads at each of them. It asserts nothing; it prints what
//! mpv does, for a human to read:
//!
//! ```bash
//! cargo test -p niketsu-mpv probe -- --ignored --nocapture
//! ```
//!
//! It needs `mpv` on the path to encode its own clip (120 s, 25 fps, a
//! keyframe every 10 s, no audio).
//!
//! What mpv 0.41 (client API 2.5) did, and the adapter is built on:
//!
//! - A seek is `SEEK`, then `PLAYBACK_RESTART` once it has landed. At
//!   `SEEK` the position reads the target that was asked for, at
//!   `PLAYBACK_RESTART` where the player is. An exact seek lands on the
//!   frame after its target (61.3 gives 61.32), a keyframe seek on a
//!   keyframe, seconds from its target.
//! - Seeks in a row give one `SEEK` each and share a `PLAYBACK_RESTART`,
//!   or get one each, the earlier of which already reads the last
//!   position while `seeking` is still true.
//! - `loadfile` returns the `playlist_entry_id` of its entry, as the
//!   result of the synchronous command and in the reply to the
//!   asynchronous one. `START_FILE` and `END_FILE` carry it; everything
//!   between a `START_FILE` and the next is about that entry. An entry
//!   replaced before it started gets neither.
//! - A load is `START_FILE`, `FILE_LOADED`, `SEEK` when `start=` was
//!   given (also `start=0`), `PLAYBACK_RESTART`. The position reads the
//!   start from `FILE_LOADED` on, and no property can be read or set
//!   before it.
//! - A load that fails is `START_FILE`, then `END_FILE` with the reason
//!   ERROR. The end of a file is `END_FILE` with EOF, a replaced or
//!   stopped one `END_FILE` with STOP.
//! - A playlist file ends with the reason REDIRECT and names the first
//!   entry put in its place, which starts next, without the `start=` of
//!   the load.
//! - The user's seeks look the same as the app's: relative ones and the
//!   percent seeks of the seek bar land on keyframes, absolute ones
//!   exactly. A frame step is no seek.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait, Source};

use crate::bindings::*;
use crate::event::{MpvEvent, MpvEventTrait};
use crate::{Mpv, MpvProperty};

struct Probe {
    ctx: *mut mpv_handle,
    began: Instant,
}

impl Probe {
    fn new() -> Self {
        let ctx = unsafe { mpv_create() };
        assert!(!ctx.is_null());
        let probe = Self {
            ctx,
            began: Instant::now(),
        };
        for (name, value) in [
            ("config", "no"),
            ("vo", "null"),
            ("ao", "null"),
            ("idle", "yes"),
            ("keep-open", "no"),
            ("pause", "yes"),
        ] {
            let (name, value) = (cstring(name), cstring(value));
            let ret = unsafe { mpv_set_option_string(ctx, name.as_ptr(), value.as_ptr()) };
            assert_eq!(ret, 0, "option {name:?}");
        }
        assert_eq!(unsafe { mpv_initialize(ctx) }, 0);
        probe
    }

    fn section(&mut self, title: &str) {
        println!("\n== {title}");
        self.began = Instant::now();
    }

    fn note(&self, what: &str) {
        println!(
            "{:>7.1} ms  {what:<44} {}",
            self.began.elapsed().as_secs_f64() * 1000.0,
            self.state()
        );
    }

    fn state(&self) -> String {
        let time = match self.double("playback-time") {
            Ok(time) => format!("{time:.3}"),
            Err(error) => format!("err {error}"),
        };
        let seeking = match self.flag("seeking") {
            Ok(seeking) => seeking.to_string(),
            Err(error) => format!("err {error}"),
        };
        format!("playback-time={time} seeking={seeking}")
    }

    fn double(&self, name: &str) -> Result<f64, c_int> {
        let name = cstring(name);
        let mut data = 0f64;
        let ret = unsafe {
            mpv_get_property(
                self.ctx,
                name.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                &mut data as *mut f64 as *mut c_void,
            )
        };
        if ret < 0 { Err(ret) } else { Ok(data) }
    }

    fn flag(&self, name: &str) -> Result<bool, c_int> {
        let name = cstring(name);
        let mut data: c_int = 0;
        let ret = unsafe {
            mpv_get_property(
                self.ctx,
                name.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                &mut data as *mut c_int as *mut c_void,
            )
        };
        if ret < 0 { Err(ret) } else { Ok(data != 0) }
    }

    fn set_double(&self, name: &str, value: f64) {
        let cname = cstring(name);
        let mut value_in = value;
        let ret = unsafe {
            mpv_set_property(
                self.ctx,
                cname.as_ptr(),
                mpv_format::MPV_FORMAT_DOUBLE,
                &mut value_in as *mut f64 as *mut c_void,
            )
        };
        self.note(&format!("set {name}={value} -> {ret}"));
    }

    fn set_flag(&self, name: &str, value: bool) {
        let cname = cstring(name);
        let mut value_in = value as c_int;
        let ret = unsafe {
            mpv_set_property(
                self.ctx,
                cname.as_ptr(),
                mpv_format::MPV_FORMAT_FLAG,
                &mut value_in as *mut c_int as *mut c_void,
            )
        };
        self.note(&format!("set {name}={value} -> {ret}"));
    }

    /// Runs a command and prints what it returns.
    fn command(&self, args: &[&str]) {
        let args_c: Vec<CString> = args.iter().map(|arg| cstring(arg)).collect();
        let mut pointers: Vec<*const c_char> = args_c.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(std::ptr::null());
        let mut result: mpv_node = unsafe { std::mem::zeroed() };
        let ret = unsafe { mpv_command_ret(self.ctx, pointers.as_mut_ptr(), &mut result) };
        let returned = if ret < 0 {
            String::new()
        } else {
            let text = unsafe { node(&result) };
            unsafe { mpv_free_node_contents(&mut result) };
            text
        };
        self.note(&format!("command {args:?} -> {ret} {returned}"));
    }

    /// Runs a command the way the adapter does and leaves the reply to
    /// the event log.
    fn command_async(&self, userdata: u64, args: &[&str]) {
        let args_c: Vec<CString> = args.iter().map(|arg| cstring(arg)).collect();
        let mut pointers: Vec<*const c_char> = args_c.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(std::ptr::null());
        let ret = unsafe { mpv_command_async(self.ctx, userdata, pointers.as_mut_ptr()) };
        self.note(&format!("async #{userdata} {args:?} -> {ret}"));
    }

    /// Prints every event of the next `window`.
    fn watch(&self, window: Duration) {
        let until = Instant::now() + window;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let event = unsafe { &*mpv_wait_event(self.ctx, left.as_secs_f64()) };
            if let Some(text) = unsafe { describe(event) } {
                self.note(&text);
            }
        }
        self.note("(window over)");
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        unsafe { mpv_terminate_destroy(self.ctx) };
    }
}

fn cstring(text: &str) -> CString {
    CString::new(text).expect("no nul in probe strings")
}

unsafe fn describe(event: &mpv_event) -> Option<String> {
    unsafe {
        Some(match event.event_id {
            mpv_event_id::MPV_EVENT_NONE => return None,
            mpv_event_id::MPV_EVENT_START_FILE => {
                let data = &*(event.data as *const mpv_event_start_file);
                format!("START_FILE entry={}", data.playlist_entry_id)
            }
            mpv_event_id::MPV_EVENT_END_FILE => {
                let data = &*(event.data as *const mpv_event_end_file);
                format!(
                    "END_FILE entry={} reason={:?} error={} insert={}+{}",
                    data.playlist_entry_id,
                    data.reason,
                    data.error,
                    data.playlist_insert_id,
                    data.playlist_insert_num_entries
                )
            }
            mpv_event_id::MPV_EVENT_COMMAND_REPLY => {
                let data = &*(event.data as *const mpv_event_command);
                format!(
                    "COMMAND_REPLY #{} error={} {}",
                    event.reply_userdata,
                    event.error,
                    node(&data.result)
                )
            }
            other => format!("{other:?}")
                .trim_start_matches("MPV_EVENT_")
                .to_string(),
        })
    }
}

unsafe fn node(node_in: &mpv_node) -> String {
    unsafe {
        match node_in.format {
            mpv_format::MPV_FORMAT_NONE => "none".into(),
            mpv_format::MPV_FORMAT_INT64 => node_in.u.int64.to_string(),
            mpv_format::MPV_FORMAT_FLAG => (node_in.u.flag != 0).to_string(),
            mpv_format::MPV_FORMAT_DOUBLE => node_in.u.double_.to_string(),
            mpv_format::MPV_FORMAT_STRING => {
                format!("{:?}", CStr::from_ptr(node_in.u.string))
            }
            mpv_format::MPV_FORMAT_NODE_MAP => {
                let list = &*node_in.u.list;
                let entries: Vec<String> = (0..list.num as usize)
                    .map(|i| {
                        let key = CStr::from_ptr(*list.keys.add(i)).to_string_lossy();
                        format!("{key}: {}", node(&*list.values.add(i)))
                    })
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            other => format!("{other:?}"),
        }
    }
}

/// Encodes the clip with mpv itself, once.
fn clip() -> String {
    let path: PathBuf = std::env::temp_dir().join("niketsu-mpv-probe.mkv");
    if !path.exists() {
        let status = Command::new("mpv")
            .args([
                "--no-config",
                "--no-audio",
                "av://lavfi:testsrc=duration=120:rate=25",
                "--ovc=libx264",
                "--ovcopts=g=250,keyint_min=250,sc_threshold=0,preset=ultrafast",
                "--of=matroska",
            ])
            .arg(format!("--o={}", path.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("mpv is on the path");
        assert!(status.success());
    }
    path.to_str().expect("utf-8 temp dir").to_string()
}

const SHORT: Duration = Duration::from_millis(600);

#[test]
#[ignore = "prints what real mpv does; run by hand"]
fn probe() {
    let clip = clip();
    let mut mpv = Probe::new();

    mpv.section("a position set with no file");
    mpv.set_double("playback-time", 5.0);
    mpv.watch(Duration::from_millis(100));

    mpv.section("loadfile start=33.3 (paused), synchronous command");
    mpv.command(&["loadfile", &clip, "replace", "0", "start=33.3"]);
    mpv.watch(SHORT);

    mpv.section("paused: set playback-time=61.3 (not on a keyframe)");
    mpv.set_double("playback-time", 61.3);
    mpv.watch(SHORT);

    mpv.section("paused: seek 71.3 absolute+exact, as the adapter seeks");
    mpv.command(&["seek", "71.3", "absolute+exact"]);
    mpv.watch(SHORT);

    mpv.section("paused: two sets in a row, 12.2 then 84.4");
    mpv.set_double("playback-time", 12.2);
    mpv.set_double("playback-time", 84.4);
    mpv.watch(SHORT);

    mpv.section("paused: set playback-time to where it is");
    let here = mpv.double("playback-time").unwrap_or_default();
    mpv.set_double("playback-time", here);
    mpv.watch(SHORT);

    mpv.section("playing: unpause, then set playback-time=27.7");
    mpv.set_flag("pause", false);
    mpv.watch(Duration::from_millis(300));
    mpv.set_double("playback-time", 27.7);
    mpv.watch(SHORT);

    mpv.section("playing: the user's seeks");
    for args in [
        &["seek", "5", "relative"][..],
        &["seek", "-5", "relative"],
        &["seek", "5", "relative+exact"],
        &["seek", "47.3", "absolute"],
        &["seek", "47.3", "absolute+keyframes"],
        &["seek", "50", "absolute-percent"],
        &["seek", "50", "absolute-percent+keyframes"],
        &["frame-step"],
    ] {
        println!();
        mpv.command(args);
        mpv.watch(Duration::from_millis(400));
    }

    mpv.section("playing: a user seek right after one of ours");
    mpv.set_flag("pause", false);
    mpv.set_double("playback-time", 15.5);
    mpv.command(&["seek", "70", "absolute"]);
    mpv.watch(SHORT);

    mpv.section("loadfile start=0, asynchronous command");
    mpv.command_async(7, &["loadfile", &clip, "replace", "0", "start=0"]);
    mpv.watch(SHORT);

    mpv.section("loadfile without start");
    mpv.command_async(8, &["loadfile", &clip, "replace"]);
    mpv.watch(SHORT);

    mpv.section("two loads in a row, start=20 then start=40");
    mpv.command_async(9, &["loadfile", &clip, "replace", "0", "start=20"]);
    mpv.command_async(10, &["loadfile", &clip, "replace", "0", "start=40"]);
    mpv.watch(SHORT);

    mpv.section("a set right after a load, before it is loaded");
    mpv.command_async(11, &["loadfile", &clip, "replace", "0", "start=20"]);
    mpv.set_double("playback-time", 55.5);
    mpv.watch(SHORT);

    mpv.section("a load that fails");
    mpv.command_async(12, &["loadfile", "/nonexistent/niketsu.mkv", "replace"]);
    mpv.watch(SHORT);

    mpv.section("the synthetic source, start=30");
    mpv.command_async(
        13,
        &[
            "loadfile",
            "av://lavfi:testsrc=duration=120:rate=25",
            "replace",
            "0",
            "start=30",
        ],
    );
    mpv.watch(SHORT);

    mpv.section("a playlist file that holds the clip, start=25");
    let list = std::env::temp_dir().join("niketsu-mpv-probe.m3u");
    std::fs::write(&list, format!("{clip}\n")).expect("the temp dir is writable");
    let list = list.to_str().expect("utf-8 temp dir");
    mpv.command_async(17, &["loadfile", list, "replace", "0", "start=25"]);
    mpv.watch(SHORT);

    mpv.section("the end of the file, playing");
    mpv.command_async(14, &["loadfile", &clip, "replace", "0", "start=119"]);
    mpv.set_flag("pause", false);
    mpv.watch(Duration::from_millis(2000));

    mpv.section("stop");
    mpv.command_async(15, &["loadfile", &clip, "replace", "0", "start=10"]);
    mpv.watch(SHORT);
    mpv.command_async(16, &["stop"]);
    mpv.watch(SHORT);
}

/// What the adapter reports within `within`, if anything.
fn next(mpv: &mut Mpv, within: Duration) -> Option<MediaPlayerEvent> {
    let until = Instant::now() + within;
    while Instant::now() < until {
        let event = mpv.event_pipe.check_for_event();
        if matches!(event, MpvEvent::MpvNone(_)) {
            std::thread::sleep(Duration::from_millis(1));
        } else if let Some(event) = event.process(mpv) {
            return Some(event);
        }
    }
    None
}

#[track_caller]
fn assert_at(mpv: &mut Mpv, secs: f64) {
    let at = mpv.get_position().expect("the player has a position");
    let off = at.abs_diff(Duration::from_secs_f64(secs));
    assert!(off < Duration::from_millis(200), "at {at:?}, not {secs}");
}

/// The user of the player does something in it, which the adapter did
/// not ask for.
fn user(mpv: &Mpv, args: &[&std::ffi::CStr]) {
    mpv.send_command(args).expect("mpv takes the command");
}

/// The adapter on real mpv: it reports a load once, keeps quiet about the
/// seeks it was asked for, and reports the user's.
#[test]
#[ignore = "needs mpv on the path to encode its clip; run by hand"]
fn the_adapter_tells_its_users_seeks_from_the_apps() {
    use MediaPlayerEvent::*;
    let clip = Source::File(clip().into());
    let secs = Duration::from_secs_f64;
    let mut mpv = Mpv::headless().expect("mpv starts");
    assert_eq!(mpv.get_position(), None);
    assert_eq!(mpv.is_paused(), None);

    mpv.load(clip.clone(), secs(33.3));
    assert_eq!(mpv.get_position(), None, "a loading player has no position");
    assert!(matches!(next(&mut mpv, SHORT), Some(Loaded(_))));
    assert_at(&mut mpv, 33.3);
    assert_eq!(mpv.is_paused(), Some(true));

    mpv.set_position(secs(61.3));
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "the app's seek is reported"
    );
    assert_at(&mut mpv, 61.3);

    mpv.set_position(secs(12.2));
    mpv.set_position(secs(84.4));
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "the app's seeks are reported"
    );
    assert_at(&mut mpv, 84.4);

    mpv.start();
    mpv.set_position(secs(27.7));
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "the app's seek is reported"
    );
    assert_eq!(mpv.is_paused(), Some(false));

    // a keyframe seek lands on a keyframe, ten seconds apart in the clip
    user(&mpv, &[c"seek", c"5", c"relative"]);
    assert!(matches!(next(&mut mpv, SHORT), Some(PositionChange(_))));
    assert_at(&mut mpv, 40.0);
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "one seek is reported twice"
    );

    mpv.set_position(secs(15.5));
    user(&mpv, &[c"seek", c"70", c"absolute"]);
    assert!(matches!(next(&mut mpv, SHORT), Some(PositionChange(_))));
    assert_at(&mut mpv, 70.0);
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "one seek is reported twice"
    );

    mpv.set_property(MpvProperty::Pause, true.into())
        .expect("mpv pauses");
    assert!(matches!(next(&mut mpv, SHORT), Some(Pause(_))));

    // the first of two loads never plays, and nothing of it is reported
    mpv.load(clip.clone(), secs(20.0));
    mpv.load(clip.clone(), secs(40.0));
    assert!(matches!(next(&mut mpv, SHORT), Some(Loaded(_))));
    assert_at(&mut mpv, 40.0);
    assert!(
        next(&mut mpv, SHORT).is_none(),
        "a replaced load is reported"
    );

    mpv.load(Source::File("/nonexistent/niketsu.mkv".into()), secs(0.0));
    assert!(next(&mut mpv, SHORT).is_none(), "a failed load is reported");
    assert_eq!(mpv.get_position(), None);
    assert_eq!(mpv.is_paused(), None);

    mpv.load(clip.clone(), secs(119.0));
    assert!(matches!(next(&mut mpv, SHORT), Some(Loaded(_))));
    mpv.start();
    let end = next(&mut mpv, Duration::from_secs(3));
    assert!(
        matches!(end, Some(FileEnd(_))),
        "the file ends with {end:?}"
    );
    assert_eq!(mpv.get_position(), None);

    mpv.load(clip, secs(10.0));
    assert!(matches!(next(&mut mpv, SHORT), Some(Loaded(_))));
    mpv.unload();
    assert!(next(&mut mpv, SHORT).is_none(), "an unload is reported");
    assert_eq!(mpv.get_position(), None);
}
