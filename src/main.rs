use std::{
    ffi::CString,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

use crate::fs::FileDatabase;

use anyhow::Result;
use futures::StreamExt;
use iced::{Application, Settings};
use log::{debug, trace};
use tokio::runtime::Runtime;
use window::MainWindow;

pub mod fs;
pub mod mpv;
pub mod window;

fn main() -> Result<()> {
    // pretty_env_logger::formatted_builder()
    //     .filter(Some("sync2"), log::LevelFilter::Trace)
    //     .init();
    pretty_env_logger::init();
    // trace!("Test");

    MainWindow::run(Settings::default())?;
    Ok(())

    // let profile = CString::new("profile").unwrap();
    // let default = CString::new("default").unwrap();
    // let res1 = mpv_set_option_string(ctx, profile.as_ptr(), default.as_ptr());

    // let mpv = Mpv::new();
    // mpv.set_ocs(true).unwrap();
    // mpv.init().unwrap();
    // let rt = Runtime::new()?;
    // let mut events = mpv.event_pipe();
    // // Spawn the root task
    // rt.spawn(async move {
    //     // let now = Instant::now();
    //     // let mut db = FileDatabase::new();
    //     // db.add_search_path(Path::new("/net/index/torrent_storage/anime").to_path_buf());
    //     // // db.add_search_path(Path::new("/home/autumnal").to_path_buf());
    //     // db.update().await.unwrap();
    //     // debug!(
    //     //     "Found files: {} in {:?}",
    //     //     db.database().len(),
    //     //     now.elapsed()
    //     // );

    //     while let Some(a) = events.next().await {
    //         debug!("{a:?}");
    //     }
    // });

    // sleep(Duration::from_secs(2));

    // // let cmd = CString::new("loadfile \"Nippontradamus.webm\"").unwrap();
    // let ret = mpv.load_file("Nippontradamus.webm");
    // debug!("{ret:?}");
    // // let res2 = mpv_command_string(ctx, cmd.as_ptr());

    // // debug!("{res1}, {res2:?}, {res3}");

    // sleep(Duration::from_secs(2));

    // let dur = mpv.get_playback_position();
    // debug!("{dur:?}");

    // let ret = mpv.set_playback_position(Duration::from_secs_f64(16.4));
    // debug!("{ret:?}");
    // sleep(Duration::from_secs(2));
    // Ok(())
}
