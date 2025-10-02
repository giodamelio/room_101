use anyhow::Result;

mod actors;
mod args;
mod commands;
mod custom_serde;
mod db;
mod network;
mod tracing;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing first
    tracing::setup_tracing()?;

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
        args::Commands::Status => commands::status::run().await,
        args::Commands::Init(_) => commands::init::run().await,
        args::Commands::Audit(audit_args) => commands::audit::run(audit_args).await,
    }
}
