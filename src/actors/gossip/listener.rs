use anyhow::Result;
use chrono::Utc;
use futures::TryStreamExt;
use iroh::NodeId;
use ractor::{ActorRef, registry};
use tracing::{debug, error, info, trace, warn};

use super::{GossipMessage, get_hostname};
use crate::db::{Event, EventType, Identity, Peer, Secret, age_public_key_to_string};
use crate::network::protocol::{PeerMessage, SignedMessage};

pub async fn start_message_listener(
    mut peer_receiver: iroh_gossip::api::GossipReceiver,
    identity: Identity,
    gossip_actor: ActorRef<GossipMessage>,
) {
    info!(
        "🎧 Peer message listener starting for node {}",
        identity.id()
    );

    let mut event_count = 0;

    loop {
        match peer_receiver.try_next().await {
            Ok(Some(event)) => {
                event_count += 1;
                trace!("Gossip event #{} received: {:?}", event_count, event);
                if let iroh_gossip::api::Event::Received(message) = event {
                    info!(
                        "📥 Received gossip message #{} from network, content length: {} bytes",
                        event_count,
                        message.content.len()
                    );
                    if let Err(e) =
                        handle_received_message(&message.content, &identity, &gossip_actor).await
                    {
                        error!("Failed to handle received message: {}", e);
                    }
                } else {
                    trace!(
                        "Ignoring non-Received gossip event #{}: {:?}",
                        event_count, event
                    );
                }
            }
            Ok(None) => {
                debug!("Gossip receiver stream ended");
                break;
            }
            Err(e) => {
                error!("Error receiving gossip message: {}", e);
                break;
            }
        }
    }

    debug!("Peer message listener stopped");
}

