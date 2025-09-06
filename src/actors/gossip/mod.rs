use anyhow::{Context, Result};
use chrono::Utc;
use iroh::node_info::NodeIdExt;
use iroh::protocol::Router;
use iroh::{Endpoint, NodeId, Watcher};
use iroh_base::ticket::NodeTicket;
use iroh_gossip::{ALPN, net::Gossip, proto::TopicId};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, info, trace};

use crate::db::{Identity, Peer, age_public_key_to_string};
use crate::network::protocol::{MessageSigner, PeerMessage};
use crate::utils::topic_id;

pub mod heartbeat;
pub mod listener;
pub mod sender;

const PEER_TOPIC: TopicId = topic_id!("PEER_TOPIC");

#[derive(Debug, Clone)]
pub struct GossipConfig {
    pub bootstrap_nodes: Option<Vec<NodeTicket>>,
}

pub struct GossipActor;

#[derive(Debug)]
pub enum GossipMessage {
    SendPeerMessage(PeerMessage),
    AnnounceSecret(crate::db::Secret),
    AnnounceSecretDeletion {
        name: String,
        hash: String,
        target_node_id: NodeId,
    },
    SendSecretSyncRequest(NodeId),
}

pub struct GossipState {
    shutdown_tx: Option<oneshot::Sender<()>>,
    gossip_task_handle: JoinHandle<()>,
    message_tx: mpsc::UnboundedSender<GossipMessage>,
}

impl Actor for GossipActor {
    type Msg = GossipMessage;
    type State = GossipState;
    type Arguments = GossipConfig;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting GossipActor with tokio task");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let (message_tx, message_rx) = mpsc::unbounded_channel::<GossipMessage>();

        let gossip_task_handle = tokio::spawn(async move {
            if let Err(e) = run_gossip_networking(config, myself, shutdown_rx, message_rx).await {
                tracing::error!("Gossip networking task failed: {}", e);
            }
        });

        Ok(GossipState {
            shutdown_tx: Some(shutdown_tx),
            gossip_task_handle,
            message_tx,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        debug!("GossipActor received message: {:?}", message);
        // Forward all messages to the networking task via channel
        if let Err(e) = state.message_tx.send(message) {
            tracing::error!("Failed to forward message to networking task: {}", e);
            return Err(Box::new(std::io::Error::other(format!(
                "Failed to forward message to networking task: {e}"
            ))) as ActorProcessingErr);
        }
        debug!("Successfully forwarded message to networking task");
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        debug!("GossipActor shutting down");

        // Send the shutdown signal to the networking task
        if let Some(tx) = state.shutdown_tx.take()
            && tx.send(()).is_err()
        {
            debug!("Gossip networking task already stopped");
        }

        // Wait for the networking task to finish
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            &mut state.gossip_task_handle,
        )
        .await
        {
            Ok(Ok(_)) => debug!("Gossip networking task shut down cleanly"),
            Ok(Err(e)) => tracing::error!("Gossip networking task error: {e:?}"),
            Err(_) => tracing::error!("Gossip networking task shutdown timed out"),
        }

        info!("GossipActor shutdown complete");
        Ok(())
    }
}

async fn run_gossip_networking(
    config: GossipConfig,
    myself: ActorRef<GossipMessage>,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut message_rx: mpsc::UnboundedReceiver<GossipMessage>,
) -> Result<()> {
    info!("Gossip networking task starting...");

    // Add bootstrap nodes to database if provided
    if let Some(nodes) = &config.bootstrap_nodes {
        info!("Adding {} bootstrap nodes to database", nodes.len());
        for node in nodes {
            info!("  - Bootstrap node: {}", node);
        }
        Peer::insert_bootstrap_nodes(nodes.clone())
            .await
            .context("Failed to add bootstrap nodes to database")?;
        info!("Bootstrap nodes added to database");
    } else {
        info!("No bootstrap nodes provided - starting in isolated mode");
    }

    // Get our identity from the db if it exists, otherwise generate one
    let identity = Identity::get_or_create()
        .await
        .context("Failed to get identity")?;
    debug!(
        public_key = %identity.secret_key.public(),
        "Identity ready"
    );

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key.clone())
        .discovery_n0() // Enable N0 discovery for internet connectivity
        .bind()
        .await
        .context("Failed to create iroh endpoint")?;

    let mut node_addr_watcher = endpoint.node_addr();
    let node_addr = node_addr_watcher.initialized().await;
    let ticket = NodeTicket::new(node_addr.clone());
    info!(
        ticket = ?ticket.to_string(),
        "Node endpoint initialized"
    );
    debug!(
        node_id = %node_addr.node_id,
        addresses = ?node_addr.direct_addresses,
        relay_url = ?node_addr.relay_url,
        z32_id = ?node_addr.node_id.to_z32(),
        "Endpoint details"
    );

    // Create gossip instance using builder pattern
    debug!("Spawning gossip protocol handler");
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Setup router
    debug!("Setting up protocol router");
    let _router = Router::builder(endpoint.clone())
        .accept(ALPN, gossip.clone())
        .spawn();

    // Setup gossip subscription and get the peer sender
    let (peer_sender, peer_receiver) =
        setup_gossip_subscription(gossip.clone(), identity.clone()).await?;

    // Start the listener task
    let identity_for_listener = identity.clone();
    let myself_for_listener = myself.clone();
    let mut listener_task = tokio::spawn(async move {
        listener::start_message_listener(peer_receiver, identity_for_listener, myself_for_listener)
            .await;
    });

    // Start heartbeat task
    let identity_for_heartbeat = identity.clone();
    let myself_for_heartbeat = myself.clone();
    let mut heartbeat_task = tokio::spawn(async move {
        heartbeat::start_heartbeat_loop(identity_for_heartbeat, myself_for_heartbeat).await;
    });

    info!("Gossip networking task initialization complete - waiting for messages and shutdown");

    // Main event loop - handle both actor messages and shutdown
    loop {
        tokio::select! {
            // Handle incoming messages from the actor
            Some(actor_message) = message_rx.recv() => {
                debug!("Received actor message from channel: {:?}", actor_message);
                if let Err(e) = handle_actor_message(actor_message, &peer_sender, &identity).await {
                    tracing::error!("Failed to handle actor message: {}", e);
                } else {
                    debug!("Successfully processed actor message");
                }
            }
            // Handle shutdown signal
            _ = &mut shutdown_rx => {
                info!("Gossip networking task received shutdown signal");
                break;
            }
            // Handle task failures
            _ = &mut listener_task => {
                tracing::warn!("Listener task ended unexpectedly");
                break;
            }
            _ = &mut heartbeat_task => {
                tracing::warn!("Heartbeat task ended unexpectedly");
                break;
            }
        }
    }

    // Send leaving message before shutdown
    let leaving_message = PeerMessage::Leaving {
        node_id: identity.id(),
        ticket: identity.ticket(),
        time: Utc::now(),
    };

    if let Ok(signed_bytes) = leaving_message.sign(&identity.secret_key) {
        if let Err(e) = peer_sender.broadcast(signed_bytes.into()).await {
            debug!("Failed to send leaving message: {}", e);
        } else {
            info!("Sent leaving message to network");
        }
    }

    info!("Gossip networking task shutdown complete");
    Ok(())
}

