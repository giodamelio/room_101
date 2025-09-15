use std::fmt::Display;

use chrono::{DateTime, Utc};
use iroh::NodeId;
use iroh_base::ticket::NodeTicket;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeerMessage {
    #[serde(rename = "JOINED")]
    Joined {
        node_id: NodeId,
        ticket: NodeTicket,
        time: DateTime<Utc>,
        hostname: Option<String>,
        age_public_key: String,
    },
    #[serde(rename = "LEAVING")]
    Leaving {
        node_id: NodeId,
        ticket: NodeTicket,
        time: DateTime<Utc>,
    },
    #[serde(rename = "INTRODUCTION")]
    Introduction {
        node_id: NodeId,
        ticket: NodeTicket,
        time: DateTime<Utc>,
        hostname: Option<String>,
        age_public_key: String,
    },
    #[serde(rename = "HEARTBEAT")]
    Heartbeat {
        node_id: NodeId,
        ticket: NodeTicket,
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
