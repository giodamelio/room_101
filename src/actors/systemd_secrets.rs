use std::{path::PathBuf, process::Stdio};

use anyhow::Result;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use serde_json::json;
use thiserror::Error;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{debug, error, trace};

use crate::db::AuditEvent;

pub struct SystemdSecretsActor;

#[derive(Debug, Clone)]
pub enum SystemdSecretsActorMessage {
    SetSecret(SystemdSecret, Vec<u8>),
    DeleteSecret(SystemdSecret),
}

#[derive(Debug, Clone)]
pub struct SystemdSecret {
    path: PathBuf,
    user: bool,
}

#[derive(Error, Debug)]
pub enum SystemdSecretsError {
    #[error("insufficient privilege for system secrets")]
    InsufficientPrivilege,

    #[error("systemd-creds command failed {0}")]
    CommandFailed(String),

    #[error("file error {0}")]
    IoError(#[from] std::io::Error),

    #[error("UTF8 error {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("ad-hoc error {0}")]
    AdHoc(#[from] anyhow::Error),
}

impl Actor for SystemdSecretsActor {
    type Msg = SystemdSecretsActorMessage;
    type State = ();
    type Arguments = ();

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        trace!(?message, "Handling Systemd Secrets message");

        match message {
            SystemdSecretsActorMessage::SetSecret(systemd_secret, data) => {
                systemd_secret.write(data).await?;
            }
            SystemdSecretsActorMessage::DeleteSecret(_systemd_secret) => todo!(),
        }

        Ok(())
    }

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<(), ractor::ActorProcessingErr> {
        Ok(())
    }
}

impl SystemdSecret {
    async fn write(self, content: Vec<u8>) -> Result<(), SystemdSecretsError> {
        trace!(path = ?self.path, "Writing secret");

        AuditEvent::log(
            "SYSTEMD_SECRET_WRITE".to_string(),
            "Writing systemd credential".to_string(),
            json!({
                "path": self.path.to_string_lossy(),
                "user": self.user,
            }),
        )
        .await?;

        // Create the new command we are going to run
        let mut cmd = Command::new("systemd-creds");
        cmd.arg("--json")
            .arg("short")
            .arg("encrypt")
            .arg("-")
            .arg(self.path);

        if self.user {
            cmd.arg("--user");
        }

        // Start the process
        let mut process = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write the input to stdin
        let mut stdin = process
            .stdin
            .take()
            .ok_or(SystemdSecretsError::CommandFailed(
                "Could not get systemd-creds stdin pipe".into(),
            ))?;
        stdin.write_all(&content).await?;
        stdin.shutdown().await?;
        drop(stdin);

        // Wait for the command to exit
        let output = process.wait_with_output().await?;
        debug!(?output, "systemd-creds output");

        // Convert error status to Result
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)?;

            if stderr.contains("io.systemd.InteractiveAuthenticationRequired") {
                return Err(SystemdSecretsError::InsufficientPrivilege);
            } else {
                return Err(SystemdSecretsError::CommandFailed(stderr));
            }
        }

        Ok(())
    }
}
