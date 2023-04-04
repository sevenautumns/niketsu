use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use iced::{Application, Settings};
use log::{debug, info, trace};
use tokio::runtime::Runtime;
use window::MainWindow;

use crate::fs::FileDatabase;
use crate::ws::ServerMessage;

pub mod fs;
pub mod mpv;
pub mod window;
pub mod ws;

fn main() -> Result<()> {
    // pretty_env_logger::formatted_builder()
    //     .filter(Some("sync2"), log::LevelFilter::Trace)
    //     .init();
    pretty_env_logger::init();
    // trace!("Test");

    let rt = Runtime::new()?;
    rt.spawn(async move {
        let now = Instant::now();
        let mut db = FileDatabase::new();
        // db.add_search_path(Path::new("/net/index/torrent_storage/anime").to_path_buf())
        db.add_search_path(Path::new("/nix/store").to_path_buf())
            // db.add_search_path(Path::new("/run/user/125030/gvfs/sftp:host=192.168.178.2,user=autumnal/media/torrent_storage/anime").to_path_buf())
            .await;
        // db.add_search_path(Path::new("/home/autumnal").to_path_buf());
        db.update().await.unwrap();
        debug!(
            "Found files: {} in {:?}",
            db.database().read().await.len(),
            now.elapsed()
        );

        // while let Some(a) = events.next().await {
        //     debug!("{a:?}");
        // }
    });

    // MainWindow::run(Settings::default())?;
    sleep(Duration::from_secs(120));
    Ok(())

    // let profile = CString::new("profile").unwrap();
    // let default = CString::new("default").unwrap();
    // let res1 = mpv_set_option_string(ctx, profile.as_ptr(), default.as_ptr());

    // let mpv = Mpv::new();
    // mpv.set_ocs(true).unwrap();
    // mpv.init().unwrap();
    // let mut events = mpv.event_pipe();
    // // Spawn the root task

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
