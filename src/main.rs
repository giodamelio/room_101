use anyhow::Result;
use tracing_subscriber::EnvFilter;

mod actors;
mod args;
mod commands;
mod custom_serde;
mod db;
mod error;
mod network;
mod systemd_secrets;
mod utils;

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
    let args = args::args().await;

    // Validate database path
    if args.db_path == ":memory:" {
        anyhow::bail!("In-memory database not allowed in production. Use a file path instead.");
    }

    // Route to appropriate command handler
    match &args.command {
        args::Commands::Server(server_args) => commands::server::run(server_args).await,
        args::Commands::Peers(peers_args) => commands::peers::run(peers_args).await,
    }
}
