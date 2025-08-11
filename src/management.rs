use anyhow::Result;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing::{debug, info};

#[derive(Debug)]
pub enum ActorType {
    Heartbeat(ActorRef<HeartbeatMessage>),
    Webserver(ActorRef<WebServerMessage>),
    Iroh(ActorRef<IrohMessage>),
}

#[derive(Debug, Clone)]
pub struct Shutdown;

pub struct Management;

impl Management {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub enum ManagementMessage {
    RegisterActor(ActorType),
    Shutdown,
}

pub struct ManagementState {
    minions: Vec<ActorType>,
}

#[async_trait::async_trait]
impl Actor for Management {
    type Msg = ManagementMessage;
    type State = ManagementState;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("The manager has arrived");
        Ok(ManagementState {
            minions: Vec::new(),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ManagementMessage::RegisterActor(actor_type) => {
                debug!("Management is registering actor: {actor_type:#?}");
                state.minions.push(actor_type);
            }
            ManagementMessage::Shutdown => {
                debug!("Management is handling shutdown");

                for minion in state.minions.iter() {
                    match minion {
                        ActorType::Heartbeat(actor_ref) => {
                            if let Err(e) = actor_ref.send_message(HeartbeatMessage::Shutdown) {
                                tracing::error!(
                                    "Failed to send shutdown to heartbeat actor: {}",
                                    e
                                );
                            }
                        }
                        ActorType::Webserver(actor_ref) => {
                            if let Err(e) = actor_ref.send_message(WebServerMessage::Shutdown) {
                                tracing::error!(
                                    "Failed to send shutdown to webserver actor: {}",
                                    e
                                );
                            }
                        }
                        ActorType::Iroh(actor_ref) => {
                            if let Err(e) = actor_ref.send_message(IrohMessage::Shutdown) {
                                tracing::error!("Failed to send shutdown to iroh actor: {}", e);
                            }
                        }
                    }
                }
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
        info!("The office is closed");
        Ok(())
    }
}

// Message types for other actors
#[derive(Debug, Clone)]
pub enum HeartbeatMessage {
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum WebServerMessage {
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum IrohMessage {
    Shutdown,
}
