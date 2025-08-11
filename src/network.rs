use anyhow::{Context, Result};
use iroh::{Endpoint, Watcher, protocol::Router};
use iroh_gossip::{ALPN, net::Gossip};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::db;

#[derive(Debug)]
pub enum IrohMessage {
    Shutdown,
}

pub async fn iroh_task(db: db::DB, mut rx: mpsc::Receiver<IrohMessage>) -> Result<()> {
    info!("Iroh task started");

    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<db::Identity> = db
        .select(("config", "identity"))
        .await
        .context("Failed to load identity from database")?;

    let identity = match identity {
        Some(identity) => {
            info!(
                public_key = %identity.secret_key.public(),
                "Loaded existing identity from database"
            );
            identity
        }
        None => {
            let new_identity = db::Identity::new();

            // Write the new identity
            let _: Option<db::Identity> = db
                .create(("config", "identity"))
                .content(new_identity.clone())
                .await
                .context("Failed to save identity to database")?;

            info!(
                public_key = %new_identity.secret_key.public(),
                "Created new identity and saved to database"
            );

            new_identity
        }
    };

    // List our current peers
    dbg!(db::Peer::list(&db).await?);

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key)
        .discovery_n0()
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    let mut node_addr_watcher = endpoint.node_addr();
    let node_addr = node_addr_watcher.initialized().await;
    info!(
        node_id = %node_addr.node_id,
        addresses = ?node_addr.direct_addresses,
        relay_url = ?node_addr.relay_url,
        "Node endpoint initialized"
    );

    // Create gossip instance using builder pattern
    debug!("Spawning gossip protocol handler");
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Setup router
    debug!("Setting up protocol router");
    let _router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    info!("Accepting connections...");
    info!(
        "To connect another node: cargo run -- {}",
        node_addr.node_id
    );

    // Wait for shutdown message
    while let Some(message) = rx.recv().await {
        match message {
            IrohMessage::Shutdown => {
                info!("Iroh received shutdown signal");
                break;
            }
        }
    }

    info!("Iroh task stopped");
    Ok(())
}

