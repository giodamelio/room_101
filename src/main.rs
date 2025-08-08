use miette::{IntoDiagnostic, Result, diagnostic};
use surrealdb::Surreal;
use surrealdb::engine::local::SurrealKv;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};
use tracing_subscriber::EnvFilter;

mod db;
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

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Initialize tracing first
    setup_tracing()?;

    info!("Starting Room 101");

    // Connect to our database
    debug!("Connecting to SurrealDB database");
    let db: db::DB = Surreal::new::<SurrealKv>("room_101.db")
        .await
        .into_diagnostic()?;
    db.use_ns("room_101")
        .use_db("main")
        .await
        .map_err(|e| diagnostic!("Error connecting to database: {e}"))?;

    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let iroh_task = tokio::spawn(network::task(db.clone(), shutdown_tx.clone()));
    let webserver_task = tokio::spawn(webserver::task(shutdown_tx.clone()));

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        debug!("\nGot Control C...");
        let _ = shutdown_tx.send(());
    });

    let _ = tokio::try_join!(iroh_task, webserver_task).into_diagnostic()?;

    info!("Node shutdown complete");

    Ok(())
}
