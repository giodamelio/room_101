use std::sync::OnceLock;

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use iroh::{NodeId, SecretKey};
use rand::rngs;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::debug;
use uuid::Uuid;

// Helper functions for converting between Iroh types and database storage
fn secret_key_to_hex(key: &SecretKey) -> String {
    hex::encode(key.to_bytes())
}

fn secret_key_from_hex(hex_str: &str) -> Result<SecretKey> {
    let bytes = hex::decode(hex_str)?;
    let secret_key = SecretKey::try_from(&bytes[..])?;
    Ok(secret_key)
}

fn node_id_to_string(node_id: &NodeId) -> String {
    node_id.to_string()
}

fn node_id_from_string(s: &str) -> Result<NodeId> {
    Ok(s.parse::<NodeId>()?)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Identity {
    pub secret_key: SecretKey,
}

impl Identity {
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

    pub async fn get_or_create() -> anyhow::Result<Self> {
        let db = get_db();

        // Try to get existing identity
        let row = sqlx::query!("SELECT secret_key FROM identities LIMIT 1")
            .fetch_optional(db)
            .await?;

        if let Some(row) = row {
            let secret_key = secret_key_from_hex(&row.secret_key)?;
            Ok(Self { secret_key })
        } else {
            // Create new identity
            let new_identity = Self::new();

            let secret_key_hex = secret_key_to_hex(&new_identity.secret_key);
            sqlx::query!(
                "INSERT INTO identities (secret_key) VALUES (?)",
                secret_key_hex
            )
            .execute(db)
            .await?;

            debug!(
                "Generated new identity public_key={}",
                new_identity.secret_key.public()
            );
            Ok(new_identity)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Peer {
    pub node_id: String,                  // NodeId as string
    pub last_seen: Option<NaiveDateTime>, // SQLite datetime
    pub hostname: Option<String>,
}

impl Peer {
    pub async fn list() -> anyhow::Result<Vec<Self>> {
        let db = get_db();
        let peers = sqlx::query_as!(
            Peer,
            "SELECT node_id, last_seen, hostname FROM peers ORDER BY last_seen DESC"
        )
        .fetch_all(db)
        .await?;
        Ok(peers)
    }

    pub fn get_last_seen_utc(&self) -> Option<DateTime<Utc>> {
        self.last_seen
            .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
    }

    pub async fn create(node_id: NodeId) -> anyhow::Result<()> {
        let db = get_db();
        let node_id_str = node_id_to_string(&node_id);
        sqlx::query!(
            "INSERT INTO peers (node_id, last_seen, hostname) VALUES (?, ?, ?)",
            node_id_str,
            None::<NaiveDateTime>,
            None::<String>
        )
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn upsert_peer(
        node_id: NodeId,
        last_seen: Option<DateTime<Utc>>,
        hostname: Option<String>,
    ) -> anyhow::Result<()> {
        let db = get_db();
        let node_id_str = node_id_to_string(&node_id);
        let last_seen_naive = last_seen.map(|dt| dt.naive_utc());
        sqlx::query!(
            "INSERT INTO peers (node_id, last_seen, hostname) VALUES (?, ?, ?)
             ON CONFLICT(node_id) DO UPDATE SET
             last_seen = COALESCE(excluded.last_seen, peers.last_seen),
             hostname = COALESCE(excluded.hostname, peers.hostname)",
            node_id_str,
            last_seen_naive,
            hostname
        )
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn insert_bootstrap_nodes(nodes: Vec<NodeId>) -> anyhow::Result<()> {
        let db = get_db();
        let mut tx = db.begin().await?;

        for node_id in nodes {
            let node_id_str = node_id_to_string(&node_id);
            sqlx::query!(
                "INSERT OR IGNORE INTO peers (node_id, last_seen, hostname) VALUES (?, ?, ?)",
                node_id_str,
                None::<NaiveDateTime>,
                None::<String>
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_node_ids() -> anyhow::Result<Vec<NodeId>> {
        let db = get_db();
        let rows = sqlx::query!("SELECT node_id FROM peers")
            .fetch_all(db)
            .await?;

        let mut node_ids = Vec::new();
        for row in rows {
            let node_id = node_id_from_string(&row.node_id)?;
            node_ids.push(node_id);
        }
        Ok(node_ids)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    PeerMessage { message_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Event {
    pub id: String,         // UUID string
    pub event_type: String, // JSON serialized EventType
    pub message: String,
    pub time: NaiveDateTime, // SQLite datetime
    pub data: String,        // JSON data
}

impl Event {
    pub async fn list() -> anyhow::Result<Vec<Self>> {
        let db = get_db();
        let events = sqlx::query_as!(
            Event,
            "SELECT id, event_type, message, time, data FROM events ORDER BY time DESC LIMIT 100"
        )
        .fetch_all(db)
        .await?;
        Ok(events)
    }

    pub async fn log(
        event_type: EventType,
        message: String,
        data: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let db = get_db();
        let event_id = Uuid::new_v4().to_string();
        let event_type_json = serde_json::to_string(&event_type)?;
        let data_json = serde_json::to_string(&data.unwrap_or(serde_json::Value::Null))?;
        let now = Utc::now().naive_utc();

        sqlx::query!(
            "INSERT INTO events (id, event_type, message, time, data) VALUES (?, ?, ?, ?, ?)",
            event_id,
            event_type_json,
            message,
            now,
            data_json
        )
        .execute(db)
        .await?;

        Ok(())
    }

    // Helper methods to deserialize the stored JSON fields and convert types
    pub fn get_event_type(&self) -> Result<EventType> {
        Ok(serde_json::from_str(&self.event_type)?)
    }

    pub fn get_data(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_str(&self.data)?)
    }

    pub fn get_time_utc(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.time, Utc)
    }
}

static DATABASE: OnceLock<SqlitePool> = OnceLock::new();

pub async fn init_db(db_path: &str) -> Result<()> {
    let connection_string = if db_path == ":memory:" {
        return Err(anyhow::anyhow!(
            "In-memory database not allowed. Use init_test_db() for tests."
        ));
    } else {
        format!("sqlite:{db_path}?mode=rwc") // rwc = read/write/create
    };

    let pool = SqlitePool::connect(&connection_string).await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    DATABASE
        .set(pool)
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;
    Ok(())
}

#[cfg(test)]
pub async fn init_test_db() -> Result<()> {
    // For tests, just use the regular DATABASE but allow reinitialization
    if DATABASE.get().is_some() {
        // Database already exists, clear all data for test isolation
        let db = get_db();
        sqlx::query!("DELETE FROM events").execute(db).await?;
        sqlx::query!("DELETE FROM peers").execute(db).await?;
        sqlx::query!("DELETE FROM identities").execute(db).await?;
        return Ok(());
    }

    let pool = SqlitePool::connect("sqlite::memory:").await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    DATABASE
        .set(pool)
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;
    Ok(())
}

pub fn get_db() -> &'static SqlitePool {
    DATABASE
        .get()
        .expect("Database not initialized. Call init_db() first.")
}

pub async fn close_db() -> Result<()> {
    if let Some(pool) = DATABASE.get() {
        pool.close().await;
    }
    Ok(())
}
