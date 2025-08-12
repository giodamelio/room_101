use std::fmt::Display;
use std::marker::PhantomData;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use ed25519_dalek::Signature;
use iroh::{Endpoint, NodeId, Watcher, protocol::Router};
use iroh::{PublicKey, SecretKey};
use iroh_gossip::api::{GossipReceiver, GossipSender};
use iroh_gossip::{ALPN, net::Gossip, proto::TopicId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use surrealdb::Datetime;
use tokio::select;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};
use tokio_stream::StreamExt;
use tracing::{debug, info, trace};

use crate::db::{self, Event, EventType, Identity};
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
        time: Datetime,
        hostname: Option<String>,
    },
    #[serde(rename = "LEAVING")]
    Leaving { node_id: NodeId, time: Datetime },
    #[serde(rename = "INTRODUCTION")]
    Introduction {
        node_id: NodeId,
        time: Datetime,
        hostname: Option<String>,
    },
    #[serde(rename = "HEARTBEAT")]
    Heartbeat { node_id: NodeId, time: Datetime },
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
    // Send the actual message
    peer_message_tx.send(message.clone()).await?;

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

pub async fn iroh_subsystem(
    subsys: SubsystemHandle,
    bootstrap_nodes: Option<Vec<NodeId>>,
) -> Result<()> {
    info!("Iroh subsystem started");

    // Add bootstrap nodes to database if provided
    if let Some(nodes) = bootstrap_nodes {
        info!("Adding {} bootstrap nodes to database", nodes.len());
        db::Peer::add_peers(nodes)
            .await
            .context("Failed to add bootstrap nodes to database")?;
    }

    // Get our identity from the db if it exists, otherwise generate one
    let identity: Option<db::Identity> = db::db()
        .await
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
            let _: Option<db::Identity> = db::db()
                .await
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

    // Create endpoint for this node
    debug!("Creating iroh endpoint with identity");
    let endpoint = Endpoint::builder()
        .secret_key(identity.clone().secret_key)
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

    // Get known peers for gossip (let Iroh discover addressing via relay/DHT)
    let peers: Vec<NodeId> = db::Peer::list()
        .await?
        .iter()
        .map(|p| p.node_id)
        .filter(|&peer_id| peer_id != identity.id()) // Don't include ourself
        .collect();

    debug!("Known peers for gossip discovery: {:?}", peers);

    info!("Accepting connections, Our Node ID: {}", node_addr.node_id);

    // Subscribe to the peers topic
    let (peer_sender, peer_reciever) = gossip.subscribe_and_join(PEER_TOPIC, peers).await?.split();
    let (peer_message_tx, peer_message_rx) = tokio::sync::mpsc::channel::<PeerMessage>(5);

    // Listen to incoming PeerMessages
    let listener_peer_message_tx = peer_message_tx.clone();
    let listener_identity = identity.clone();
    subsys.start(SubsystemBuilder::new(
        "peer-message-listener",
        move |subsys| {
            peer_message_handler(
                subsys,
                listener_identity,
                peer_reciever,
                listener_peer_message_tx,
            )
        },
    ));

    // Send outgoing messages
    let sender_peer_message_sender = peer_sender.clone();
    let sender_identity = identity.clone();
    subsys.start(SubsystemBuilder::new(
        "peer-message-sender",
        move |subsys| {
            peer_message_sender(
                subsys,
                sender_peer_message_sender,
                peer_message_rx,
                sender_identity,
            )
        },
    ));

    // Send heartbeat
    let heartbeat_peer_message_sender = peer_message_tx.clone();
    let heartbeat_identity = identity.clone();
    subsys.start(SubsystemBuilder::new(
        "peer-message-hearbeat",
        move |subsys| {
            peer_message_heartbeat(subsys, heartbeat_identity, heartbeat_peer_message_sender)
        },
    ));

    // Send our welcome message
    send_peer_message(
        peer_message_tx.clone(),
        PeerMessage::Joined {
            node_id: identity.id(),
            time: Datetime::from(Utc::now()),
            hostname: get_hostname(),
        },
    )
    .await?;

    // Wait for shutdown signal
    subsys.on_shutdown_requested().await;

    // Send our leaving event
    info!("Sending leaving message...");
    send_peer_message(
        peer_message_tx.clone(),
        PeerMessage::Leaving {
            node_id: identity.id(),
            time: Datetime::from(Utc::now()),
        },
    )
    .await?;

    // Wait for message propagation - this is unfortunately necessary
    // because gossip protocols need time to deliver messages to peers over the network
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Shutdown the gossip
    trace!("Shutting down gossip network");
    gossip.shutdown().await?;

    info!("Iroh subsystem stopped");
    Ok(())
}

async fn peer_message_handler(
    _subsys: SubsystemHandle,
    identity: Identity,
    mut peer_reciever: GossipReceiver,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    while let Some(event) = peer_reciever.try_next().await? {
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
                    if let Err(e) =
                        db::Peer::upsert_peer(node_id, Some(time), hostname.clone()).await
                    {
                        debug!("Failed to add peer {node_id} to database: {e}");
                    }

                    // Send an introduction to the network
                    send_peer_message(
                        peer_message_tx.clone(),
                        PeerMessage::Introduction {
                            node_id: identity.id(),
                            time: Datetime::from(Utc::now()),
                            hostname,
                        },
                    )
                    .await?;
                }
                PeerMessage::Leaving { node_id, time } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Leaving");

                    // Update last_seen time when they leave
                    if let Err(e) = db::Peer::upsert_peer(node_id, Some(time), None).await {
                        debug!("Failed to update peer {node_id} last_seen time: {e}");
                    }
                }
                PeerMessage::Heartbeat { node_id, time } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Heartbeat");

                    // Update last_seen time on heartbeat
                    if let Err(e) = db::Peer::upsert_peer(node_id, Some(time), None).await {
                        debug!("Failed to update peer {node_id} heartbeat time: {e}");
                    }
                }
                PeerMessage::Introduction {
                    node_id,
                    time,
                    hostname,
                } => {
                    trace!(%node_id, %time, "Handling PeerMessage::Introduction");

                    // Update last_seen time on heartbeat
                    if let Err(e) = db::Peer::upsert_peer(node_id, Some(time), hostname).await {
                        debug!("Failed to update peer {node_id} heartbeat time: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}

async fn peer_message_sender(
    subsys: SubsystemHandle,
    peer_sender: GossipSender,
    mut peer_message_rx: mpsc::Receiver<PeerMessage>,
    identity: Identity,
) -> anyhow::Result<()> {
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
            _ = subsys.on_shutdown_requested() => {
                break
            }
        }
    }

    Ok(())
}

async fn peer_message_heartbeat(
    subsys: SubsystemHandle,
    identity: Identity,
    peer_message_tx: mpsc::Sender<PeerMessage>,
) -> anyhow::Result<()> {
    let mut ticker = tokio::time::interval(Duration::from_secs(10));

    loop {
        select! {
            _ = ticker.tick() => {
                    send_peer_message(
                        peer_message_tx.clone(),
                    PeerMessage::Heartbeat {
                        node_id: identity.id(),
                        time: Datetime::from(Utc::now()),
                    }
                    )
                    .await?;
            }
            _ = subsys.on_shutdown_requested() => {
                break
            }
        }
    }

    Ok(())
}
