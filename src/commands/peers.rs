use anyhow::{Context, Result, ensure};
use chrono_humanize::HumanTime;

use crate::args::{PeerCommands, PeersArgs};
use crate::db::{Identity, Peer};

#[allow(clippy::print_stdout)] // CLI output is appropriate here
pub async fn run(peers_args: &PeersArgs) -> Result<()> {
    match &peers_args.command {
        PeerCommands::List => {
            let peers = Peer::list()
                .await
                .context("Failed to retrieve peers from database")?;

            if peers.is_empty() {
                println!("No peers found in database");
                return Ok(());
            }

            println!("Found {} peer(s):", peers.len());
            for peer in peers {
                println!("  Node ID: {}", peer.node_id);
                if let Some(hostname) = &peer.hostname {
                    println!("    Hostname: {}", hostname);
                }
                if let Some(last_seen) = &peer.last_seen {
                    let human_time = HumanTime::from(*last_seen);
                    println!("    Last seen: {} ({})", human_time, last_seen);
                } else {
                    println!("    Last seen: Never");
                }
                if peer.age_public_key.is_some() {
                    println!("    Has Age public key: YES");
                } else {
                    println!("    Has Age public key: NO");
                }
                println!("    Ticket: {}", peer.ticket);
                println!("    Node Addr: {:#?}", peer.ticket.node_addr());
                println!();
            }
            Ok(())
        }
        PeerCommands::Add { ticket } => {
            let identity = Identity::get().await.context("Failed to get identity")?;
            let our_node_id = identity.id();
            let ticket_node_id = ticket.node_addr().node_id;

            ensure!(
                our_node_id != ticket_node_id,
                "Cannot add yourself as a peer"
            );

            let result = Peer::insert_from_ticket(ticket.clone())
                .await
                .context("Failed to add peer from ticket")?;

            match result {
                Some(peer) => {
                    println!("Successfully added peer:");
                    println!("  Node ID: {}", peer.node_id);
                    println!("  Ticket: {}", peer.ticket);
                }
                None => {
                    println!("Peer already exists or failed to add");
                }
            }
            Ok(())
        }
    }
}
