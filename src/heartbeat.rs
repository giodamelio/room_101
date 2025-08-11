use std::time::Duration;

use anyhow::Result;
use ractor::{Actor, ActorProcessingErr, ActorRef, ActorStatus};
use tokio::time;
use tracing::{debug, info};

use crate::{db, management::HeartbeatMessage};

pub struct HeartbeatActor;

impl HeartbeatActor {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub struct HeartbeatState {
    db: db::DB,
    interval: Duration,
}

#[derive(Debug)]
pub struct HeartbeatArgs {
    pub db: db::DB,
    pub interval: Duration,
}

#[async_trait::async_trait]
impl Actor for HeartbeatActor {
    type Msg = HeartbeatMessage;
    type State = HeartbeatState;
    type Arguments = HeartbeatArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Heartbeat Actor Starting");

        // Start the heartbeat interval
        let interval = args.interval;
        let myself_clone = myself.clone();
        tokio::spawn(async move {
            let mut interval_timer = time::interval(interval);
            loop {
                interval_timer.tick().await;

                if myself_clone.get_status() == ActorStatus::Stopped {
                    break;
                }

                info!("BEAT");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        Ok(HeartbeatState {
            db: args.db,
            interval: args.interval,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            HeartbeatMessage::Shutdown => {
                debug!("This is my last heartbeat...");
                myself.stop(None);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        info!("Heartbeat Actor Stopping");
        Ok(())
    }
}
