use anyhow::{Context, Result, bail};
use distributed_topic_tracker::{
    AutoDiscoveryGossip, GossipReceiver, GossipSender, RecordPublisher, TopicId,
};
use iroh::{Endpoint, NodeId, SecretKey, Watcher};
use iroh_base::ticket::NodeTicket;
use iroh_gossip::net::Gossip;
use ractor::Actor;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::actors::gossip2::{GossipMessage, gossip_sender::GossipSenderMessage};

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

        let topic_id = TopicId::new("ROOM_101".to_string());

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

                    // Send some test messages to the gossip
                    gossip_sender_ref.send_message(GossipMessage::Ping.into())?;
                    gossip_sender_ref.send_message(GossipMessage::Pong.into())?;
                    gossip_sender_ref.send_message(GossipMessage::Ping.into())?;
                    gossip_sender_ref.send_message(GossipMessage::Pong.into())?;

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

    // TODO: this should come in via the args
    let initial_secret = b"super-duper-secret".to_vec();
    let record_publisher = RecordPublisher::new(
        topic_id,
        endpoint.node_id(),
        secret_key.secret().clone(),
        None,
        initial_secret,
    );

    // If we don't have any bootstrap peers don't wait
    let topic = if bootstrap_node_ids.is_empty() {
        gossip
            .subscribe_and_join_with_auto_discovery_no_wait(record_publisher)
            .await?
    } else {
        gossip
            .subscribe_and_join_with_auto_discovery(record_publisher)
            .await?
    };

    topic.split().await
}
