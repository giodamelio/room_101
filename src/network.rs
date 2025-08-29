use std::fmt::Display;
use std::marker::PhantomData;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use futures::TryStreamExt;
use iroh::{Endpoint, NodeId, Watcher, protocol::Router};
use iroh::{PublicKey, SecretKey};
use iroh_gossip::{ALPN, net::Gossip, proto::TopicId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::select;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

use crate::db::{Event, EventType, Identity, Peer};
use crate::utils::topic_id;

#[derive(Debug, Serialize, Deserialize)]
struct SignedMessage<M: Serialize + DeserializeOwned> {
    phantom: PhantomData<M>,
    from: PublicKey,
    data: Vec<u8>,
    signature: Signature,
}

impl<M: Serialize + DeserializeOwned> SignedMessage<M> {
    pub fn verify_and_decode(bytes: &[u8]) -> Result<(PublicKey, M)> {
        let signed_message: Self = serde_json::from_slice(bytes)?;
        signed_message
            .from
            .verify(&signed_message.data, &signed_message.signature)?;
        let message: M = serde_json::from_slice(&signed_message.data)?;
        Ok((signed_message.from, message))
    }

    pub fn sign_and_encode(secret_key: &SecretKey, message: &M) -> Result<Vec<u8>> {
        let data = serde_json::to_vec(&message)?;
        let signature = secret_key.sign(&data);
        let from: PublicKey = secret_key.public();
        let signed_message = Self {
            phantom: PhantomData,
            from,
            data,
            signature,
        };
        let encoded = serde_json::to_vec(&signed_message)?;
        Ok(encoded)
    }
}

trait MessageSigner: Serialize + DeserializeOwned {
    fn sign(&self, secret_key: &SecretKey) -> Result<Vec<u8>> {
        SignedMessage::<Self>::sign_and_encode(secret_key, self)
    }
}

impl MessageSigner for PeerMessage {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeerMessage {
    #[serde(rename = "JOINED")]
    Joined {
        node_id: NodeId,
        time: DateTime<Utc>,
        hostname: Option<String>,
    },
    #[serde(rename = "LEAVING")]
    Leaving {
        node_id: NodeId,
        time: DateTime<Utc>,
    },
    #[serde(rename = "INTRODUCTION")]
    Introduction {
        node_id: NodeId,
        time: DateTime<Utc>,
        hostname: Option<String>,
    },
    #[serde(rename = "HEARTBEAT")]
    Heartbeat {
        node_id: NodeId,
        time: DateTime<Utc>,
    },
}

impl Display for PeerMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerMessage::Joined { .. } => f.write_str("JOINED"),
            PeerMessage::Leaving { .. } => f.write_str("LEAVING"),
            PeerMessage::Introduction { .. } => f.write_str("INTRODUCTION"),
            PeerMessage::Heartbeat { .. } => f.write_str("HEARTBEAT"),
        }
    }
}

const PEER_TOPIC: TopicId = topic_id!("PEER_TOPIC");

/// Get the systems hostname, returns None if unable
fn get_hostname() -> Option<String> {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .ok()
}

async fn send_peer_message(
    peer_message_tx: mpsc::Sender<PeerMessage>,
    message: PeerMessage,
) -> Result<()> {
    // Send the actual message - use try_send to avoid deadlock during shutdown
    if let Err(e) = peer_message_tx.try_send(message.clone()) {
        // If channel is closed/full during shutdown, just log and continue
        match e {
            mpsc::error::TrySendError::Closed(_) => {
                debug!("Peer message channel closed during shutdown, skipping message");
                return Ok(());
            }
            mpsc::error::TrySendError::Full(_) => {
                // For full channel, still try the blocking send with timeout
                tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    peer_message_tx.send(message.clone()),
                )
                .await??;
            }
        }
    }

    // Create a log about sending the message (unless it is a heartbeat)
    if matches!(message, PeerMessage::Heartbeat { .. }) {
        return Ok(());
    }

    // Convert PeerMessage to JSON (uses our custom Serialize implementation)
    let data = serde_json::to_value(&message)?;

    Event::log(
        EventType::PeerMessage {
            message_type: message.to_string(),
        },
        "Send peer message".into(),
        Some(data),
    )
    .await?;

    Ok(())
}

