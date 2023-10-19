use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate_to;
use clap_complete::shells::*;
use clap_mangen::Man;

include!("src/cli.rs");

fn main() -> Result<()> {
    let outdir: PathBuf = env::var_os("OUT_DIR").context("No outdir found")?.into();

    let mut cmd = Args::command();
    let bin = "niketsu";

    generate_to(Bash, &mut cmd, bin, &outdir)?;
    generate_to(Fish, &mut cmd, bin, &outdir)?;
    generate_to(Zsh, &mut cmd, bin, &outdir)?;

    let man = Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;

    std::fs::write(outdir.join("niketsu.1"), buffer)?;

    Ok(())
}
