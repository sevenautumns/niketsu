use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use bindgen::callbacks::{DeriveInfo, ParseCallbacks};
use bindgen::EnumVariation;
use clap::CommandFactory;
use clap_complete::generate_to;
use clap_complete::shells::*;

include!("src/cli.rs");

fn main() -> Result<()> {
    link_mpv()?;
    cli()?;

    Ok(())
}

fn cli() -> Result<()> {
    let outdir = env::var_os("OUT_DIR").context("No outdir found")?;

    let mut cmd = Args::command();
    let bin = "niketsu";

    let path = generate_to(Bash, &mut cmd, bin, &outdir)?;
    println!("cargo:warning=completion file is generated: {:?}", path);
    let path = generate_to(Fish, &mut cmd, bin, &outdir)?;
    println!("cargo:warning=completion file is generated: {:?}", path);
    let path = generate_to(Zsh, &mut cmd, bin, &outdir)?;
    println!("cargo:warning=completion file is generated: {:?}", path);
    Ok(())
}

fn link_mpv() -> Result<()> {
    let outdir = env::var_os("OUT_DIR").context("No outdir found")?;

    let bindings = bindgen::Builder::default()
        .header_contents("mpv.h", "#include <mpv/client.h>")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
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

    if env::var("CARGO_CFG_TARGET_FAMILY").unwrap().eq("unix") {
        link_arg_linux();
    } else {
        link_arg_windows();
    }

    Ok(())
}

fn link_arg_windows() {
    let source = env::var("MPV_SOURCE").expect("env var `MPV_SOURCE` not set");
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
