use std::env;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate_to;
use clap_complete::shells::*;

include!("src/cli.rs");

fn main() -> Result<()> {
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
