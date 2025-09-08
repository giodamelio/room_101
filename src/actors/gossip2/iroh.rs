use std::time::Duration;

use anyhow::{Context, Result};
use iroh::{Endpoint, SecretKey, Watcher, node_info::NodeIdExt, protocol::Router};
use iroh_base::ticket::NodeTicket;
use iroh_gossip::{net::Gossip, proto::TopicId};
use ractor::Actor;
use tracing::debug;

use crate::{actors::gossip2::gossip_sender::GossipSenderMessage, db2::PeerExt, utils::topic_id};

pub struct IrohActor;

#[derive(Debug)]
pub enum IrohMessage {}

#[derive(Debug)]
pub struct IrohState {
    router: Router,
    gossip: Gossip,
}

impl Actor for IrohActor {
    type Msg = IrohMessage;
    type State = IrohState;
    type Arguments = (Vec<crate::db2::Peer>,);

    async fn pre_start(
        &self,
        myself: ractor::ActorRef<Self::Msg>,
        (bootstrap_peers,): Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        debug!("Starting Iroh Actor");

        let topic_id = topic_id!("ROOM_101");

        let secret_key = SecretKey::generate(rand::rngs::OsRng);

        let endpoint = Endpoint::builder()
            .secret_key(secret_key.clone())
            .discovery_n0()
            .bind()
            .await?;

        let ticket = NodeTicket::new(endpoint.node_addr().initialized().await);
        debug!(node_id = ?ticket.node_addr().node_id, z32_node_id = ?ticket.node_addr().node_id.to_z32(), ticket = ?ticket.to_string(), "Iroh Endpoint created");

        let gossip = Gossip::builder().spawn(endpoint.clone());

        let router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .spawn();

        debug!(
            ?topic_id,
            bootstrap_node_addrs_count = bootstrap_peers.len(),
            "Subscribing to Gossip"
        );

        // If we don't have any bootstrap peers don't wait
        let topic = if bootstrap_peers.is_empty() {
            // Don't wait for any peers
            gossip.subscribe(topic_id, vec![]).await?
        } else {
            // Add bootstrap peers to endpoint's address book first
            for peer in &bootstrap_peers.clone() {
                debug!(node_id = ?peer.node_id, "Adding bootstrap node to address book");
                endpoint.add_node_addr(peer.node_addr().clone())?;
            }

            // Wait for at least one peer to connect
            gossip
                .subscribe_and_join(topic_id, bootstrap_peers.clone().to_node_ids())
                .await?
        };

        let (sender, receiver) = topic.split();

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
        if !bootstrap_peers.is_empty() {
            gossip_sender_ref.send_message(GossipSenderMessage::JoinPeers(
                bootstrap_peers.to_node_ids(),
            ))?;
        }

        // Start the heartbeat
        Actor::spawn_linked(
            Some("heartbeat".into()),
            super::heartbeat::HeartbeatActor,
            (Duration::from_secs(10), gossip_sender_ref.clone()),
            myself.clone().into(),
        )
        .await
        .context("Failed to start Heartbeat Actor")?;

        Ok(IrohState { router, gossip })
    }

    async fn post_stop(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Shutdown the Iroh gossip and router
        state.gossip.shutdown().await?;
        state.router.shutdown().await?;

        Ok(())
    }
}
