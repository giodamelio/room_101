use anyhow::{Context, Result};
use clap::Parser;
use iroh::NodeId;
use std::{error::Error, time::Duration};
use tokio_graceful_shutdown::{SubsystemBuilder, Toplevel};
use tracing::info;
use tracing_subscriber::EnvFilter;

use network::iroh_subsystem;
use webserver::webserver_subsystem;

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
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing first
    setup_tracing()?;

    info!("Starting Room 101");

    // Database will be initialized on first use

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

    // Capture flag value before closure
    let start_web = args.start_web;

    // Create the top level
    let result = Toplevel::new(move |s| async move {
        // Only start web server if requested
        if start_web {
            s.start(SubsystemBuilder::new("webserver", |subsys| {
                webserver_subsystem(subsys)
            }));
        }

        s.start(SubsystemBuilder::new("iroh", {
            let bootstrap = bootstrap_nodes.clone();
            move |subsys| iroh_subsystem(subsys, bootstrap)
        }));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_secs(30))
    .await;

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Extract more specific error information about subsystem failures
            let errors = e.get_subsystem_errors();
            if !errors.is_empty() {
                let error_details: Vec<String> = errors
                    .iter()
                    .map(|subsys_error| {
                        let error_msg = match subsys_error.source() {
                            Some(source) => source.to_string(),
                            None => "Unknown error".to_string(),
                        };
                        format!("{}: {}", subsys_error.name(), error_msg)
                    })
                    .collect();

                anyhow::bail!(
                    "Subsystem errors occurred:\n  {}",
                    error_details.join("\n  ")
                );
            } else {
                anyhow::bail!("Application shutdown with error: {}", e);
            }
        }
    }
}