pub async fn network_manager_task(
    shutdown_rx: broadcast::Receiver<()>,
    bootstrap_nodes: Option<Vec<NodeId>>,
) -> Result<()> {
    info!("Network manager starting...");

    // Add bootstrap nodes to database if provided
    if let Some(nodes) = bootstrap_nodes {
        info!("Adding {} bootstrap nodes to database", nodes.len());
        Peer::insert_bootstrap_nodes(nodes)
            .await
            .context("Failed to add bootstrap nodes to database")?;
    }

    // Get our identity from the db if it exists, otherwise generate one
    let identity = Identity::get_or_create().await?;
    info!(
        public_key = %identity.secret_key.public(),
        "Identity ready"
    );

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.secret_key.clone())
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

    // Create oneshot channels for coordinating gossip handles
    let (listener_gossip_tx, listener_gossip_rx) =
        tokio::sync::oneshot::channel::<iroh_gossip::api::GossipReceiver>();
    let (sender_gossip_tx, sender_gossip_rx) =
        tokio::sync::oneshot::channel::<iroh_gossip::api::GossipSender>();
    let (heartbeat_sender_gossip_tx, heartbeat_sender_gossip_rx) =
        tokio::sync::oneshot::channel::<iroh_gossip::api::GossipSender>();

    // Create separate channels for message passing
    let (listener_tx, listener_rx) = tokio::sync::mpsc::channel::<PeerMessage>(100);
    let (heartbeat_tx, heartbeat_rx) = tokio::sync::mpsc::channel::<PeerMessage>(100);

    // Spawn all network subtasks immediately so they can receive shutdown signals
    let mut tasks = Vec::new();

    // Gossip setup task - handles the blocking subscribe_and_join operation
    let gossip_setup_shutdown_rx = shutdown_rx.resubscribe();
    let gossip_for_setup = gossip.clone();
    let identity_for_setup = identity.clone();
    let listener_tx_for_setup = listener_tx.clone();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = gossip_setup_task(
            gossip_setup_shutdown_rx,
            gossip_for_setup,
            identity_for_setup,
            listener_gossip_tx,
            sender_gossip_tx,
            heartbeat_sender_gossip_tx,
            listener_tx_for_setup,
        )
        .await
        {
            tracing::error!("Gossip setup task error: {}", e);
        }
        info!("Gossip setup task completed");
    }));

    // Peer message listener task
    let listener_identity = identity.clone();
    let listener_shutdown_rx = shutdown_rx.resubscribe();
    let listener_tx_for_handler = listener_tx.clone();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = peer_message_listener_task(
            listener_shutdown_rx,
            listener_identity,
            listener_gossip_rx,
            listener_tx_for_handler,
        )
        .await
        {
            tracing::error!("Peer message listener error: {}", e);
        }
        info!("Peer message listener completed");
    }));

    // Listener message sender task
    let listener_sender_identity = identity.clone();
    let listener_sender_shutdown_rx = shutdown_rx.resubscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = peer_message_sender_task(
            listener_sender_shutdown_rx,
            sender_gossip_rx,
            listener_rx,
            listener_sender_identity,
        )
        .await
        {
            tracing::error!("Listener message sender error: {}", e);
        }
        info!("Listener message sender completed");
    }));

    // Heartbeat task
    let heartbeat_identity = identity.clone();
    let heartbeat_shutdown_rx = shutdown_rx.resubscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) =
            peer_message_heartbeat(heartbeat_shutdown_rx, heartbeat_identity, heartbeat_tx).await
        {
            tracing::error!("Heartbeat task error: {}", e);
        }
        info!("Heartbeat task completed");
    }));

    // Heartbeat message sender task
    let heartbeat_sender_identity = identity.clone();
    let heartbeat_sender_shutdown_rx = shutdown_rx.resubscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) = peer_message_sender_task(
            heartbeat_sender_shutdown_rx,
            heartbeat_sender_gossip_rx,
            heartbeat_rx,
            heartbeat_sender_identity,
        )
        .await
        {
            tracing::error!("Heartbeat message sender error: {}", e);
        }
        info!("Heartbeat message sender completed");
    }));

    info!("Network manager running, all subtasks spawned. Waiting for shutdown...");

    // Wait for shutdown signal
    let mut shutdown_rx = shutdown_rx;
    let _ = shutdown_rx.recv().await;
    info!("Network manager received shutdown signal, cleaning up...");

    // Shutdown the gossip with timeout protection
    info!("Shutting down gossip network...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), gossip.shutdown()).await {
        Ok(result) => {
            result?;
            info!("Gossip network shutdown complete");
        }
        Err(_) => {
            info!("Gossip network shutdown timed out after 5 seconds, forcing shutdown");
        }
    }

    // Wait for all network subtasks to complete
    info!("Waiting for network subtasks to complete...");
    let task_results = futures::future::join_all(tasks).await;
    for result in task_results {
        if let Err(e) = result {
            tracing::error!("Network subtask panicked: {}", e);
        }
    }

    info!("Network manager stopped cleanly");
    Ok(())
}

