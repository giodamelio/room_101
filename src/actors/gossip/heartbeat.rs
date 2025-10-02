use std::time::Duration;

use anyhow::Result;
use ractor::{Actor, ActorRef, time::send_interval};
use tracing::trace;

use crate::actors::gossip::{
    GossipMessage,
    gossip_sender::{self, GossipSenderMessage},
};

pub struct HeartbeatActor;

impl Actor for HeartbeatActor {
    type Msg = ();
    type State = ActorRef<GossipSenderMessage>;
    type Arguments = (Duration, ActorRef<GossipSenderMessage>);

    async fn pre_start(
        &self,
        myself: ractor::ActorRef<Self::Msg>,
        (duration, gossip_sender_ref): Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        send_interval(duration, myself.get_cell(), || ());
        Ok(gossip_sender_ref)
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        _message: Self::Msg,
        _gossip_sender_ref: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Send the heartbeat message
        let heartbeat = GossipMessage::heartbeat_now();
        trace!(?heartbeat, "Sending heartbeat");
        gossip_sender::send(heartbeat.clone()).await?;

        Ok(())
    }
}
