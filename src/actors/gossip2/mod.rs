use serde::{Deserialize, Serialize};

pub mod gossip_receiver;
pub mod gossip_sender;
pub mod iroh;

#[derive(Debug, Serialize, Deserialize)]
pub enum GossipMessage {
    Ping,
    Pong,
}
