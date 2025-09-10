use anyhow::{Context, Result};
use iroh_base::ticket::NodeTicket;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use std::time::Duration;
use std::{str::FromStr, sync::OnceLock};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::db::Peer;

mod actors;
mod args;
mod custom_serde;
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

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub enable_webserver: bool,
    pub webserver_port: u16,
    pub systemd_config: SystemdSecretsConfig,
}

pub struct SupervisorActor;

#[derive(Debug)]
pub enum SupervisorMessage {
    Shutdown,
}

impl Actor for SupervisorActor {
    type Msg = SupervisorMessage;
    type State = ();
    type Arguments = AppConfig;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting SupervisorActor with linked children");

        // Get all the existing peers
        let peers = Peer::list().await?;

        let (_iroh_actor, _iroh_handle) = Actor::spawn_linked(
            Some("iroh".into()),
            actors::gossip2::iroh::IrohActor,
            (peers,),
            myself.clone().into(),
        )
        .await?;

        // // Start all child actors with spawn_linked() - if any die, supervisor dies
        // let (_gossip_actor, _gossip_handle) = Actor::spawn_linked(
        //     Some("gossip".into()),
        //     actors::gossip::GossipActor,
        //     actors::gossip::GossipConfig {
        //         bootstrap_nodes: config.bootstrap_nodes,
        //     },
        //     myself.clone().into(),
        // )
        // .await
        // .map_err(|e| {
        //     Box::new(std::io::Error::other(format!(
        //         "Failed to start GossipActor: {e}"
        //     ))) as ActorProcessingErr
        // })?;
        //
        // let (_systemd_actor, _systemd_handle) = Actor::spawn_linked(
        //     Some("systemd".into()),
        //     actors::systemd::SystemdActor,
        //     config.systemd_config,
        //     myself.clone().into(),
        // )
        // .await
        // .map_err(|e| {
        //     Box::new(std::io::Error::other(format!(
        //         "Failed to start SystemdActor: {e}"
        //     ))) as ActorProcessingErr
        // })?;

        // debug!("Starting web server? {}", config.enable_webserver);
        // if config.enable_webserver {
        //     let (_webserver_actor, _webserver_handle) = Actor::spawn_linked(
        //         Some("webserver".into()),
        //         actors::webserver::WebServerActor,
        //         (config.webserver_port, 10),
        //         myself.clone().into(),
        //     )
        //     .await
        //     .context("Failed to start Webserver Actor")?;
        // }
        //
        // info!("All actors started successfully");
        Ok(())
    }
}

static SYSTEMD_SECRETS_CONFIG: OnceLock<SystemdSecretsConfig> = OnceLock::new();

pub fn get_systemd_secrets_config() -> anyhow::Result<&'static SystemdSecretsConfig> {
    SYSTEMD_SECRETS_CONFIG
        .get()
        .ok_or_else(|| anyhow::anyhow!("SystemdSecretsConfig not initialized"))
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
    let args = args::args().await;

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

    // Add any bootstrap tickets as Peers
    if !args.bootstrap.is_empty() {
        for ticket_str in &args.bootstrap {
            let ticket = NodeTicket::from_str(ticket_str)?;
            Peer::insert_from_ticket(ticket).await?;
        }
    };

    // Create application configuration
    let app_config = AppConfig {
        enable_webserver: args.start_web,
        webserver_port: args.port,
        systemd_config: SystemdSecretsConfig {
            path: args.systemd_secrets_path.clone(),
            user_scope: args.systemd_user_scope,
        },
    };

    // Start the supervisor actor
    debug!("Starting SupervisorActor");
    let (supervisor_actor, supervisor_handle) =
        Actor::spawn(Some("supervisor".into()), SupervisorActor, app_config).await?;

    info!("SupervisorActor started, waiting for Ctrl+C...");

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for ctrl-c")?;

    info!("Received Ctrl+C, initiating shutdown...");

    // Stop the supervisor actor, which will stop all linked actors
    supervisor_actor.stop(None);

    // Wait for supervisor to complete shutdown
    let shutdown_result = tokio::time::timeout(Duration::from_secs(10), supervisor_handle).await;

    match shutdown_result {
        Ok(Ok(())) => {
            debug!("SupervisorActor shut down cleanly");
        }
        Ok(Err(e)) => {
            error!("SupervisorActor error during shutdown: {:?}", e);
        }
        Err(_) => {
            warn!("SupervisorActor shutdown timed out after 10 seconds");
        }
    }

    // TODO: Clean up database connection
    debug!("Closing database connection...");
    // if let Err(e) = db::close_db().await {
    //     error!("Failed to close database cleanly: {}", e);
    // }

    info!("Application shutdown complete");
    Ok(())
}
