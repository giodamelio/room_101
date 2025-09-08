use anyhow::Result;
use iroh::NodeId;
use iroh_gossip::api::GossipSender;
use ractor::Actor;
use tracing::{debug, trace};

use crate::actors::gossip2::GossipMessage;

pub struct GossipSenderActor;

#[derive(Debug)]
pub enum GossipSenderMessage {
    Broadcast(GossipMessage),
    JoinPeers(Vec<NodeId>),
}

impl From<GossipMessage> for GossipSenderMessage {
    fn from(value: GossipMessage) -> Self {
        GossipSenderMessage::Broadcast(value)
    }
}

#[derive(Debug)]
pub struct GossipSenderState {
    sender: GossipSender,
}

impl Actor for GossipSenderActor {
    type Msg = GossipSenderMessage;
    type State = GossipSenderState;
    type Arguments = (GossipSender,);

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        (sender,): Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        debug!("Starting GossipSender Actor");

        Ok(Self::State { sender })
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match message {
            GossipSenderMessage::Broadcast(data) => {
                trace!(?data, "Broadcasting data");
                let data_bytes = serde_json::to_vec(&data)?;
                state.sender.broadcast(data_bytes.into()).await?;
            }
            GossipSenderMessage::JoinPeers(bootstrap_peer_node_ids) => {
                trace!(?bootstrap_peer_node_ids, "Manually adding peer(s)");
                state.sender.join_peers(bootstrap_peer_node_ids).await?;
            }
        }

        Ok(())
    }
}
