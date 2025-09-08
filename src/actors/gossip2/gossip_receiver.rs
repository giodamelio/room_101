use std::collections::HashSet;

use anyhow::Result;
use futures::TryStreamExt;
use iroh_gossip::api::GossipReceiver;
use ractor::{Actor, ActorRef};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{debug, trace, warn};

use crate::actors::gossip2::GossipMessage;

pub struct GossipReceiverActor;

type Subscriber = ActorRef<GossipMessage>;
type Subscribers = HashSet<Subscriber>;

#[derive(Debug)]
pub enum GossipReceiverMessage {
    Subscribe(Subscriber),
    Unsubscribe(Subscriber),
}

#[derive(Debug)]
pub struct GossipReceiverState {
    subscribers: Subscribers,
    subscribers_tx: watch::Sender<Subscribers>,
    handle: JoinHandle<Result<()>>,
}

impl Actor for GossipReceiverActor {
    type Msg = GossipReceiverMessage;
    type State = GossipReceiverState;
    type Arguments = (GossipReceiver,);

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        (mut receiver,): Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        debug!("Starting GossipSender Actor");

        let subscribers: Subscribers = HashSet::new();
        let (subscribers_tx, mut subscribers_rx) = watch::channel(subscribers.clone());
        let handle =
            tokio::spawn(async move { run_reciever(&mut receiver, &mut subscribers_rx).await });

        Ok(Self::State {
            subscribers,
            subscribers_tx,
            handle,
        })
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match message {
            GossipReceiverMessage::Subscribe(actor_ref) => {
                trace!(?actor_ref, "Subscribing to GossipReceiver");

                state.subscribers.insert(actor_ref);
                state.subscribers_tx.send(state.subscribers.clone())?;
            }
            GossipReceiverMessage::Unsubscribe(actor_ref) => {
                trace!(?actor_ref, "Unsubscribing to GossipReceiver");

                state.subscribers.remove(&actor_ref);
                state.subscribers_tx.send(state.subscribers.clone())?;
            }
        }

        Ok(())
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

async fn run_reciever(
    receiver: &mut GossipReceiver,
    subscribers_rx: &mut watch::Receiver<HashSet<Subscriber>>,
) -> Result<()> {
    trace!("Receiver task running");

    while let Some(event) = receiver.try_next().await? {
        trace!(?event, "Received event from Gossip");

        match event {
            iroh_gossip::api::Event::Received(message) => {
                for subscriber in subscribers_rx.borrow().clone() {
                    warn!(
                        ?subscriber,
                        ?message,
                        "I should be sending this to the subscriber"
                    );
                    // subscriber.send_message(message)
                }
            }
            iroh_gossip::api::Event::NeighborUp(public_key) => {
                trace!(?public_key, "Neighbor Connected");
            }
            iroh_gossip::api::Event::NeighborDown(public_key) => {
                trace!(?public_key, "Neighbor Dropped");
            }
            iroh_gossip::api::Event::Lagged => {
                warn!("Iroh Gossip is lagging and we are missing messages!");
            }
        }
    }

    Ok(())
}
