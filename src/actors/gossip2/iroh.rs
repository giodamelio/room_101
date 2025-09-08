use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::{Endpoint, NodeId, SecretKey, Watcher};
use iroh_base::ticket::NodeTicket;
use iroh_gossip::{
    api::{GossipReceiver, GossipSender},
    net::Gossip,
    proto::TopicId,
};
use ractor::Actor;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::{actors::gossip2::gossip_sender::GossipSenderMessage, utils::topic_id};

pub struct IrohActor;

#[derive(Debug)]
pub enum IrohMessage {}

#[derive(Debug)]
pub struct IrohState {
    handle: JoinHandle<Result<()>>,
}

impl Actor for IrohActor {
    type Msg = IrohMessage;
    type State = IrohState;
    type Arguments = (Vec<NodeId>,);

    async fn pre_start(
        &self,
        myself: ractor::ActorRef<Self::Msg>,
        (bootstrap_node_ids,): Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        debug!("Starting Iroh Actor");

        let topic_id = topic_id!("ROOM_101");

        let bootstrap_node_ids_prime = bootstrap_node_ids.clone();
        let handle = tokio::spawn(async move {
            match run_iroh_network(topic_id.clone(), bootstrap_node_ids_prime).await {
                Err(err) => {
                    bail!(err.context("Iroh Actor Failed"));
                }
                Ok((sender, receiver)) => {
                    debug!(topic_id = ?topic_id, "Topic Attached");

                    // Run the actor that handles sending messages
                    let (gossip_sender_ref, _gossip_sender_handle) = Actor::spawn_linked(
                        Some("gossip_sender".into()),
                        super::gossip_sender::GossipSenderActor,
                        (sender,),
                        myself.clone().into(),
                    )
                    .await
                    .context("Failed to start GossipSender Actor")?;

                    // Run the actor that handles receiving messages
                    Actor::spawn_linked(
                        Some("gossip_receiver".into()),
                        super::gossip_receiver::GossipReceiverActor,
                        (receiver,),
                        myself.clone().into(),
                    )
                    .await
                    .context("Failed to start GossipReceiver Actor")?;

                    // Manually add the bootstrap nodes if they exist
                    if !bootstrap_node_ids.is_empty() {
                        gossip_sender_ref
                            .send_message(GossipSenderMessage::JoinPeers(bootstrap_node_ids))?;
                    }

                    // Start the heartbeat
                    Actor::spawn_linked(
                        Some("heartbeat".into()),
                        super::heartbeat::HeartbeatActor,
                        Duration::from_secs(1),
                        myself.clone().into(),
                    )
                    .await
                    .context("Failed to start Heartbeat Actor")?;

                    Ok(())
                }
            }
        });

        Ok(IrohState { handle })
    }

    async fn post_stop(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Kill the Iroh task
        state.handle.abort();

        Ok(())
    }
}

async fn run_iroh_network(
    topic_id: TopicId,
    bootstrap_node_ids: Vec<NodeId>,
) -> Result<(GossipSender, GossipReceiver)> {
    // Generate a new random secret key
    // TODO: this should come in via the args
    let secret_key = SecretKey::generate(rand::rngs::OsRng);

    let endpoint = Endpoint::builder()
        .secret_key(secret_key.clone())
        .discovery_n0()
        .bind()
        .await?;

    let ticket = NodeTicket::new(endpoint.node_addr().initialized().await);
    debug!(node_id = ?endpoint.node_id(), ticket = ?ticket.to_string(), "Iroh Endpoint created");

    let gossip = Gossip::builder().spawn(endpoint.clone());

    let _router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    // If we don't have any bootstrap peers don't wait
    let topic = if bootstrap_node_ids.is_empty() {
        // Don't wait for any peers
        gossip.subscribe(topic_id, vec![]).await?
    } else {
        // Wait for at least one peer to connect
        gossip
            .subscribe_and_join(topic_id, bootstrap_node_ids)
            .await?
    };

    Ok(topic.split())
}
