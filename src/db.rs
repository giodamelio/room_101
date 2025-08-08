use anyhow::{Context, Result};
use iroh::{NodeId, SecretKey};
use rand::rngs;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::{Datetime, Surreal};
use tracing::{debug, instrument, warn};

pub type DB = Surreal<Db>;

pub async fn initialize_database(db: &DB) -> Result<()> {
    // Create unique index on node_id field for peers table
    db.query("DEFINE INDEX unique_node_id ON TABLE peer COLUMNS node_id UNIQUE")
        .await?;

    Ok(())
}

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
    #[serde(
        serialize_with = "serialize_node_id",
        deserialize_with = "deserialize_node_id"
    )]
    pub node_id: NodeId,
    pub last_seen: Option<Datetime>,
}

fn serialize_node_id<S>(node_id: &NodeId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&node_id.to_string())
}

fn deserialize_node_id<'de, D>(deserializer: D) -> Result<NodeId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

impl Peer {
    pub async fn list(db: &DB) -> Result<Vec<Peer>> {
        db.select("peer")
            .await
            .context("Failed to select peers from database")
    }

    pub async fn create(db: &DB, node_id: NodeId) -> Result<Option<Peer>> {
        let peer = Peer {
            node_id,
            last_seen: None,
        };

        let result: Option<Peer> = db
            .create("peer")
            .content(peer)
            .await
            .with_context(|| format!("Failed to create peer in database: {node_id}"))?;

        Ok(result)
    }
}
