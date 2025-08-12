use anyhow::{Context, Result, anyhow};
use iroh::{NodeId, SecretKey};
use rand::rngs;
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::{self, Any};
use surrealdb::{Datetime, Surreal};
use tracing::{debug, info, instrument};
use url::Url;

pub type DB = Surreal<Any>;

#[cfg(test)]
pub async fn new_test() -> DB {
    let db = any::connect("mem://").await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();
    db
}

pub async fn connect(url: &str) -> Result<DB> {
    info!("Connecting to SurrealDB: {}", url);

    let db = any::connect(url)
        .await
        .context("Failed to connect to SurrealDB")?;

    // Extract credentials from URL and authenticate
    if let Ok(parsed_url) = Url::parse(url) {
        let username = parsed_url.username();
        if let Some(password) = parsed_url.password() {
            if !username.is_empty() && !password.is_empty() {
                db.signin(surrealdb::opt::auth::Root { username, password })
                    .await
                    .context("Failed to authenticate with SurrealDB")?;
            }
        }
    }

    db.use_ns("room_101")
        .use_db("main")
        .await
        .context("Failed to set namespace/database")?;

    Ok(db)
}

pub async fn initialize_database(db: &DB) -> Result<()> {
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

    pub fn id(&self) -> NodeId {
        self.secret_key.public()
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
    pub hostname: Option<String>,
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
            hostname: None,
        };

        db.create("peer")
            .content(peer)
            .await
            .map_err(|e| anyhow!("Failed to create peer: {}", e))
    }

    pub async fn upsert_peer(
        db: &DB,
        node_id: NodeId,
        last_seen: Option<Datetime>,
        hostname: Option<String>,
    ) -> Result<()> {
        let mut upsert_data = serde_json::Map::new();

        // Always add node id
        upsert_data.insert(
            "node_id".to_string(),
            serde_json::to_value(node_id.to_string())?,
        );

        if let Some(ref last_seen) = last_seen {
            upsert_data.insert("last_seen".to_string(), serde_json::to_value(last_seen)?);
        }

        if let Some(ref hostname) = hostname {
            upsert_data.insert("hostname".to_string(), serde_json::to_value(hostname)?);
        }

        let _: Option<Peer> = db
            .upsert(("peer", node_id.to_string()))
            .merge(upsert_data)
            .await
            .with_context(|| format!("Failed to merge peer {node_id}"))?;

        Ok(())
    }

    pub async fn add_peers(db: &DB, node_ids: Vec<NodeId>) -> Result<()> {
        if node_ids.is_empty() {
            return Ok(());
        }

        // Use individual upserts with proper record IDs
        let peer_count = node_ids.len();
        for node_id in node_ids {
            Peer::upsert_peer(db, node_id, None, None).await?;
        }

        debug!("Successfully upserted {} peers", peer_count);

        Ok(())
    }
}