async fn gossip_setup_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    gossip: Gossip,
    identity: Identity,
    listener_gossip_tx: tokio::sync::oneshot::Sender<iroh_gossip::api::GossipReceiver>,
    sender_gossip_tx: tokio::sync::oneshot::Sender<iroh_gossip::api::GossipSender>,
    heartbeat_sender_gossip_tx: tokio::sync::oneshot::Sender<iroh_gossip::api::GossipSender>,
    listener_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    info!("Gossip setup task starting...");

    // Get known peers for gossip (let Iroh discover addressing via relay/DHT)
    let all_peer_ids = Peer::list_node_ids().await?;
    let peers: Vec<NodeId> = all_peer_ids
        .into_iter()
        .filter(|&peer_id| peer_id != identity.id()) // Don't include ourself
        .collect();

    debug!("Known peers for gossip discovery: {:?}", peers);

    // Subscribe to the peers topic with shutdown awareness
    info!(
        "Subscribing to gossip topic with {} known peers",
        peers.len()
    );
    let gossip_result = select! {
        result = gossip.subscribe_and_join(PEER_TOPIC, peers) => {
            result.context("Failed to subscribe to gossip topic")
        }
        _ = shutdown_rx.recv() => {
            info!("Gossip setup task received shutdown before subscription completed");
            return Ok(());
        }
    };

    let (peer_sender, peer_reciever) = gossip_result?.split();
    info!("Successfully subscribed to gossip topic");

    // Send gossip handles to other tasks
    let _ = listener_gossip_tx.send(peer_reciever);
    let _ = sender_gossip_tx.send(peer_sender.clone());
    let _ = heartbeat_sender_gossip_tx.send(peer_sender);

    // Send our welcome message
    send_peer_message(
        listener_tx.clone(),
        PeerMessage::Joined {
            node_id: identity.id(),
            time: Utc::now(),
            hostname: get_hostname(),
        },
    )
    .await?;

    // Wait for shutdown
    let _ = shutdown_rx.recv().await;
    info!("Gossip setup task received shutdown signal");

    // Send leaving message
    info!("Sending leaving message...");
    match tokio::time::timeout(
        std::time::Duration::from_secs(1),
        send_peer_message(
            listener_tx.clone(),
            PeerMessage::Leaving {
                node_id: identity.id(),
                time: Utc::now(),
            },
        ),
    )
    .await
    {
        Ok(result) => {
            result?;
            info!("Leaving message sent successfully");
        }
        Err(_) => {
            info!("Leaving message send timed out, continuing shutdown");
        }
    }

    info!("Gossip setup task stopped cleanly");
    Ok(())
}

