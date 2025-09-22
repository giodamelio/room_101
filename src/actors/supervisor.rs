use anyhow::Result;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing::info;

use crate::db::Peer;

#[derive(Debug, Clone)]
pub struct SystemdSecretsConfig {
    pub path: String,
    pub user_scope: bool,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub systemd_config: SystemdSecretsConfig,
}

pub struct SupervisorActor;

#[derive(Debug)]
pub enum SupervisorMessage {
    Shutdown,
}

impl Actor for SupervisorActor {
    type Msg = SupervisorMessage;
    type State = ();
    type Arguments = AppConfig;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting SupervisorActor with linked children");

        // Get all the existing peers
        let peers = Peer::list().await?;

        let (_iroh_actor, _iroh_handle) = Actor::spawn_linked(
            Some("iroh".into()),
            super::gossip::iroh::IrohActor,
            (peers,),
            myself.clone().into(),
        )
        .await?;

        // Start a actor that we are using to test the gossip subscriber
        let (_test_listener_actor, _test_listener_handle) = Actor::spawn_linked(
            Some("test_listener".into()),
            super::test_listener::TestListenerActor,
            (),
            myself.clone().into(),
        )
        .await?;

        let (_systemd_secrets_actor, _systemd_secrets_handle) = Actor::spawn_linked(
            Some("systemd_secrets".into()),
            super::systemd_secrets::SystemdSecretsActor,
            (),
            myself.clone().into(),
        )
        .await?;

        info!("All actors started successfully");
        Ok(())
    }
}
