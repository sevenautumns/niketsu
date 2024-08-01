use anyhow::Result;
use log::{info, warn};

use crate::cli::{setup_logger, Args};
use crate::config::Config;
use clap::Parser;
use libp2p::identity::Keypair;
mod cli;
mod config;
mod relay;

#[tokio::main]
async fn main() -> Result<()> {
    let mut config = Config::load_or_default();
    let args = Args::parse();
    setup_logger(args.log_level.into())?;

    let keypair: Keypair;
    if let Some(kp) = config.keypair {
        keypair = Keypair::from_protobuf_encoding(kp.as_slice())?;
    } else {
        keypair = Keypair::generate_ed25519();
    }

    if let Some(ipv6) = args.ipv6 {
        config.ipv6 = ipv6;
    }
    if let Some(port) = args.port {
        config.port = port;
    }
    config.keypair = Some(keypair.to_protobuf_encoding()?);
    if let Err(e) = config.save() {
        warn!("Failed to save config to file: {e:?}");
    }

    let mut relay = relay::new(config)?;
    info!(
        "Finished initialization. Now receiving requests for relay: {:?}",
        relay.peer_id()
    );
    relay.run().await;

    Ok(())
}
