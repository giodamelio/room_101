use anyhow::{Context, Result};
use iroh::{Endpoint, Watcher, protocol::Router};
use iroh_gossip::{ALPN, net::Gossip};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::oneshot;
use tracing::{debug, info};

use crate::{db, management::IrohMessage};

pub struct IrohActor;

impl IrohActor {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub struct IrohState {
    db: db::DB,
    iroh_task_handle: Option<tokio::task::JoinHandle<Result<()>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

#[derive(Debug)]
pub struct IrohArgs {
    pub db: db::DB,
}

#[async_trait::async_trait]
impl Actor for IrohActor {
    type Msg = IrohMessage;
    type State = IrohState;
    type Arguments = IrohArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> std::result::Result<Self::State, ActorProcessingErr> {
        info!("Iroh actor started");

        let db = args.db.clone();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Start the Iroh task in the background
        let iroh_task_handle = tokio::spawn(async move {
            let result = run_iroh_task(db, shutdown_rx).await;
            if let Err(ref e) = result {
                tracing::error!("Iroh task error: {}", e);
            }
            result
        });

        Ok(IrohState {
            db: args.db,
            iroh_task_handle: Some(iroh_task_handle),
            shutdown_tx: Some(shutdown_tx),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> std::result::Result<(), ActorProcessingErr> {
        match message {
            IrohMessage::Shutdown => {
                info!("Iroh received shutdown signal, finishing network tasks...");

                // Send shutdown signal to the background task
                if let Some(shutdown_tx) = state.shutdown_tx.take() {
                    let _ = shutdown_tx.send(());
                }

                // Wait for the Iroh task to complete
                if let Some(iroh_task_handle) = state.iroh_task_handle.take() {
                    info!("Waiting for Iroh background task to complete...");
                    match iroh_task_handle.await {
                        Ok(Ok(())) => info!("Iroh background task completed successfully"),
                        Ok(Err(e)) => tracing::error!("Iroh background task failed: {}", e),
                        Err(e) => tracing::error!("Iroh background task panicked: {}", e),
                    }
                }

                // Stop the actor after shutdown
                myself.stop(None);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut Self::State,
    ) -> std::result::Result<(), ActorProcessingErr> {
        info!("Iroh actor stopped");
        Ok(())
    }
}

async fn run_iroh_task(db: db::DB, shutdown_rx: oneshot::Receiver<()>) -> Result<()> {
    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<db::Identity> = db
        .select(("config", "identity"))
        .await
        .context("Failed to load identity from database")?;

    let identity = match identity {
        Some(identity) => {
            info!(
                public_key = %identity.secret_key.public(),
                "Loaded existing identity from database"
            );

            identity
        }
        None => {
            let new_identity = db::Identity::new();

            // Write the new identity
            let _: Option<db::Identity> = db
                .create(("config", "identity"))
                .content(new_identity.clone())
                .await
                .context("Failed to save identity to database")?;

            info!(
                public_key = %new_identity.secret_key.public(),
                "Created new identity and saved to database"
            );

            new_identity
        }
    };

    // List our current peers
    dbg!(db::Peer::list(&db).await?);

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key)
        .discovery_n0()
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    let mut node_addr_watcher = endpoint.node_addr();
    let node_addr = node_addr_watcher.initialized().await;
    info!(
        node_id = %node_addr.node_id,
        addresses = ?node_addr.direct_addresses,
        relay_url = ?node_addr.relay_url,
        "Node endpoint initialized"
    );

    // Create gossip instance using builder pattern
    debug!("Spawning gossip protocol handler");
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Setup router
    debug!("Setting up protocol router");
    let _router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    info!("Accepting connections...");
    info!(
        "To connect another node: cargo run -- {}",
        node_addr.node_id
    );

    // Wait for shutdown signal
    let _ = shutdown_rx.await;
    info!("Iroh task received shutdown signal, exiting gracefully");

    Ok(())
}
