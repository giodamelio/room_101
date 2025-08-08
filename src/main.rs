use iroh::{Endpoint, NodeAddr, NodeId, SecretKey, Watcher, protocol::Router};
use iroh_gossip::{ALPN, net::Gossip};
use miette::{IntoDiagnostic, Result, diagnostic};
use poem::{IntoResponse, Route, Server, get, handler, listener::TcpListener, web::Path};
use rand::rngs;
use serde::{Deserialize, Serialize};
use std::{env, str::FromStr};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::{Datetime, Surreal};
use tracing::{debug, error, info, instrument, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Identity {
    secret_key: SecretKey,
}

impl Identity {
    #[instrument]
    fn new() -> Self {
        debug!("Generating new identity with random secret key");
        let identity = Self {
            secret_key: SecretKey::generate(rngs::OsRng),
        };
        debug!(public_key = %identity.secret_key.public(), "Generated new identity");
        identity
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Peer {
    id: NodeId,
    last_seen: Datetime,
}

async fn iroh(db: DB) -> Result<()> {
    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<Identity> = db.select(("config", "identity")).await.into_diagnostic()?;
    let identity = match identity {
        Some(identity) => {
            info!(
                public_key = %identity.secret_key.public(),
                "Loaded existing identity from database"
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

            info!(
                public_key = %new_identity.secret_key.public(),
                "Created new identity and saved to database"
            );

            new_identity
        }
    };

    // List our current peers
    dbg!(get_peers(&db).await?);

    let args: Vec<String> = env::args().collect();

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

    // If a peer node ID is provided, try to connect to it
    if args.len() > 1 {
        let peer_node_id_str = &args[1];
        if let Ok(peer_node_id) = NodeId::from_str(peer_node_id_str) {
            let peer_addr = NodeAddr::new(peer_node_id);
            info!(
                peer_node_id = %peer_node_id,
                "Attempting to connect to peer"
            );

            // Try to connect to the peer
            if let Err(e) = endpoint.connect(peer_addr, ALPN).await {
                error!(
                    peer_node_id = %peer_node_id,
                    error = %e,
                    "Failed to connect to peer"
                );
            } else {
                info!(
                    peer_node_id = %peer_node_id,
                    "Successfully connected to peer"
                );
            }
        } else {
            warn!(
                invalid_node_id = %peer_node_id_str,
                "Invalid peer node ID provided"
            );
        }
    }

    // Keep the application running
    info!("Node running and listening for connections. Press Ctrl+C to exit");
    tokio::signal::ctrl_c().await.into_diagnostic()?;

    info!("Received shutdown signal, gracefully shutting down...");
    router.shutdown().await.into_diagnostic()?;

    Ok(())
}

#[handler]
fn hello(Path(name): Path<String>) -> String {
    format!("hello: {name}")
}

async fn webserver() -> Result<()> {
    let app = Route::new().at("/hello/:name", get(hello));

    info!("Starting web ui on port 3000");

    Server::new(TcpListener::bind("0.0.0.0:3000"))
        .run(app)
        .await
        .into_diagnostic()?;

    Ok(())
}

/// Initialize simple tracing-based logging to stdout
fn setup_tracing() -> Result<()> {
    // Set up environment filter
    // Default to INFO level, but allow override with RUST_LOG environment variable
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("room_101=info,iroh=error,iroh_gossip=error"));

    // Initialize the subscriber with structured logging to stdout
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    Ok(())
}

type DB = Surreal<Db>;

async fn get_peers(db: &DB) -> Result<Vec<Peer>> {
    db.select("peer").await.into_diagnostic()
}

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Initialize tracing first
    setup_tracing()?;

    info!("Starting Room 101");

    // Connect to our database
    debug!("Connecting to SurrealDB database");
    let db: DB = Surreal::new::<SurrealKv>("room_101.db")
        .await
        .into_diagnostic()?;
    db.use_ns("room_101")
        .use_db("main")
        .await
        .map_err(|e| diagnostic!("Error connecting to database: {e}"))?;

    let iroh_task = tokio::spawn(iroh(db.clone()));
    let webserver_task = tokio::spawn(webserver());

    tokio::try_join!(iroh_task, webserver_task).into_diagnostic()?;

    info!("Node shutdown complete");

    Ok(())
}
