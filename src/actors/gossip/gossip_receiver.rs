use std::collections::HashMap;

use anyhow::{Result, anyhow};
use futures::TryStreamExt;
use iroh_gossip::api::GossipReceiver;
use ractor::{Actor, ActorCell};
use serde_json::json;
use tokio::{sync::watch, task::JoinHandle};
use tracing::{debug, error, trace, warn};

use crate::{
    actors::gossip::{GossipMessage, signing::SignedMessage},
    db::{AuditEvent, Peer},
};

pub struct GossipReceiverActor;

type Subscribers = HashMap<String, ActorCell>;

#[derive(Debug)]
pub enum GossipReceiverMessage {
    Subscribe(String),
    Unsubscribe(String),
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

        let subscribers: Subscribers = HashMap::new();
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
            GossipReceiverMessage::Subscribe(name) => {
                trace!(?name, "Subscribing to GossipReceiver");

                let actor = ractor::registry::where_is(name.clone())
                    .ok_or_else(|| anyhow!("Could not find actor"))?;

                state.subscribers.insert(name, actor);
                state.subscribers_tx.send(state.subscribers.clone())?;
            }
            GossipReceiverMessage::Unsubscribe(name) => {
                trace!(?name, "Unsubscribing to GossipReceiver");

                state.subscribers.remove(&name);
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
    subscribers_rx: &mut watch::Receiver<Subscribers>,
) -> Result<()> {
    trace!("Receiver task running");

    while let Some(event) = receiver.try_next().await? {
        trace!(?event, "Received event from Gossip");

        match event {
            iroh_gossip::api::Event::Received(message) => {
                match SignedMessage::<GossipMessage>::verify_and_decode(&message.content) {
                    Ok((sender_public_key, gossip_message)) => {
                        trace!(
                            ?sender_public_key,
                            ?gossip_message,
                            "Successfully verified and decoded gossip message"
                        );

                        Peer::bump_last_seen(message.delivered_from).await?;

                        for (_name, subscriber) in subscribers_rx.borrow().clone() {
                            trace!(?subscriber, "Sending verified message to subscriber");
                            if let Err(err) =
                                subscriber.send_message((sender_public_key, gossip_message))
                            {
                                warn!(?err, "Failed to send message to subscriber");
                            }
                        }
                    }
                    Err(err) => {
                        error!(
                            ?err,
                            from = ?message.delivered_from,
                            "Failed to verify signature or decode message - dropping"
                        );
                    }
                }
            }
            iroh_gossip::api::Event::NeighborUp(public_key) => {
                debug!(?public_key, "Neighbor Connected");

                // Add a shitty peer with a basically empty ticket
                // TODO: can we get the relay/IP data from somewhere else?
                Peer::insert_from_node_id(public_key).await?;

                Peer::bump_last_seen(public_key).await?;

                AuditEvent::log(
                    "GOSSIP_NEIGHBOR_UP".to_string(),
                    "Neighbor connected to gossip network".to_string(),
                    json!({
                        "node_id": public_key.to_string()
                    }),
                )
                .await?;
            }
            iroh_gossip::api::Event::NeighborDown(public_key) => {
                debug!(?public_key, "Neighbor Dropped");

                Peer::bump_last_seen(public_key).await?;

                AuditEvent::log(
                    "GOSSIP_NEIGHBOR_DOWN".to_string(),
                    "Neighbor disconnected from gossip network".to_string(),
                    json!({
                        "node_id": public_key.to_string()
                    }),
                )
                .await?;
            }
            iroh_gossip::api::Event::Lagged => {
                warn!("Iroh Gossip is lagging and we are missing messages!");

                AuditEvent::log(
                    "GOSSIP_LAGGED".to_string(),
                    "Gossip network is lagging and messages may be missing".to_string(),
                    json!({}),
                )
                .await?;
            }
        }
    }

    Ok(())
}