async fn peer_message_listener_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    identity: Identity,
    gossip_rx: tokio::sync::oneshot::Receiver<iroh_gossip::api::GossipReceiver>,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    info!("Peer message listener starting...");

    // Wait for gossip handles or shutdown
    let mut peer_reciever = select! {
        receiver = gossip_rx => {
            receiver.map_err(|_| anyhow::anyhow!("Gossip setup task dropped"))?
        }
        _ = shutdown_rx.recv() => {
            info!("Peer message listener received shutdown before gossip ready");
            return Ok(());
        }
    };

    info!("Peer message listener got gossip receiver, starting message loop...");

    loop {
        select! {
            event = peer_reciever.try_next() => {
                let Some(event) = event? else {
                    break;
                };
                if let iroh_gossip::api::Event::Received(message) = event {
                    let (_from, message): (NodeId, PeerMessage) =
                        SignedMessage::verify_and_decode(&message.content)?;

                    match message {
                PeerMessage::Joined {
                    node_id,
                    time,
                    hostname,
                } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Joined");

                    // Add the peer to the database
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), hostname.clone()).await {
                        debug!("Failed to upsert peer {node_id} to database: {e}");
                    }

                    // Send an introduction to the network
                    send_peer_message(
                        peer_message_tx.clone(),
                        PeerMessage::Introduction {
                            node_id: identity.id(),
                            time: Utc::now(),
                            hostname,
                        },
                    )
                    .await?;
                }
                PeerMessage::Leaving { node_id, time } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Leaving");

                    // Update last_seen time when they leave
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), None).await {
                        debug!("Failed to update peer {node_id} last_seen time: {e}");
                    }
                }
                PeerMessage::Heartbeat { node_id, time } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Heartbeat");

                    // Update last_seen time on heartbeat
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), None).await {
                        debug!("Failed to update peer {node_id} heartbeat time: {e}");
                    }
                }
                PeerMessage::Introduction {
                    ref node_id,
                    ref time,
                    ref hostname,
                } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Introduction");

                    Event::log(
                        EventType::PeerMessage {
                            message_type: message.to_string(),
                        },
                        "Got introduction".into(),
                        serde_json::to_value(message.clone()).ok(),
                    )
                    .await?;

                    // Update last_seen time on introduction
                    if let Err(e) = Peer::upsert_peer(*node_id, Some(*time), hostname.clone()).await {
                        debug!("Failed to update peer {node_id} introduction time: {e}");
                    }
                }
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Peer message listener received shutdown signal");
                break;
            }
        }
    }

    info!("Peer message listener stopped");
    Ok(())
}

async fn peer_message_sender_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    gossip_rx: tokio::sync::oneshot::Receiver<iroh_gossip::api::GossipSender>,
    mut peer_message_rx: mpsc::Receiver<PeerMessage>,
    identity: Identity,
) -> anyhow::Result<()> {
    info!("Peer message sender starting...");

    // Wait for gossip handles or shutdown
    let peer_sender = select! {
        sender = gossip_rx => {
            sender.map_err(|_| anyhow::anyhow!("Gossip setup task dropped"))?
        }
        _ = shutdown_rx.recv() => {
            info!("Peer message sender received shutdown before gossip ready");
            return Ok(());
        }
    };

    info!("Peer message sender got gossip sender, starting message loop...");

    loop {
        select! {
            Some(message) = peer_message_rx.recv() => {
                peer_sender
                    .broadcast(
                        message
                            .sign(&identity.secret_key)?
                            .into(),
                    )
                .await?;
            }
            _ = shutdown_rx.recv() => {
                info!("Peer message sender received shutdown signal");
                break
            }
        }
    }

    info!("Peer message sender stopped");
    Ok(())
}

async fn peer_message_heartbeat(
    mut shutdown_rx: broadcast::Receiver<()>,
    identity: Identity,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    info!("Peer message heartbeat starting...");
    let mut ticker = tokio::time::interval(Duration::from_secs(10));

    loop {
        select! {
            _ = ticker.tick() => {
                    send_peer_message(
                        peer_message_tx.clone(),
                    PeerMessage::Heartbeat {
                        node_id: identity.id(),
                        time: Utc::now(),
                    }
                    )
                    .await?;
            }
            _ = shutdown_rx.recv() => {
                info!("Peer message heartbeat received shutdown signal");
                break
            }
        }
    }

    info!("Peer message heartbeat stopped");
    Ok(())
}
