use anyhow::{Context, Result};
use heartbeat::{HeartbeatActor, HeartbeatArgs};
use management::{ActorType, Management, ManagementMessage};
use network::{IrohActor, IrohArgs};
use ractor::Actor;
use std::{env, time::Duration};
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;
use webserver::{WebServerActor, WebServerArgs};

mod db;
mod error;
mod heartbeat;
mod management;
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
    debug!("Connecting to SurrealDB database");
    let db_url = get_database_url();
    let db = db::connect(&db_url)
        .await
        .context("Failed to connect to database")?;

    // Initialize database schema
    debug!("Initializing database schema");
    db::initialize_database(&db)
        .await
        .context("Failed to initialize database schema")?;

    // Start actors using Ractor
    let heartbeat_args = HeartbeatArgs {
        db: db.clone(),
        interval: Duration::from_secs(2),
    };
    let (heartbeat_actor, heartbeat_handle) =
        Actor::spawn(None, HeartbeatActor::new(), heartbeat_args)
            .await
            .context("Failed to start heartbeat actor")?;

    let webserver_args = WebServerArgs { db: db.clone() };
    let (webserver_actor, webserver_handle) =
        Actor::spawn(None, WebServerActor::new(), webserver_args)
            .await
            .context("Failed to start webserver actor")?;

    let iroh_args = IrohArgs { db: db.clone() };
    let (iroh_actor, iroh_handle) = Actor::spawn(None, IrohActor::new(), iroh_args)
        .await
        .context("Failed to start iroh actor")?;

    // Start up the management actor and tell it about the other actors
    let (management_actor, management_handle) = Actor::spawn(None, Management::new(), ())
        .await
        .context("Failed to start management actor")?;

    // Register actors with management
    management_actor.send_message(ManagementMessage::RegisterActor(ActorType::Heartbeat(
        heartbeat_actor,
    )))?;

    management_actor.send_message(ManagementMessage::RegisterActor(ActorType::Webserver(
        webserver_actor,
    )))?;

    management_actor.send_message(ManagementMessage::RegisterActor(ActorType::Iroh(
        iroh_actor,
    )))?;

    tokio::signal::ctrl_c().await.unwrap();
    info!("Received Ctrl+C, initiating graceful shutdown...");

    // Send the shutdown signal to management
    management_actor.send_message(ManagementMessage::Shutdown)?;

    // Wait for all actors to finish
    let _ = tokio::try_join!(
        heartbeat_handle,
        webserver_handle,
        iroh_handle,
        management_handle
    );

    info!("Node shutdown complete");

    Ok(())
}
