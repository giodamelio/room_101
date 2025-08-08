use iroh::{Endpoint, Watcher, protocol::Router};
use iroh_gossip::{ALPN, net::Gossip};
use miette::{IntoDiagnostic, Result};
use tokio::sync::broadcast;
use tracing::{debug, info};

use crate::db;

pub async fn task(db: db::DB, shutdown_tx: broadcast::Sender<()>) -> Result<()> {
    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<db::Identity> =
        db.select(("config", "identity")).await.into_diagnostic()?;

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
                .into_diagnostic()?;

            info!(
                public_key = %new_identity.secret_key.public(),
                "Created new identity and saved to database"
            );

            new_identity
        }
    };

    // List our current peers
    dbg!(db::get_peers(&db).await?);

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key)
        .discovery_n0()
        .bind()
        .await
        .into_diagnostic()?;

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
    let router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    info!("Accepting connections...");
    info!(
        "To connect another node: cargo run -- {}",
        node_addr.node_id
    );

    // Wait until the shudown signal comes
    let _ = shutdown_tx.subscribe().recv().await;

    // Shutdown the router
    info!("Iroh listener shutting down gracefully...");
    router.shutdown().await.into_diagnostic()?;

    debug!("Iroh listener stopped");

    Ok(())
}
