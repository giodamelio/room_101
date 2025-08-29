use anyhow::{Context, Result};
use clap::Parser;
use iroh::NodeId;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use network::network_manager_task;
use webserver::webserver_task;

mod db;
mod error;
mod middleware;
mod network;
mod utils;
mod webserver;

#[derive(Parser, Debug)]
#[command(name = "room_101")]
#[command(about = "A peer-to-peer networking application")]
struct Args {
    /// Bootstrap node IDs to connect to (hex strings)
    bootstrap: Vec<String>,

    /// Start the web server
    #[arg(long)]
    start_web: bool,

    /// Path to SQLite database file
    #[arg(long)]
    db_path: String,
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

    // Only start web server if requested
    if args.start_web {
        debug!("Starting webserver task");
        let shutdown_rx = shutdown_tx.subscribe();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = webserver_task(shutdown_rx).await {
                error!("Webserver task error: {}", e);
            }
            debug!("Webserver task completed");
        }));
    }

    debug!("Starting network manager task");
    let shutdown_rx = shutdown_tx.subscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = network_manager_task(shutdown_rx, bootstrap_nodes).await {
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
