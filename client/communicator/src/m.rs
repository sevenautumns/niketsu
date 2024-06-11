mod lib;

use clap::Parser;
use futures::prelude::*;
use lib::DHTSender;
use libp2p::multiaddr::Protocol;
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::spawn;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();
    let opt = Opt::parse();

    let mut network_client = lib::P2PClient::new(opt.room, opt.password, opt.host).await?;
    let handle = network_client.run();
    tokio::time::sleep(Duration::from_secs(1)).await;

    match opt.argument {
        // Providing a file.
        CliArgument::Provide { path, name } => {
            // Advertise oneself as a provider of the file on the DHT.
            network_client.start_providing(name.clone()).await;

            loop {
                match network_client.next_event().await {
                    Some(lib::Event::InboundRequest { request, channel }) => {
                        if request == name {
                            network_client
                                .respond_file(std::fs::read(&path)?, channel)
                                .await;
                        }
                    }
                    e => todo!("{:?}", e),
                }
            }
        }
        // Locating and getting a file.
        CliArgument::Get { name } => {
            // Locate all nodes providing the file.
            let file_content = network_client.get_file(name.clone()).await;
            match file_content {
                Ok(file) => {
                    std::io::stdout().write_all(&file)?;
                }
                Err(e) => {
                    println!("Failed to get file {e:?}");
                }
            }
        }
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[command(name = "libp2p file sharing example", version, about)]
struct Opt {
    #[arg(long)]
    room: String,

    #[arg(long)]
    password: String,

    #[clap(subcommand)]
    argument: CliArgument,

    #[arg(long, default_value_t = false)]
    host: bool,
}

#[derive(Debug, Parser)]
enum CliArgument {
    Provide {
        #[clap(long)]
        path: PathBuf,
        #[clap(long)]
        name: String,
    },
    Get {
        #[clap(long)]
        name: String,
    },
}
