use std::path::PathBuf;

use anyhow::{Context, Result};
use bindgen::callbacks::{DeriveInfo, ParseCallbacks};
use bindgen::EnumVariation;

fn main() -> Result<()> {
    let outdir = std::env::var_os("OUT_DIR").context("No outdir found")?;

    let bindings = bindgen::Builder::default()
        .header_contents("mpv.h", "#include <mpv/client.h>")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(CustomCallback))
        .default_enum_style(EnumVariation::Rust {
            non_exhaustive: true,
        })
        .layout_tests(true)
        .generate()
        .context("Unable to generate bindings")?;
    bindings
        .write_to_file(PathBuf::from(&outdir).join("libmpv.rs"))
        .context("Error writing bindgen")?;

    if std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap().eq("unix") {
        link_arg_linux();
    } else {
        link_arg_windows();
    }

    Ok(())
}

fn link_arg_windows() {
    let source = std::env::var("MPV_SOURCE").expect("env var `MPV_SOURCE` not set");
    println!("cargo:rustc-link-search={source}");
    println!("cargo:rustc-link-lib=mpv");
}

fn link_arg_linux() {
    println!("cargo:rustc-link-lib=mpv");
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xcursor");
    println!("cargo:rustc-link-lib=Xrandr");
    println!("cargo:rustc-link-lib=Xi");
    println!("cargo:rustc-link-lib=vulkan");
    println!("cargo:rustc-link-lib=wayland-egl");
    println!("cargo:rustc-link-lib=wayland-client");
    println!("cargo:rustc-link-lib=wayland-server");
    println!("cargo:rustc-link-arg=-lm");
}

#[derive(Debug)]
struct CustomCallback;

impl ParseCallbacks for CustomCallback {
    fn add_derives(&self, info: &DeriveInfo<'_>) -> Vec<String> {
        if info.name.eq("mpv_error") {
            return vec!["FromPrimitive".into(), "Display".into(), "Error".into()];
        }
        vec![]
    }
}
