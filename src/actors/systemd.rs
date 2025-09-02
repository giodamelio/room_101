use anyhow::Result;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing::{debug, error, info, warn};

use crate::SystemdSecretsConfig;
use crate::db::{Identity, Secret, decrypt_secret_for_identity};

pub struct SystemdActor;

#[derive(Debug)]
pub enum SystemdMessage {
    SyncSecret { name: String, content: Vec<u8> },
    SyncAllSecrets,
}

impl Actor for SystemdActor {
    type Msg = SystemdMessage;
    type State = SystemdSecretsConfig;
    type Arguments = SystemdSecretsConfig;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("Starting SystemdActor");
        debug!(
            "SystemD secrets config: path='{}', user_scope={}",
            config.path, config.user_scope
        );

        // Check systemd-creds availability at startup
        if crate::systemd_secrets::is_available() {
            info!("systemd-creds is available - systemd secrets integration enabled");
        } else {
            warn!("systemd-creds is NOT available - systemd secrets integration disabled");
            warn!(
                "To enable systemd secrets, install systemd-creds (usually part of systemd >= 248)"
            );
        }

        Ok(config)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SystemdMessage::SyncSecret { name, content } => {
                self.handle_sync_secret(name, content, state).await?;
            }
            SystemdMessage::SyncAllSecrets => {
                self.handle_sync_all_secrets(state).await?;
            }
        }
        Ok(())
    }
}

impl SystemdActor {
    async fn handle_sync_secret(
        &self,
        name: String,
        encrypted_content: Vec<u8>,
        config: &SystemdSecretsConfig,
    ) -> Result<(), ActorProcessingErr> {
        debug!("Syncing secret '{}' to systemd", name);

        // Get identity to decrypt the secret
        let identity = Identity::get_or_create().await.map_err(|e| {
            Box::new(std::io::Error::other(format!(
                "Failed to get identity: {e}"
            ))) as ActorProcessingErr
        })?;

        // Decrypt the secret content
        let decrypted_content = decrypt_secret_for_identity(&encrypted_content, &identity)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::other(format!(
                    "Failed to decrypt secret '{name}': {e}"
                ))) as ActorProcessingErr
            })?;

        // Write to systemd credentials
        let cred_path = format!("{}/{}.cred", config.path, name);
        match crate::systemd_secrets::write_secret(
            &name,
            &decrypted_content,
            &cred_path,
            config.user_scope,
        )
        .await
        {
            Ok(()) => {
                debug!("Successfully synced secret '{}' to systemd", name);
            }
            Err(e) => {
                error!("Failed to sync secret '{}' to systemd: {}", name, e);
            }
        }

        Ok(())
    }

    async fn handle_sync_all_secrets(
        &self,
        config: &SystemdSecretsConfig,
    ) -> Result<(), ActorProcessingErr> {
        debug!("Syncing all secrets to systemd");

        let identity = Identity::get_or_create().await.map_err(|e| {
            Box::new(std::io::Error::other(format!(
                "Failed to get identity: {e}"
            ))) as ActorProcessingErr
        })?;
        let current_node_id = identity.id();

        // Get all secrets for the current node
        let all_secrets = Secret::list_all().await.map_err(|e| {
            Box::new(std::io::Error::other(format!(
                "Failed to list secrets: {e}"
            ))) as ActorProcessingErr
        })?;
        let my_secrets: Vec<_> = all_secrets
            .into_iter()
            .filter(|secret| {
                secret
                    .get_target_node_id()
                    .map(|id| id == current_node_id)
                    .unwrap_or(false)
            })
            .collect();

        debug!(
            "Syncing {} secrets to systemd for current node",
            my_secrets.len()
        );

        let mut success_count = 0;
        let mut error_count = 0;

        for secret in my_secrets {
            match decrypt_secret_for_identity(&secret.encrypted_data, &identity).await {
                Ok(decrypted_content) => {
                    let cred_path = format!("{}/{}.cred", config.path, secret.name);
                    match crate::systemd_secrets::write_secret(
                        &secret.name,
                        &decrypted_content,
                        &cred_path,
                        config.user_scope,
                    )
                    .await
                    {
                        Ok(()) => {
                            debug!("Synced secret '{}' to systemd", secret.name);
                            success_count += 1;
                        }
                        Err(e) => {
                            error!("Failed to sync secret '{}' to systemd: {}", secret.name, e);
                            error_count += 1;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to decrypt secret '{}' for systemd sync: {}",
                        secret.name, e
                    );
                    error_count += 1;
                }
            }
        }

        info!(
            "Systemd sync complete: {} success, {} errors",
            success_count, error_count
        );
        Ok(())
    }
}
