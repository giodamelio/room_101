use anyhow::{Context, Result};
use std::{env, time::Duration};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use heartbeat::{HeartbeatMessage, heartbeat_task};
use network::{IrohMessage, iroh_task};
use webserver::{WebServerMessage, webserver_task};

mod db;
mod error;
mod heartbeat;
mod middleware;
mod network;
mod webserver;

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

fn get_database_url() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| "surrealkv://room_101.db".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing first
    setup_tracing()?;

    info!("Starting Room 101");

    // Connect to our database
    tracing::debug!("Connecting to SurrealDB database");
    let db_url = get_database_url();
    let db = db::connect(&db_url)
        .await
        .context("Failed to connect to database")?;

    // Initialize database schema
    tracing::debug!("Initializing database schema");
    db::initialize_database(&db)
        .await
        .context("Failed to initialize database schema")?;

    // Create channels for each task
    let (heartbeat_tx, heartbeat_rx) = mpsc::channel::<HeartbeatMessage>(32);
    let (webserver_tx, webserver_rx) = mpsc::channel::<WebServerMessage>(32);
    let (iroh_tx, iroh_rx) = mpsc::channel::<IrohMessage>(32);

    // Spawn tasks
    let heartbeat_handle = tokio::spawn(heartbeat_task(
        db.clone(),
        Duration::from_secs(2),
        heartbeat_rx,
    ));

    let webserver_handle = tokio::spawn(webserver_task(db.clone(), webserver_rx));

    let iroh_handle = tokio::spawn(iroh_task(db.clone(), iroh_rx));

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await.unwrap();
    info!("Received Ctrl+C, initiating graceful shutdown...");

    // Send shutdown messages to all tasks
    let _ = heartbeat_tx.send(HeartbeatMessage::Shutdown).await;
    let _ = webserver_tx.send(WebServerMessage::Shutdown).await;
    let _ = iroh_tx.send(IrohMessage::Shutdown).await;

    // Wait for all tasks to complete
    let _ = tokio::try_join!(heartbeat_handle, webserver_handle, iroh_handle);

    info!("Node shutdown complete");

    Ok(())
}
