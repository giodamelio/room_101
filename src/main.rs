use anyhow::{Context, Result};
use clap::Parser;
use iroh::NodeId;
use ractor::Actor;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use network::network_manager_task;

mod actors;
mod db;
mod error;
mod middleware;
mod network;
mod systemd_secrets;
mod utils;
mod web_components;

#[derive(Debug, Clone)]
pub struct SystemdSecretsConfig {
    pub path: String,
    pub user_scope: bool,
}

static SYSTEMD_SECRETS_CONFIG: OnceLock<SystemdSecretsConfig> = OnceLock::new();

pub fn get_systemd_secrets_config() -> anyhow::Result<&'static SystemdSecretsConfig> {
    SYSTEMD_SECRETS_CONFIG
        .get()
        .ok_or_else(|| anyhow::anyhow!("SystemdSecretsConfig not initialized"))
}

#[derive(Parser, Debug)]
#[command(name = "room_101")]
#[command(about = "A peer-to-peer networking application")]
struct Args {
    /// Bootstrap node IDs to connect to (hex strings)
    bootstrap: Vec<String>,

    /// Start the web server
    #[arg(long)]
    start_web: bool,

    /// Web server port (default: 3000)
    #[arg(long, default_value = "3000")]
    port: u16,

    /// Path to SQLite database file
    #[arg(long)]
    db_path: String,

    /// Directory to store systemd credentials (default: /var/lib/credstore)
    #[arg(long, default_value = "/var/lib/credstore")]
    systemd_secrets_path: String,

    /// Use user-scope systemd credentials instead of system-scope (default: system-scope)
    #[arg(long)]
    systemd_user_scope: bool,
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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing first
    setup_tracing()?;

    // Parse command line arguments
    let args = Args::parse();

    info!("Starting Room 101");

    // Validate database path
    if args.db_path == ":memory:" {
        anyhow::bail!("In-memory database not allowed in production. Use a file path instead.");
    }

    // Initialize systemd secrets configuration
    SYSTEMD_SECRETS_CONFIG
        .set(SystemdSecretsConfig {
            path: args.systemd_secrets_path.clone(),
            user_scope: args.systemd_user_scope,
        })
        .map_err(|_| anyhow::anyhow!("Failed to initialize SystemdSecretsConfig"))?;

    // Check systemd-creds availability at startup
    info!(
        "SystemD secrets config: path='{}', user_scope={}",
        args.systemd_secrets_path, args.systemd_user_scope
    );
    if systemd_secrets::is_available() {
        info!("✅ systemd-creds is available - systemd secrets integration enabled");
    } else {
        warn!("⚠️ systemd-creds is NOT available - systemd secrets integration disabled");
        warn!("To enable systemd secrets, install systemd-creds (usually part of systemd >= 248)");
    }

    // Initialize the global database
    db::init_db(&args.db_path)
        .await
        .context("Failed to initialize database")?;

    // Parse bootstrap node strings into NodeIDs
    let bootstrap_nodes = if args.bootstrap.is_empty() {
        None
    } else {
        let mut nodes = Vec::new();
        for node_str in args.bootstrap {
            let node_id: NodeId = node_str
                .parse()
                .with_context(|| format!("Invalid node ID format: {node_str}"))?;
            nodes.push(node_id);
        }
        Some(nodes)
    };

    // Create shutdown broadcast channel
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Spawn tasks
    let mut tasks = Vec::new();

    // Create shared message channel for webserver integration if web server is enabled
    let (_peer_message_tx, webserver_rx) = if args.start_web {
        debug!("Creating shared peer message channel for webserver integration");
        let (tx, rx) = tokio::sync::mpsc::channel::<network::PeerMessage>(100);

        Actor::spawn(
            Some(actors::webserver::NAME.into()),
            actors::webserver::WebServerActor,
            (args.port, 10),
        )
        .await?;

        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    debug!("Starting network manager task");
    let shutdown_rx = shutdown_tx.subscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = network_manager_task(shutdown_rx, bootstrap_nodes, webserver_rx).await {
            error!("Network manager task error: {}", e);
        }
        debug!("Network manager task completed");
    }));

    debug!("All tasks started, waiting for Ctrl+C...");

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for ctrl-c")?;

    info!("Received Ctrl+C, initiating shutdown...");

    // Send shutdown signal to all tasks
    let _ = shutdown_tx.send(());

    // Wait for all tasks to complete with timeout
    let shutdown_result =
        tokio::time::timeout(Duration::from_secs(5), futures::future::join_all(tasks)).await;

    match shutdown_result {
        Ok(results) => {
            for result in results {
                if let Err(e) = result {
                    error!("Task panicked: {}", e);
                }
            }
            debug!("All tasks completed successfully");
        }
        Err(_) => {
            warn!("Tasks did not complete within 5 seconds, forcing exit");
        }
    }

    // Clean up database connection
    debug!("Closing database connection...");
    if let Err(e) = db::close_db().await {
        error!("Failed to close database cleanly: {}", e);
    }

    info!("Application shutdown complete");
    Ok(())
}