async fn handle_actor_message(
    message: GossipMessage,
    peer_sender: &iroh_gossip::api::GossipSender,
    identity: &Identity,
) -> Result<()> {
    use crate::actors::gossip::sender;

    match message {
        GossipMessage::SendPeerMessage(peer_message) => {
            trace!("Forwarding peer message to network: {:?}", peer_message);
            sender::send_peer_message(peer_sender, &peer_message, identity).await?;
        }
        GossipMessage::AnnounceSecret(secret) => {
            trace!("Announcing secret to network: {}", secret.name);
            sender::announce_secret(peer_sender, &secret, identity).await?;
        }
        GossipMessage::AnnounceSecretDeletion {
            name,
            hash,
            target_node_id,
        } => {
            trace!("Announcing secret deletion to network: {} ({})", name, hash);
            sender::announce_secret_deletion(peer_sender, &name, &hash, target_node_id, identity)
                .await?;
        }
        GossipMessage::SendSecretSyncRequest(target_node_id) => {
            trace!("Sending secret sync request to: {}", target_node_id);
            sender::send_secret_sync_request(peer_sender, target_node_id, identity).await?;
        }
    }
    Ok(())
}

async fn setup_gossip_subscription(
    gossip: Gossip,
    identity: Identity,
) -> Result<(
    iroh_gossip::api::GossipSender,
    iroh_gossip::api::GossipReceiver,
)> {
    debug!("Gossip setup task starting...");

    // Get known peers for gossip (let Iroh discover addressing via relay/DHT)
    let all_peer_ids = Peer::list_node_ids().await?;
    let peers: Vec<NodeId> = all_peer_ids
        .iter()
        .filter(|&peer_id| *peer_id != identity.id()) // Don't include ourself
        .cloned()
        .collect();

    info!(
        "Peer discovery: found {} peers in database",
        all_peer_ids.len()
    );
    for peer_id in &all_peer_ids {
        if *peer_id == identity.id() {
            info!("  - {} (this node)", peer_id);
        } else {
            info!("  - {} (remote)", peer_id);
        }
    }

    let peer_count = peers.len();
    info!(
        "Subscribing to gossip topic with {} remote peers for bootstrap",
        peer_count
    );

    let (peer_sender, peer_receiver) = gossip
        .subscribe(PEER_TOPIC, peers)
        .await
        .context("Failed to subscribe to gossip topic")?
        .split();

    info!(
        "Successfully subscribed to gossip topic and connected to {} peers",
        peer_count
    );

    // Send our joined message
    let joined_message = PeerMessage::Joined {
        node_id: identity.id(),
        ticket: identity.ticket(),
        time: Utc::now(),
        hostname: get_hostname(),
        age_public_key: age_public_key_to_string(&identity.age_key),
    };

    info!(
        "🚀 Sending JOINED message to network: node_id={}, hostname={:?}",
        identity.id(),
        get_hostname()
    );
    debug!("Join message details: {:?}", joined_message);

    let signed_bytes = joined_message.sign(&identity.secret_key)?;
    debug!(
        "JOINED message signed, broadcasting {} bytes",
        signed_bytes.len()
    );
    peer_sender.broadcast(signed_bytes.into()).await?;
    info!("✅ Successfully sent JOINED message to network");

    debug!("Gossip setup task completed");
    Ok((peer_sender, peer_receiver))
}

pub fn get_hostname() -> Option<String> {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .ok()
}
