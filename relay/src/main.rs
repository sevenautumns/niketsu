use anyhow::Result;
use clap::Parser;
use libp2p::identity::Keypair;
use niketsu_relay::cli::{Args, setup_logger};
use niketsu_relay::config::Config;
use niketsu_relay::relay;
use tracing::{info, warn};

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

    if let Some(port) = args.port {
        config.port = port;
    }
    config.keypair = Some(keypair.to_protobuf_encoding()?);
    if let Err(error) = config.save() {
        warn!(%error, "Failed to save config to file");
    }

    let mut relay = relay::new(config)?;
    info!(
        peer_id = %relay.peer_id(),
        "Finished initialization. Now receiving requests for relay"
    );
    relay.run().await;

    Ok(())
}
