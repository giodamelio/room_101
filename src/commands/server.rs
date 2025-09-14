use anyhow::{Context, Result};
use iroh_base::ticket::NodeTicket;
use ractor::Actor;
use std::time::Duration;
use std::{str::FromStr, sync::OnceLock};
use tracing::{debug, error, info, warn};

use crate::args::ServerArgs;
use crate::db::Peer;
use crate::{
    actors::{AppConfig, SupervisorActor, SystemdSecretsConfig},
    systemd_secrets,
};

static SYSTEMD_SECRETS_CONFIG: OnceLock<SystemdSecretsConfig> = OnceLock::new();

pub fn get_systemd_secrets_config() -> anyhow::Result<&'static SystemdSecretsConfig> {
    SYSTEMD_SECRETS_CONFIG
        .get()
        .ok_or_else(|| anyhow::anyhow!("SystemdSecretsConfig not initialized"))
}

pub async fn run(server_args: &ServerArgs) -> Result<()> {
    info!("Starting Room 101 Server");

    // Initialize systemd secrets configuration
    SYSTEMD_SECRETS_CONFIG
        .set(SystemdSecretsConfig {
            path: server_args.systemd_secrets_path.clone(),
            user_scope: server_args.systemd_user_scope,
        })
        .map_err(|_| anyhow::anyhow!("Failed to initialize SystemdSecretsConfig"))?;

    // Check systemd-creds availability at startup
    info!(
        "SystemD secrets config: path='{}', user_scope={}",
        server_args.systemd_secrets_path, server_args.systemd_user_scope
    );
    if systemd_secrets::is_available() {
        info!("systemd-creds is available - systemd secrets integration enabled");
    } else {
        warn!("systemd-creds is NOT available - systemd secrets integration disabled");
        warn!("To enable systemd secrets, install systemd-creds (usually part of systemd >= 248)");
    }

    // Add any bootstrap tickets as Peers
    if !server_args.bootstrap.is_empty() {
        for ticket_str in &server_args.bootstrap {
            let ticket = NodeTicket::from_str(ticket_str)?;
            Peer::insert_from_ticket(ticket).await?;
        }
    }

    // Create application configuration
    let app_config = AppConfig {
        systemd_config: SystemdSecretsConfig {
            path: server_args.systemd_secrets_path.clone(),
            user_scope: server_args.systemd_user_scope,
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
