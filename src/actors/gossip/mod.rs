use ::iroh::NodeId;
use chrono::{DateTime, Utc};
use iroh_base::ticket::NodeTicket;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

pub mod gossip_receiver;
pub mod gossip_sender;
pub mod heartbeat;
pub mod iroh;
pub mod signing;

#[derive(Debug, Clone)]
pub enum GossipEvent {
    Message(NodeId, GossipMessage),
    NeighborUp(NodeId),
    NeighborDown(NodeId),
}

static NODE_TICKET: OnceCell<NodeTicket> = OnceCell::const_new();

/// Get the full ticket of the current node
pub fn node_ticket() -> Option<NodeTicket> {
    NODE_TICKET.get().cloned()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum GossipMessage {
    Ping,
    Pong,
    Heartbeat {
        sent_at: chrono::DateTime<chrono::Utc>,
    },
    Introduction {
        node_id: NodeId,
        ticket: NodeTicket,
        time: DateTime<Utc>,
        hostname: Option<String>,
        age_public_key: String,
    },
}

impl GossipMessage {
    pub fn heartbeat_now() -> GossipMessage {
        GossipMessage::Heartbeat {
            sent_at: Utc::now(),
        }
    }
}
