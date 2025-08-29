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
use tracing::{debug, error, info, trace, warn};

use crate::db::{Event, EventType, Identity, Peer, Secret, age_public_key_to_string};
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
        age_public_key: String,
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
        age_public_key: String,
    },
    #[serde(rename = "HEARTBEAT")]
    Heartbeat {
        node_id: NodeId,
        time: DateTime<Utc>,
        age_public_key: String,
    },
    #[serde(rename = "SECRET")]
    Secret {
        name: String,
        encrypted_data: Vec<u8>,
        hash: String,
        target_node_id: NodeId,
        time: DateTime<Utc>,
    },
    #[serde(rename = "SECRET_DELETE")]
    SecretDelete {
        name: String,
        hash: String,
        target_node_id: NodeId,
        time: DateTime<Utc>,
    },
    #[serde(rename = "SECRET_SYNC_REQUEST")]
    SecretSyncRequest {
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
            PeerMessage::Secret { .. } => f.write_str("SECRET"),
            PeerMessage::SecretDelete { .. } => f.write_str("SECRET_DELETE"),
            PeerMessage::SecretSyncRequest { .. } => f.write_str("SECRET_SYNC_REQUEST"),
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

async fn send_all_secrets_to_node(
    target_node_id: NodeId,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> Result<()> {
    let all_secrets = Secret::list_all().await?;
    let secrets_count = all_secrets.len();
    info!(
        "📤 Sending {} secrets to newly connected node {}",
        secrets_count, target_node_id
    );

    for secret in all_secrets {
        let secret_message = PeerMessage::Secret {
            name: secret.name.clone(),
            encrypted_data: secret.encrypted_data.clone(),
            hash: secret.hash.clone(),
            target_node_id: secret.get_target_node_id()?,
            time: Utc::now(),
        };

        // Log secret send event
        Event::log(
            EventType::PeerMessage {
                message_type: "SECRET".to_string(),
            },
            format!("Sending secret '{}' to node {target_node_id}", secret.name),
            serde_json::to_value(&secret_message).ok(),
        )
        .await?;

        send_peer_message(peer_message_tx.clone(), secret_message).await?;
    }

    debug!("Sent {} secrets to node {}", secrets_count, target_node_id);

    // Log batch send completion
    Event::log(
        EventType::PeerMessage {
            message_type: "SECRET_BATCH".to_string(),
        },
        format!("Completed sending {secrets_count} secrets to new node {target_node_id}"),
        serde_json::json!({
            "target_node_id": target_node_id.to_string(),
            "secrets_count": secrets_count
        })
        .into(),
    )
    .await?;

    Ok(())
}

pub async fn announce_secret(
    secret: &Secret,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> Result<()> {
    let secret_message = PeerMessage::Secret {
        name: secret.name.clone(),
        encrypted_data: secret.encrypted_data.clone(),
        hash: secret.hash.clone(),
        target_node_id: secret.get_target_node_id()?,
        time: Utc::now(),
    };

    // Log secret announcement event
    Event::log(
        EventType::PeerMessage {
            message_type: "SECRET".to_string(),
        },
        format!(
            "Announcing secret '{}' for node {}",
            secret.name, secret.target_node_id
        ),
        serde_json::to_value(&secret_message).ok(),
    )
    .await?;

    info!(
        "📢 Announcing secret '{}' for node {} to network",
        secret.name, secret.target_node_id
    );
    send_peer_message(peer_message_tx, secret_message).await?;
    info!(
        "✅ Successfully announced secret '{}' for node {} to network",
        secret.name, secret.target_node_id
    );
    Ok(())
}

pub async fn sync_all_secrets_to_systemd() -> Result<()> {
    use crate::db::{Identity, Secret, decrypt_secret_for_identity};

    let identity = Identity::get_or_create().await?;
    let current_node_id = identity.id();

    // Get all secrets for the current node
    let all_secrets = Secret::list_all().await?;
    let my_secrets: Vec<_> = all_secrets
        .into_iter()
        .filter(|secret| {
            secret
                .get_target_node_id()
                .map(|id| id == current_node_id)
                .unwrap_or(false)
        })
        .collect();

    debug!(
        "Syncing {} secrets to systemd for current node",
        my_secrets.len()
    );

    let config = crate::get_systemd_secrets_config();
    let mut success_count = 0;
    let mut error_count = 0;

    for secret in my_secrets {
        match decrypt_secret_for_identity(&secret.encrypted_data, &identity).await {
            Ok(decrypted_content) => {
                let cred_path = format!("{}/{}.cred", config.path, secret.name);
                match crate::systemd_secrets::write_secret(
                    &secret.name,
                    &decrypted_content,
                    &cred_path,
                    config.user_scope,
                )
                .await
                {
                    Ok(()) => {
                        debug!("Synced secret '{}' to systemd", secret.name);
                        success_count += 1;
                    }
                    Err(e) => {
                        error!("Failed to sync secret '{}' to systemd: {}", secret.name, e);
                        error_count += 1;
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to decrypt secret '{}' for systemd sync: {}",
                    secret.name, e
                );
                error_count += 1;
            }
        }
    }

    info!(
        "Systemd sync complete: {} success, {} errors",
        success_count, error_count
    );
    Ok(())
}

pub async fn send_secret_sync_request(
    target_node_id: NodeId,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> Result<()> {
    let sync_request = PeerMessage::SecretSyncRequest {
        node_id: target_node_id,
        time: Utc::now(),
    };

    // Log sync request event
    Event::log(
        EventType::PeerMessage {
            message_type: "SECRET_SYNC_REQUEST".to_string(),
        },
        format!("Sending secret sync request to node {}", target_node_id),
        serde_json::to_value(&sync_request).ok(),
    )
    .await?;

    send_peer_message(peer_message_tx, sync_request).await?;
    debug!("Sent secret sync request to node {}", target_node_id);
    Ok(())
}

pub async fn announce_secret_deletion(
    name: String,
    hash: String,
    target_node_id: NodeId,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> Result<()> {
    let delete_message = PeerMessage::SecretDelete {
        name: name.clone(),
        hash: hash.clone(),
        target_node_id,
        time: Utc::now(),
    };

    // Log secret deletion announcement event
    Event::log(
        EventType::PeerMessage {
            message_type: "SECRET_DELETE".to_string(),
        },
        format!("Announcing deletion of secret '{name}' for node {target_node_id}"),
        serde_json::to_value(&delete_message).ok(),
    )
    .await?;

    send_peer_message(peer_message_tx, delete_message).await?;
    debug!(
        "Announced deletion of secret '{}' for node {}",
        name, target_node_id
    );
    Ok(())
}

pub async fn network_manager_task(
    shutdown_rx: broadcast::Receiver<()>,
    bootstrap_nodes: Option<Vec<NodeId>>,
    external_peer_message_rx: Option<mpsc::Receiver<PeerMessage>>,
) -> Result<()> {
    debug!("Network manager starting...");

    // Add bootstrap nodes to database if provided
    if let Some(nodes) = bootstrap_nodes {
        info!("🚀 Adding {} bootstrap nodes to database", nodes.len());
        for node in &nodes {
            info!("  - Bootstrap node: {}", node);
        }
        Peer::insert_bootstrap_nodes(nodes)
            .await
            .context("Failed to add bootstrap nodes to database")?;
        info!("✅ Bootstrap nodes added to database");
    } else {
        info!("ℹ️ No bootstrap nodes provided - starting in isolated mode");
    }

    // Get our identity from the db if it exists, otherwise generate one
    let identity = Identity::get_or_create().await?;
    debug!(
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

    // Create channels for message passing
    // Use external receiver if provided (from webserver), otherwise create internal channels
    let (listener_tx, listener_rx) = if let Some(ext_rx) = external_peer_message_rx {
        // When webserver is enabled, we listen to messages from the webserver
        // but still need our own sender for internal network messages
        let (internal_tx, _) = tokio::sync::mpsc::channel::<PeerMessage>(1); // dummy internal channel
        (internal_tx, ext_rx)
    } else {
        // No webserver, use internal channels for everything
        tokio::sync::mpsc::channel::<PeerMessage>(100)
    };
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
            error!("Gossip setup task error: {}", e);
        }

        trace!("Gossip setup task completed");
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
            error!("Peer message listener error: {}", e);
        }

        trace!("Peer message listener completed");
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
            error!("Listener message sender error: {}", e);
        }

        trace!("Listener message sender completed");
    }));

    // Heartbeat task
    let heartbeat_identity = identity.clone();
    let heartbeat_shutdown_rx = shutdown_rx.resubscribe();
    tasks.push(tokio::spawn(async move {
        if let Err(e) =
            peer_message_heartbeat(heartbeat_shutdown_rx, heartbeat_identity, heartbeat_tx).await
        {
            error!("Heartbeat task error: {}", e);
        }

        trace!("Heartbeat task completed");
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
            error!("Heartbeat message sender error: {}", e);
        }

        trace!("Heartbeat message sender completed");
    }));

    debug!("Network manager running, all subtasks spawned. Waiting for shutdown...");

    // Wait for shutdown signal
    let mut shutdown_rx = shutdown_rx;
    let _ = shutdown_rx.recv().await;
    trace!("Network manager received shutdown signal, cleaning up...");

    // Shutdown the gossip with timeout protection
    debug!("Shutting down gossip network...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), gossip.shutdown()).await {
        Ok(result) => {
            result?;
            debug!("Gossip network shutdown complete");
        }
        Err(_) => {
            debug!("Gossip network shutdown timed out after 5 seconds, forcing shutdown");
        }
    }

    // Wait for all network subtasks to complete
    debug!("Waiting for network subtasks to complete...");
    let task_results = futures::future::join_all(tasks).await;
    for result in task_results {
        if let Err(e) = result {
            error!("Network subtask panicked: {}", e);
        }
    }

    debug!("Network manager stopped cleanly");
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
    debug!("Gossip setup task starting...");

    // Get known peers for gossip (let Iroh discover addressing via relay/DHT)
    let all_peer_ids = Peer::list_node_ids().await?;
    let peers: Vec<NodeId> = all_peer_ids
        .iter()
        .filter(|&peer_id| *peer_id != identity.id()) // Don't include ourself
        .cloned()
        .collect();

    info!(
        "🔍 Peer discovery: found {} peers in database",
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
        "🌐 Subscribing to gossip topic with {} remote peers for bootstrap",
        peer_count
    );
    let gossip_result = select! {
        result = gossip.subscribe_and_join(PEER_TOPIC, peers) => {
            result.context("Failed to subscribe to gossip topic")
        }
        _ = shutdown_rx.recv() => {
            debug!("Gossip setup task received shutdown before subscription completed");
            return Ok(());
        }
    };

    let (peer_sender, peer_reciever) = gossip_result?.split();
    info!(
        "✅ Successfully subscribed to gossip topic and connected to {} peers",
        peer_count
    );

    // Send gossip handles to other tasks
    let _ = listener_gossip_tx.send(peer_reciever);
    let _ = sender_gossip_tx.send(peer_sender.clone());
    let _ = heartbeat_sender_gossip_tx.send(peer_sender);

    // Send our welcome message
    info!(
        "📢 Broadcasting JOIN message to network as node {}",
        identity.id()
    );
    send_peer_message(
        listener_tx.clone(),
        PeerMessage::Joined {
            node_id: identity.id(),
            time: Utc::now(),
            hostname: get_hostname(),
            age_public_key: age_public_key_to_string(&identity.age_key),
        },
    )
    .await?;

    // Wait for shutdown
    let _ = shutdown_rx.recv().await;
    debug!("Gossip setup task received shutdown signal");

    // Send leaving message
    trace!("Sending leaving message...");
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
            debug!("Leaving message sent successfully");
        }
        Err(_) => {
            debug!("Leaving message send timed out, continuing shutdown");
        }
    }

    debug!("Gossip setup task stopped cleanly");

    Ok(())
}

