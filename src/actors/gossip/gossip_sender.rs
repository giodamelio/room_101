use anyhow::{Context, Result};
use iroh::NodeId;
use iroh_gossip::api::GossipSender;
use ractor::{Actor, registry};
use tracing::{debug, trace};

use crate::actors::gossip::{GossipMessage, signing::SignedMessage};
use crate::db::Identity;

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
                trace!(?data, "Broadcasting signed data");
                let identity = Identity::get_or_generate().await?;
                let signed_bytes = SignedMessage::sign_and_encode(&identity.secret_key, &data)?;
                state.sender.broadcast(signed_bytes.into()).await?;
            }
            GossipSenderMessage::JoinPeers(bootstrap_peer_node_ids) => {
                trace!(?bootstrap_peer_node_ids, "Manually adding peer(s)");
                state.sender.join_peers(bootstrap_peer_node_ids).await?;
            }
        }

        Ok(())
    }
}

pub async fn send(message: GossipMessage) -> Result<()> {
    let gossip_sender_ref = registry::where_is("gossip_sender".to_string())
        .context("Could not get Gossip Sender Actor")?;

    let wrapped_message = GossipSenderMessage::Broadcast(message);
    gossip_sender_ref.send_message(wrapped_message)?;

    Ok(())
}
