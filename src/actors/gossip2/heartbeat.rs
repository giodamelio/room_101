use std::time::Duration;

use anyhow::{Result, anyhow};
use ractor::{Actor, ActorCell, time::send_interval};
use tracing::trace;

use crate::actors::gossip2::GossipMessage;

pub struct HeartbeatActor;

impl Actor for HeartbeatActor {
    type Msg = ();
    type State = ActorCell;
    type Arguments = Duration;

    async fn pre_start(
        &self,
        myself: ractor::ActorRef<Self::Msg>,
        duration: Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        let actor_cell = ractor::registry::where_is("gossip_sender".to_string())
            .ok_or(anyhow!("No GossipSender Actor"))?;

        send_interval(duration, myself.get_cell(), || ());

        Ok(actor_cell)
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        _message: Self::Msg,
        actor_cell: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Send the heartbeat message
        let heartbeat = GossipMessage::heartbeat_now();
        trace!(?heartbeat, "Heartbeat");
        actor_cell.send_message(heartbeat)?;

        Ok(())
    }
}
