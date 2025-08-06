use iroh::{Endpoint, NodeAddr, NodeId, SecretKey, Watcher, protocol::Router};
use iroh_gossip::{ALPN, net::Gossip};
use miette::{IntoDiagnostic, Result, diagnostic};
use rand::rngs;
use serde::{Deserialize, Serialize};
use std::{env, str::FromStr};
use surrealdb::Surreal;
use surrealdb::engine::local::SurrealKv;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Identity {
    secret_key: SecretKey,
}

impl Identity {
    fn new() -> Self {
        Self {
            secret_key: SecretKey::generate(rngs::OsRng),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to our database
    let db = Surreal::new::<SurrealKv>("room_101.db")
        .await
        .into_diagnostic()?;
    db.use_ns("room_101")
        .use_db("main")
        .await
        .map_err(|e| diagnostic!("Error: {e}"))?;

    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<Identity> = db.select(("config", "identity")).await.into_diagnostic()?;
    let identity = match identity {
        Some(identity) => {
            println!(
                "Loaded identity from db with public key: {}",
                identity.secret_key.public()
            );

            identity
        }
        None => {
            let new_identity = Identity::new();

            // Write the new identity
            let _: Option<Identity> = db
                .create(("config", "identity"))
                .content(new_identity.clone())
                .await
                .into_diagnostic()?;

            println!(
                "Created new identity with public key: {}",
                new_identity.secret_key.public()
            );

            new_identity
        }
    };

    let args: Vec<String> = env::args().collect();

    // Create endpoint for this node
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key)
        .discovery_n0()
        .bind()
        .await
        .into_diagnostic()?;

    let mut node_addr_watcher = endpoint.node_addr();
    let node_addr = node_addr_watcher.initialized().await;
    println!("🚀 Starting Room 101 node");
    println!("Node ID: {}", node_addr.node_id);
    println!("Full address: {node_addr:#?}");

    // Create gossip instance using builder pattern
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Setup router
    let router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    println!("📡 Gossip protocol started");
    println!(
        "💡 To connect another node: cargo run -- {}",
        node_addr.node_id
    );

    // If a peer node ID is provided, try to connect to it
    if args.len() > 1 {
        let peer_node_id_str = &args[1];
        if let Ok(peer_node_id) = NodeId::from_str(peer_node_id_str) {
            let peer_addr = NodeAddr::new(peer_node_id);
            println!("🔗 Attempting to connect to peer: {peer_node_id}");

            // Try to connect to the peer
            if let Err(e) = endpoint.connect(peer_addr, ALPN).await {
                println!("⚠️  Failed to connect to peer: {e}");
            } else {
                println!("✅ Successfully connected to peer");
            }
        } else {
            println!("⚠️  Invalid peer node ID: {peer_node_id_str}");
        }
    }

    // Keep the application running
    println!("👂 Node running... Press Ctrl+C to exit");
    tokio::signal::ctrl_c().await.into_diagnostic()?;

    println!("\n🔄 Shutting down...");
    router.shutdown().await.into_diagnostic()?;

    Ok(())
}
