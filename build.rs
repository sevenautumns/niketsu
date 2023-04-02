use std::{env, io::Error, path::PathBuf};

use bindgen::EnumVariation;

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

    Ok(())
}
