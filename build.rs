use std::{env, io::Error, path::PathBuf};

use bindgen::{
    callbacks::{DeriveInfo, ParseCallbacks},
    EnumVariation,
};

fn main() -> Result<(), Error> {
    let outdir = match env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(outdir) => outdir,
    };
    let mpv = match env::var_os("MPV_DIR") {
        None => String::from("mpv"),
        Some(outdir) => outdir.into_string().unwrap(),
    };

    let bindings = bindgen::Builder::default()
        .header(format!("{mpv}/libmpv/client.h"))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .parse_callbacks(Box::new(CustomCallback))
        .default_enum_style(EnumVariation::Rust {
            non_exhaustive: true,
        })
        .layout_tests(false)
        .generate()
        .expect("Unable to generate bindings");
    bindings
        .write_to_file(PathBuf::from(&outdir).join("libmpv.rs"))
        .expect("Error writing bindgen");

    println!("cargo:rustc-link-lib=mpv");
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xcursor");
    println!("cargo:rustc-link-lib=Xrandr");
    println!("cargo:rustc-link-lib=Xi");
    println!("cargo:rustc-link-lib=vulkan");
    println!("cargo:rustc-link-arg=-lm");
    Ok(())
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
