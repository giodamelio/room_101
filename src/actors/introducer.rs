use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing::info;

use crate::{
    actors::gossip::{
        GossipEvent, GossipMessage, gossip_receiver::GossipReceiverMessage, gossip_sender,
    },
    db::{Identity, Peer},
};

pub struct IntroducerActor;

impl Actor for IntroducerActor {
    type Msg = GossipEvent;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting Introducer Actor");

        let actor = ractor::registry::where_is("gossip_receiver".to_string())
            .ok_or_else(|| anyhow!("Could not find gossip_receiver actor"))?;

        actor.send_message::<GossipReceiverMessage>(GossipReceiverMessage::Subscribe(
            "introducer".to_string(),
        ))?;

        Ok(())
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match message {
            GossipEvent::Message(_sender_node_id, gossip_message) => {
                if let GossipMessage::Introduction {
                    node_id,
                    ticket,
                    hostname,
                    age_public_key,
                    ..
                } = gossip_message
                {
                    Peer::update_from_introduction(node_id, ticket, hostname, age_public_key)
                        .await?;
                }
            }
            GossipEvent::NeighborUp(node_id) => {
                if !Peer::is_known(node_id).await?
                    && let Some(_peer) = Peer::insert_from_node_id(node_id).await?
                {
                    let identity = Identity::get().await?;
                    let ticket = crate::actors::gossip::node_ticket()
                        .context("Node ticket not yet initialized")?;

                    let introduction = GossipMessage::Introduction {
                        node_id: identity.id(),
                        ticket,
                        time: Utc::now(),
                        hostname: None,
                        age_public_key: identity.age_key.to_public().to_string(),
                    };

                    gossip_sender::send(introduction).await?;
                }
            }
            GossipEvent::NeighborDown(_node_id) => {}
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let actor = ractor::registry::where_is("gossip_receiver".to_string())
            .ok_or_else(|| anyhow!("Could not find gossip_receiver actor"))?;

        actor.send_message::<GossipReceiverMessage>(GossipReceiverMessage::Unsubscribe(
            "introducer".to_string(),
        ))?;

        Ok(())
    }
}
