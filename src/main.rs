use std::{ffi::CString, thread::sleep, time::Duration};

use crate::mpv::{bindings::*, event::MpvEventPipe, Mpv};

use anyhow::Result;
use futures::StreamExt;
use iced::{Application, Settings};
use tokio::runtime::Runtime;
use window::MainWindow;

pub mod mpv;
pub mod window;

fn main() -> Result<()> {
    // MainWindow::run(Settings::default())?;
    // Ok(())
    unsafe {
        let ctx = mpv_create();
        // let profile = CString::new("profile").unwrap();
        // let default = CString::new("default").unwrap();
        // let res1 = mpv_set_option_string(ctx, profile.as_ptr(), default.as_ptr());
        let osc = CString::new("osc").unwrap();
        let res1 = mpv_set_property(
            ctx,
            osc.as_ptr(),
            mpv_format::MPV_FORMAT_FLAG,
            &mut 1 as *mut _ as *mut _,
        );
        let res3 = mpv_initialize(ctx);

        let rt = Runtime::new()?;
        let mut events = MpvEventPipe::new(ctx);
        // Spawn the root task
        rt.spawn(async move {
            while let Some(a) = events.next().await {
                println!("{a:?}");
            }
        });

        let cmd = CString::new("loadfile \"Nippontradamus.webm\"").unwrap();
        let res2 = mpv_command_string(ctx, cmd.as_ptr());

        println!("{res1}, {res2}, {res3}");

        sleep(Duration::from_secs(5));

        let mpv = Mpv::new(ctx);

        let dur = mpv.get_playback_position();
        println!("{dur:?}");

        mpv.set_playback_position(16.2).unwrap();
        sleep(Duration::from_secs(5));

        mpv_terminate_destroy(ctx);
    }
    Ok(())
}
