use iroh::{NodeId, SecretKey};
use miette::{IntoDiagnostic, Result};
use rand::rngs;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::{Datetime, Surreal};
use tracing::{debug, instrument, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub secret_key: SecretKey,
}

impl Identity {
    #[instrument]
    pub fn new() -> Self {
        debug!("Generating new identity with random secret key");
        let identity = Self {
            secret_key: SecretKey::generate(rngs::OsRng),
        };
        debug!(public_key = %identity.secret_key.public(), "Generated new identity");
        identity
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    id: NodeId,
    last_seen: Datetime,
}

pub type DB = Surreal<Db>;

pub async fn get_peers(db: &DB) -> Result<Vec<Peer>> {
    db.select("peer").await.into_diagnostic()
}
