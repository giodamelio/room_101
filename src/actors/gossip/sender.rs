use anyhow::Result;
use chrono::Utc;
use iroh::NodeId;
use tracing::debug;

use crate::db::{Identity, Secret};
use crate::network::protocol::{MessageSigner, PeerMessage};

// Direct helper functions for sending messages
pub async fn send_peer_message(
    peer_sender: &iroh_gossip::api::GossipSender,
    message: &PeerMessage,
    identity: &Identity,
) -> Result<()> {
    debug!("Preparing to send peer message: {:?}", message);
    let signed_bytes = message.sign(&identity.secret_key)?;
    debug!(
        "Message signed, broadcasting {} bytes to network",
        signed_bytes.len()
    );
    peer_sender.broadcast(signed_bytes.into()).await?;
    debug!(
        "Successfully broadcast peer message to network: {}",
        message
    );
    Ok(())
}

pub async fn announce_secret(
    peer_sender: &iroh_gossip::api::GossipSender,
    secret: &Secret,
    identity: &Identity,
) -> Result<()> {
    // Parse target_node_id from string to NodeId
    let target_node_id: NodeId = secret
        .target_node_id
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid target_node_id: {}", e))?;

    let message = PeerMessage::Secret {
        name: secret.name.clone(),
        hash: secret.hash.clone(),
        encrypted_data: secret.encrypted_data.clone(),
        target_node_id,
        time: Utc::now(),
    };

    let signed_bytes = message.sign(&identity.secret_key)?;
    peer_sender.broadcast(signed_bytes.into()).await?;
    debug!("Announced secret '{}' to network", secret.name);
    Ok(())
}

pub async fn announce_secret_deletion(
    peer_sender: &iroh_gossip::api::GossipSender,
    name: &str,
    hash: &str,
    target_node_id: NodeId,
    identity: &Identity,
) -> Result<()> {
    let message = PeerMessage::SecretDelete {
        name: name.to_string(),
        hash: hash.to_string(),
        target_node_id,
        time: Utc::now(),
    };

    let signed_bytes = message.sign(&identity.secret_key)?;
    peer_sender.broadcast(signed_bytes.into()).await?;
    debug!("Announced secret deletion '{}' to network", name);
    Ok(())
}

pub async fn send_secret_sync_request(
    peer_sender: &iroh_gossip::api::GossipSender,
    target_node_id: NodeId,
    identity: &Identity,
) -> Result<()> {
    let message = PeerMessage::SecretSyncRequest {
        node_id: target_node_id,
        time: Utc::now(),
    };

    let signed_bytes = message.sign(&identity.secret_key)?;
    peer_sender.broadcast(signed_bytes.into()).await?;
    debug!("Sent secret sync request to node {}", target_node_id);
    Ok(())
}
