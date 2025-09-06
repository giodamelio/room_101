use chrono::Utc;
use serde::{Deserialize, Serialize};

pub mod gossip_receiver;
pub mod gossip_sender;
pub mod heartbeat;
pub mod iroh;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum GossipMessage {
    Ping,
    Pong,
    Heartbeat {
        sent_at: chrono::DateTime<chrono::Utc>,
    },
}

impl GossipMessage {
    fn heartbeat_now() -> GossipMessage {
        GossipMessage::Heartbeat {
            sent_at: Utc::now(),
        }
    }
}
