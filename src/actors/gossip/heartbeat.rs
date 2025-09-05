use chrono::Utc;
use ractor::ActorRef;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error};

use super::GossipMessage;
use crate::db::{Identity, age_public_key_to_string};
use crate::network::protocol::PeerMessage;

pub async fn start_heartbeat_loop(identity: Identity, gossip_actor: ActorRef<GossipMessage>) {
    debug!("Starting heartbeat loop");

    loop {
        sleep(Duration::from_secs(10)).await;

        let heartbeat_message = PeerMessage::Heartbeat {
            node_id: identity.id(),
            ticket: identity.ticket(),
            time: Utc::now(),
            age_public_key: age_public_key_to_string(&identity.age_key),
        };

        if let Err(e) = gossip_actor.cast(GossipMessage::SendPeerMessage(heartbeat_message)) {
            error!("Failed to send heartbeat message: {}", e);
            break;
        }
    }

    debug!("Heartbeat loop stopped");
}
