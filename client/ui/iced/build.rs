fn main() {
    if std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap().eq("unix") {
        link_arg_linux();
    }
}

fn link_arg_linux() {
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