async fn peer_message_listener_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    identity: Identity,
    gossip_rx: tokio::sync::oneshot::Receiver<iroh_gossip::api::GossipReceiver>,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    debug!("Peer message listener starting...");

    // Wait for gossip handles or shutdown
    let mut peer_reciever = select! {
        receiver = gossip_rx => {
            receiver.context("Gossip setup task dropped")?
        }
        _ = shutdown_rx.recv() => {
            debug!("Peer message listener received shutdown before gossip ready");
            return Ok(());
        }
    };

    debug!("Peer message listener got gossip receiver, starting message loop...");

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
                    age_public_key,
                } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Joined");

                    // Add the peer to the database
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), hostname.clone(), Some(age_public_key)).await {
                        debug!("Failed to upsert peer {node_id} to database: {e}");
                    }

                    // Send our introduction to the network so the new peer gets our age key
                    send_peer_message(
                        peer_message_tx.clone(),
                        PeerMessage::Introduction {
                            node_id: identity.id(),
                            time: Utc::now(),
                            hostname: get_hostname(),
                            age_public_key: age_public_key_to_string(&identity.age_key),
                        },
                    )
                    .await?;

                    // Send all secrets to the newly joined node
                    if let Err(e) = send_all_secrets_to_node(node_id, peer_message_tx.clone()).await {
                        debug!("Failed to send secrets to new node {node_id}: {e}");
                    }
                }
                PeerMessage::Leaving { node_id, time } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Leaving");

                    // Update last_seen time when they leave
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), None, None).await {
                        debug!("Failed to update peer {node_id} last_seen time: {e}");
                    }
                }
                PeerMessage::Heartbeat { node_id, time, age_public_key } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Heartbeat");

                    // Update last_seen time and age key on heartbeat
                    if let Err(e) = Peer::upsert_peer(node_id, Some(time), None, Some(age_public_key)).await {
                        debug!("Failed to update peer {node_id} heartbeat time: {e}");
                    }
                }
                PeerMessage::Introduction {
                    ref node_id,
                    ref time,
                    ref hostname,
                    ref age_public_key,
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
                    if let Err(e) = Peer::upsert_peer(*node_id, Some(*time), hostname.clone(), Some(age_public_key.clone())).await {
                        debug!("Failed to update peer {node_id} introduction time: {e}");
                    }

                    // Send all secrets to the newly introduced node
                    info!("🤝 New peer {} introduced, sending all secrets", node_id);
                    match send_all_secrets_to_node(*node_id, peer_message_tx.clone()).await {
                        Ok(()) => info!("✅ Successfully sent all secrets to new node {}", node_id),
                        Err(e) => error!("❌ Failed to send secrets to new node {}: {}", node_id, e),
                    }
                }
                PeerMessage::Secret {
                    ref name,
                    ref encrypted_data,
                    ref hash,
                    ref target_node_id,
                    ref time,
                } => {
                    debug!(%name, %target_node_id, %hash, %time, encrypted_data_len = encrypted_data.len(),
                           current_node = %identity.id(), "Handling PeerMessage::Secret");

                    // Only process secrets that are meant for us
                    if *target_node_id == identity.id() {
                        debug!("Secret '{}' is for current node, proceeding with upsert and systemd sync", name);
                        // Use upsert to handle hash-based deduplication
                        match Secret::upsert(
                            name.clone(),
                            encrypted_data.clone(),
                            hash.clone(),
                            *target_node_id,
                        ).await {
                            Ok(was_updated) => {
                                if was_updated {
                                    info!("Successfully updated secret '{}' from peer with systemd sync", name);
                                    Event::log(
                                        EventType::PeerMessage {
                                            message_type: message.to_string(),
                                        },
                                        format!("Received new/updated secret: {name}"),
                                        serde_json::to_value(message.clone()).ok(),
                                    )
                                    .await?;
                                } else {
                                    debug!("Secret '{}' already up to date (same hash), no systemd sync needed", name);
                                }
                            }
                            Err(e) => {
                                error!("Failed to store secret '{}' for current node: {}", name, e);
                            }
                        }
                    } else {
                        debug!("Secret '{}' is for node {} (not current node), storing for gossip distribution", name, target_node_id);
                        // Store secrets for other nodes too (for gossip distribution)
                        match Secret::upsert(
                            name.clone(),
                            encrypted_data.clone(),
                            hash.clone(),
                            *target_node_id,
                        ).await {
                            Ok(was_updated) => {
                                if was_updated {
                                    debug!("Stored secret '{}' for node {}", name, target_node_id);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to store secret '{}' for node {}: {}", name, target_node_id, e);
                            }
                        }
                    }
                }
                PeerMessage::SecretDelete {
                    ref name,
                    ref hash,
                    ref target_node_id,
                    ref time,
                } => {
                    trace!(%name, %target_node_id, %hash, %time, "Handling PeerMessage::SecretDelete");

                    // Verify that the message sender is the target node (only they can delete their own secrets)
                    if _from != *target_node_id {
                        debug!("Ignoring secret deletion from {_from} for secret '{name}' belonging to {target_node_id}");
                        continue;
                    }

                    // Delete the secret from our local database
                    match Secret::delete(name, hash, *target_node_id).await {
                        Ok(was_deleted) => {
                            if was_deleted {
                                debug!("Deleted secret '{}' for node {}", name, target_node_id);
                                Event::log(
                                    EventType::PeerMessage {
                                        message_type: message.to_string(),
                                    },
                                    format!("Deleted secret: {name}"),
                                    serde_json::to_value(message.clone()).ok(),
                                )
                                .await?;
                            } else {
                                trace!("Secret '{}' with hash {} not found for deletion", name, hash);
                            }
                        }
                        Err(e) => {
                            debug!("Failed to delete secret '{}': {}", name, e);
                        }
                    }
                }
                PeerMessage::SecretSyncRequest {
                    ref node_id,
                    ref time,
                } => {
                    trace!(%node_id, %time, "Handling PeerMessage::SecretSyncRequest");

                    // Only process sync requests meant for us
                    if *node_id == identity.id() {
                        debug!("Received secret sync request, syncing all secrets to systemd");
                        if let Err(e) = sync_all_secrets_to_systemd().await {
                            error!("Failed to sync secrets to systemd: {}", e);
                        }

                        Event::log(
                            EventType::PeerMessage {
                                message_type: message.to_string(),
                            },
                            format!("Processed secret sync request from {}", _from),
                            serde_json::to_value(message.clone()).ok(),
                        )
                        .await?;
                    } else {
                        trace!("Ignoring sync request for different node: {}", node_id);
                    }
                }
            }
        }
            }
            _ = shutdown_rx.recv() => {
                debug!("Peer message listener received shutdown signal");
                break;
            }
        }
    }

    debug!("Peer message listener stopped");
    Ok(())
}

