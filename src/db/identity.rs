use age::x25519::Identity as AgeIdentity;
use anyhow::{Result, anyhow, Context};
use iroh::{NodeId, SecretKey};
use rand::rngs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::debug;

use crate::db::AuditEvent;

use super::db;

#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    pub secret_key: SecretKey,
    #[serde(with = "crate::custom_serde::age_identity_serde")]
    pub age_key: AgeIdentity,
}

impl Identity {
    pub fn id(&self) -> NodeId {
        self.secret_key.public()
    }

    #[cfg(test)]
    pub fn from_string(secret_key_str: &str, age_key_str: &str) -> Result<Identity> {
        let secret_key = secret_key_str.parse::<SecretKey>()
            .context("Failed to parse SecretKey from string")?;
        let age_key = age_key_str.parse::<AgeIdentity>()
            .map_err(|e| anyhow!("Failed to parse Age Identity from string: {e}"))?;
        
        Ok(Identity {
            secret_key,
            age_key,
        })
    }

    pub async fn get() -> Result<Identity> {
        db().await?
            .select(("identity", "self"))
            .await?
            .ok_or(anyhow!("Please have an identity crisis"))
    }

    pub fn generate() -> Identity {
        let identity = Self {
            secret_key: SecretKey::generate(rngs::OsRng),
            age_key: AgeIdentity::generate(),
        };
        debug!(public_key = %identity.secret_key.public(), "Generated new identity");
        identity
    }

    pub async fn generate_and_create() -> Result<Identity> {
        let ident = Self::generate();

        AuditEvent::log(
            "IDENTITY_GENERATED".to_string(),
            "Generated new identity".to_string(),
            json!({}),
        )
        .await?;

        db().await?
            .create(("identity", "self"))
            .content(ident)
            .await?
            .ok_or(anyhow!("Failed to create identity self"))
    }

    pub async fn get_or_generate() -> Result<Identity> {
        let ident: Option<Identity> = db().await?.select(("identity", "self")).await?;
        Ok(match ident {
            Some(identity) => identity,
            None => Self::generate_and_create().await?,
        })
    }
}
