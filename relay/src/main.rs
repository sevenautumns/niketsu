use anyhow::Result;
use clap::Parser;
use niketsu_relay::cli::{Args, setup_logger};
use niketsu_relay::config::Config;
use niketsu_relay::relay::Relay;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    setup_logger(args.log_level.into())?;

    let mut config = Config::load_or_default();
    if let Some(port) = args.port {
        config.port = port;
    }
    let keypair = config.identity()?;
    if let Err(error) = config.save() {
        warn!(%error, "Failed to save config to file");
    }

    let mut relay = Relay::new(keypair, config.port)?;
    info!(
        peer_id = %relay.peer_id(),
        "Finished initialization. Now receiving requests for relay"
    );
    relay.run().await;

    Ok(())
}
