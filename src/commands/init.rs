use anyhow::Result;
use iroh::{Endpoint, Watcher};
use iroh_base::ticket::NodeTicket;
use tracing::{error, trace};

use crate::db::Identity;

#[allow(clippy::print_stdout)] // CLI output is appropriate here
pub async fn run() -> Result<()> {
    if Identity::get().await.is_ok() {
        println!("Identity already exists");
    } else {
        println!("Generating new Identity");
    }

    let identity = Identity::get_or_generate().await?;

    println!();
    println!("Node ID: {}", identity.id());
    println!("Age Public Key: {}", identity.age_key.to_public());

    println!();
    println!("Finding best Iroh Relay...");

    let endpoint = Endpoint::builder()
        .secret_key(identity.clone().secret_key)
        .discovery_n0()
        .bind()
        .await?;

    let _relay = endpoint.home_relay().initialized().await;
    let addr = endpoint.node_addr().initialized().await;
    let ticket = NodeTicket::new(addr.clone());
    println!("Iroh Ticket: {ticket}");

    // Write ticket to file if specified
    if let crate::args::Commands::Init(init_args) = &crate::args::args().await.command
        && let Some(ref ticket_path) = init_args.ticket_file
    {
        match crate::utils::write_ticket_to_file(&ticket, ticket_path).await {
            Ok(()) => trace!("Successfully wrote ticket to file '{ticket_path:?}'"),
            Err(e) => error!("Failed to write ticket to file '{ticket_path:?}': {e:?}"),
        }
    }

    Ok(())
}
