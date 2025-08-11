use anyhow::{Context, Result};
use std::{env, time::Duration};
use tokio_graceful_shutdown::{SubsystemBuilder, Toplevel};
use tracing::info;
use tracing_subscriber::EnvFilter;

use heartbeat::heartbeat_subsystem;
use network::iroh_subsystem;
use webserver::webserver_subsystem;

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

    // Create the top level
    Toplevel::new(|s| async move {
        s.start(SubsystemBuilder::new("heartbeat", {
            let db = db.clone();
            move |subsys| heartbeat_subsystem(subsys, db, Duration::from_secs(2))
        }));

        s.start(SubsystemBuilder::new("webserver", {
            let db = db.clone();
            move |subsys| webserver_subsystem(subsys, db)
        }));

        s.start(SubsystemBuilder::new("iroh", {
            let db = db.clone();
            move |subsys| iroh_subsystem(subsys, db)
        }));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_secs(30))
    .await
    .map_err(Into::into)
}