async fn peer_message_sender_task(
    mut shutdown_rx: broadcast::Receiver<()>,
    gossip_rx: tokio::sync::oneshot::Receiver<iroh_gossip::api::GossipSender>,
    mut peer_message_rx: mpsc::Receiver<PeerMessage>,
    identity: Identity,
) -> anyhow::Result<()> {
    debug!("Peer message sender starting...");

    // Wait for gossip handles or shutdown
    let peer_sender = select! {
        sender = gossip_rx => {
            sender.context("Gossip setup task dropped")?
        }
        _ = shutdown_rx.recv() => {
            debug!("Peer message sender received shutdown before gossip ready");
            return Ok(());
        }
    };

    debug!("Peer message sender got gossip sender, starting message loop...");

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
                debug!("Peer message sender received shutdown signal");
                break
            }
        }
    }

    debug!("Peer message sender stopped");
    Ok(())
}

async fn peer_message_heartbeat(
    mut shutdown_rx: broadcast::Receiver<()>,
    identity: Identity,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    debug!("Peer message heartbeat starting...");
    let mut ticker = tokio::time::interval(Duration::from_secs(10));

    loop {
        select! {
            _ = ticker.tick() => {
                    send_peer_message(
                        peer_message_tx.clone(),
                    PeerMessage::Heartbeat {
                        node_id: identity.id(),
                        time: Utc::now(),
                        age_public_key: age_public_key_to_string(&identity.age_key),
                    }
                    )
                    .await?;
            }
            _ = shutdown_rx.recv() => {
                debug!("Peer message heartbeat received shutdown signal");
                break
            }
        }
    }

    debug!("Peer message heartbeat stopped");
    Ok(())
}