async fn handle_received_message(
    content: &[u8],
    identity: &Identity,
    gossip_actor: &ActorRef<GossipMessage>,
) -> Result<()> {
    debug!(
        "Attempting to verify and decode message of {} bytes",
        content.len()
    );
    let (from, message): (NodeId, PeerMessage) = match SignedMessage::verify_and_decode(content) {
        Ok(result) => {
            debug!("Successfully verified message from node: {}", result.0);
            result
        }
        Err(e) => {
            error!("Failed to verify message signature: {}", e);
            return Err(e);
        }
    };

    info!(
        "🔍 Processing message type: {} from node: {}",
        message, from
    );

    match message {
        PeerMessage::Joined {
            node_id,
            ticket,
            time,
            hostname,
            age_public_key,
        } => {
            info!("🎉 New node JOINED the network: {} at {}", node_id, time);
            debug!(
                "Join details: hostname={:?}, age_key={}",
                hostname, age_public_key
            );

            // Add the peer to the database
            if let Err(e) = Peer::upsert_peer(
                node_id,
                ticket,
                Some(time),
                hostname.clone(),
                Some(age_public_key),
            )
            .await
            {
                debug!("Failed to upsert peer {node_id} to database: {e}");
            } else {
                debug!("Successfully added peer {} to database", node_id);
            }

            // Send our introduction to the network so the new peer gets our age key
            let introduction_message = PeerMessage::Introduction {
                node_id: identity.id(),
                ticket: identity.ticket(),
                time: Utc::now(),
                hostname: get_hostname(),
                age_public_key: age_public_key_to_string(&identity.age_key),
            };

            info!("📢 Sending introduction to newly joined node {}", node_id);
            if let Err(e) = gossip_actor.cast(GossipMessage::SendPeerMessage(introduction_message))
            {
                error!("Failed to send introduction message: {}", e);
            } else {
                debug!("Successfully queued introduction message");
            }

            // Send all secrets to the newly joined node
            info!("📦 Sending all secrets to newly joined node {}", node_id);
            match send_all_secrets_to_node(node_id, gossip_actor).await {
                Ok(()) => info!("✅ Successfully sent all secrets to new node {}", node_id),
                Err(e) => error!("❌ Failed to send secrets to new node {}: {}", node_id, e),
            }
        }
        PeerMessage::Leaving {
            node_id,
            ticket,
            time,
        } => {
            trace!(%node_id, %time, "Handling PeerMessage::Leaving");

            // Update last_seen time when they leave
            if let Err(e) = Peer::upsert_peer(node_id, ticket, Some(time), None, None).await {
                debug!("Failed to update peer {node_id} last_seen time: {e}");
            }
        }
        PeerMessage::Heartbeat {
            node_id,
            ticket,
            time,
            age_public_key,
        } => {
            trace!(%node_id, %time, "Handling PeerMessage::Heartbeat");

            // Update last_seen time and age key on heartbeat
            if let Err(e) =
                Peer::upsert_peer(node_id, ticket, Some(time), None, Some(age_public_key)).await
            {
                debug!("Failed to update peer {node_id} heartbeat time: {e}");
            }
        }
        PeerMessage::Introduction {
            ref node_id,
            ref ticket,
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
            if let Err(e) = Peer::upsert_peer(
                *node_id,
                ticket.clone(),
                Some(*time),
                hostname.clone(),
                Some(age_public_key.clone()),
            )
            .await
            {
                debug!("Failed to update peer {node_id} introduction time: {e}");
            }

            // Send all secrets to the newly introduced node
            info!("New peer {} introduced, sending all secrets", node_id);
            match send_all_secrets_to_node(*node_id, gossip_actor).await {
                Ok(()) => info!("Successfully sent all secrets to new node {}", node_id),
                Err(e) => error!("Failed to send secrets to new node {}: {}", node_id, e),
            }
        }
        PeerMessage::Secret {
            ref name,
            ref encrypted_data,
            ref hash,
            ref target_node_id,
            ref time,
        } => {
            info!(
                "🔐 Received PeerMessage::Secret: name='{}', target={}, hash={}, time={}, encrypted_data_len={}, current_node={}",
                name,
                target_node_id,
                hash,
                time,
                encrypted_data.len(),
                identity.id()
            );

            // Debug the comparison
            debug!(
                "Comparing target_node_id '{}' with current identity.id() '{}'",
                target_node_id,
                identity.id()
            );
            debug!(
                "NodeId comparison result: {}",
                *target_node_id == identity.id()
            );

            // Only process secrets that are meant for us
            if *target_node_id == identity.id() {
                info!(
                    "✅ Secret '{}' IS FOR current node, proceeding with upsert and systemd sync",
                    name
                );
                // Use upsert to handle hash-based deduplication
                info!("📝 Calling Secret::upsert() for secret '{}'", name);
                match Secret::upsert(
                    name.clone(),
                    encrypted_data.clone(),
                    hash.clone(),
                    *target_node_id,
                )
                .await
                {
                    Ok(was_updated) => {
                        if was_updated {
                            info!(
                                "Successfully updated secret '{}' from peer with systemd sync",
                                name
                            );
                            Event::log(
                                EventType::PeerMessage {
                                    message_type: message.to_string(),
                                },
                                format!("Received new/updated secret: {name}"),
                                serde_json::to_value(message.clone()).ok(),
                            )
                            .await?;

                            // Send systemd sync message
                            if let Some(systemd_actor) = registry::where_is("systemd".to_string()) {
                                let systemd_actor: ActorRef<
                                    crate::actors::systemd::SystemdMessage,
                                > = systemd_actor.into();
                                if let Err(e) = systemd_actor.cast(
                                    crate::actors::systemd::SystemdMessage::SyncSecret {
                                        name: name.clone(),
                                        content: encrypted_data.clone(),
                                    },
                                ) {
                                    error!("Failed to send systemd sync message: {}", e);
                                }
                            }
                        } else {
                            debug!(
                                "Secret '{}' already up to date (same hash), no systemd sync needed",
                                name
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to store secret '{}' for current node: {}", name, e);
                    }
                }
            } else {
                info!(
                    "❌ Secret '{}' is NOT FOR current node (target: {}, current: {}), storing for gossip distribution only",
                    name,
                    target_node_id,
                    identity.id()
                );
                // Store secrets for other nodes too (for gossip distribution)
                match Secret::upsert(
                    name.clone(),
                    encrypted_data.clone(),
                    hash.clone(),
                    *target_node_id,
                )
                .await
                {
                    Ok(was_updated) => {
                        if was_updated {
                            debug!("Stored secret '{}' for node {}", name, target_node_id);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to store secret '{}' for node {}: {}",
                            name, target_node_id, e
                        );
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
            if from != *target_node_id {
                debug!(
                    "Ignoring secret deletion from {from} for secret '{name}' belonging to {target_node_id}"
                );
                return Ok(());
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
                        trace!(
                            "Secret '{}' was not found for deletion (already deleted?)",
                            name
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to delete secret '{}': {}", name, e);
                }
            }
        }
        PeerMessage::SecretSyncRequest { node_id, time, .. } => {
            trace!(%node_id, %time, "Handling PeerMessage::SecretSyncRequest");

            // Only handle sync requests for our own node
            if node_id == identity.id() {
                debug!("Received systemd sync request for current node");

                // Send systemd sync all message
                if let Some(systemd_actor) = registry::where_is("systemd".to_string()) {
                    let systemd_actor: ActorRef<crate::actors::systemd::SystemdMessage> =
                        systemd_actor.into();
                    if let Err(e) =
                        systemd_actor.cast(crate::actors::systemd::SystemdMessage::SyncAllSecrets)
                    {
                        error!("Failed to send systemd sync all message: {}", e);
                    }
                } else {
                    warn!("SystemdActor not found in registry for sync request");
                }
            } else {
                debug!(
                    "Ignoring sync request for node {} (not current node)",
                    node_id
                );
            }
        }
    }

    Ok(())
}

async fn send_all_secrets_to_node(
    target_node_id: NodeId,
    gossip_actor: &ActorRef<GossipMessage>,
) -> Result<()> {
    debug!(
        "🔍 Looking up all secrets in database to send to node {}",
        target_node_id
    );
    let all_secrets = Secret::list_all().await?;
    let secrets_count = all_secrets.len();
    info!(
        "📊 Found {} secrets in database to send to node {}",
        secrets_count, target_node_id
    );

    if secrets_count == 0 {
        info!("📪 No secrets to send to node {}", target_node_id);
        return Ok(());
    }

    for (i, secret) in all_secrets.iter().enumerate() {
        debug!(
            "📤 Preparing secret {}/{}: '{}' for node {} (target: {})",
            i + 1,
            secrets_count,
            secret.name,
            target_node_id,
            secret.target_node_id
        );

        let target_node_id_parsed = secret.get_target_node_id()?;
        let secret_message = PeerMessage::Secret {
            name: secret.name.clone(),
            encrypted_data: secret.encrypted_data.clone(),
            hash: secret.hash.clone(),
            target_node_id: target_node_id_parsed,
            time: Utc::now(),
        };

        debug!(
            "🏷️ Secret '{}': name={}, hash={}, target={}, encrypted_data_len={}",
            secret.name,
            secret.name,
            secret.hash,
            target_node_id_parsed,
            secret.encrypted_data.len()
        );

        // Log secret send event
        Event::log(
            EventType::PeerMessage {
                message_type: "SECRET".to_string(),
            },
            format!("Sending secret '{}' to node {target_node_id}", secret.name),
            serde_json::to_value(&secret_message).ok(),
        )
        .await?;

        debug!(
            "📨 Casting GossipMessage::SendPeerMessage for secret '{}'",
            secret.name
        );
        if let Err(e) = gossip_actor.cast(GossipMessage::SendPeerMessage(secret_message)) {
            error!("❌ Failed to send secret message '{}': {}", secret.name, e);
        } else {
            debug!(
                "✅ Successfully queued secret '{}' for sending",
                secret.name
            );
        }
    }

    info!(
        "📦 Completed queuing {} secrets to send to node {}",
        secrets_count, target_node_id
    );

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
