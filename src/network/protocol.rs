use std::fmt::Display;
use std::marker::PhantomData;

use anyhow::Result;
use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use iroh::{NodeId, PublicKey, SecretKey};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Serialize, Deserialize)]
pub struct SignedMessage<M: Serialize + DeserializeOwned> {
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

pub trait MessageSigner: Serialize + DeserializeOwned {
    fn sign(&self, secret_key: &SecretKey) -> Result<Vec<u8>> {
        SignedMessage::<Self>::sign_and_encode(secret_key, self)
    }
}

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

impl MessageSigner for PeerMessage {}

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
