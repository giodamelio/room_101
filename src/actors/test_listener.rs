use anyhow::{Result, anyhow};
use iroh::NodeId;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing::{info, warn};

use crate::actors::gossip::{GossipMessage, gossip_receiver::GossipReceiverMessage};

pub struct TestListenerActor;

impl Actor for TestListenerActor {
    type Msg = (NodeId, GossipMessage);
    type State = usize;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting Test Listener");

        let actor = ractor::registry::where_is("gossip_receiver".to_string())
            .ok_or_else(|| anyhow!("Could not find actor"))?;

        actor.send_message::<GossipReceiverMessage>(GossipReceiverMessage::Subscribe(
            "test_listener".to_string(),
        ))?;

        warn!("Listening for 10 messages");

        Ok(0)
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        warn!(count = ?state, gossip_message = ?message, "Got message");

        let actor = ractor::registry::where_is("gossip_receiver".to_string())
            .ok_or_else(|| anyhow!("Could not find actor"))?;

        if *state >= 10 {
            actor.send_message::<GossipReceiverMessage>(GossipReceiverMessage::Unsubscribe(
                "test_listener".to_string(),
            ))?;
        }

        *state += 1;

        Ok(())
    }
}
