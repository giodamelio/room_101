use anyhow::{Context, Result};

use crate::args::{PeerCommands, PeersArgs};
use crate::db::Peer;

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
                    println!("    Last seen: {}", last_seen);
                }
                if peer.age_public_key.is_some() {
                    println!("    Has Age public key: yes");
                } else {
                    println!("    Has Age public key: no");
                }
                println!("    Ticket: {}", peer.ticket);
                println!();
            }
            Ok(())
        }
    }
}
