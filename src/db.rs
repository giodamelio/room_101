use std::sync::OnceLock;

use age::secrecy::ExposeSecret;
use age::x25519::Identity as AgeIdentity;
use age::{Decryptor, Encryptor};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use iroh::{NodeId, SecretKey};
use rand::rngs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::io::{Read as StdRead, Write as StdWrite};
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

fn age_identity_to_string(age_key: &AgeIdentity) -> String {
    age_key.to_string().expose_secret().to_string()
}

fn age_identity_from_str(s: &str) -> Result<AgeIdentity, anyhow::Error> {
    s.parse::<AgeIdentity>()
        .map_err(anyhow::Error::msg)
        .context("Failed to parse AgeIdentity")
}

pub fn age_public_key_to_string(age_key: &AgeIdentity) -> String {
    age_key.to_public().to_string()
}

/// Custom serde serialization module for AgeIdentity
///
/// Provides safe serialization/deserialization for age::x25519::Identity using
/// the built-in to_string() and from_str() methods. The age identity is serialized
/// as a string in Bech32 format with "AGE-SECRET-KEY-1" prefix.
mod age_identity_serde {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(age_key: &AgeIdentity, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        age_key.to_string().expose_secret().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AgeIdentity, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<AgeIdentity>().map_err(serde::de::Error::custom)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_age_identity_serialize() {
            let age_identity = AgeIdentity::generate();
            let serialized =
                serde_json::to_string(&age_identity.to_string().expose_secret()).unwrap();

            // Verify it starts with the expected prefix in the serialized string
            assert!(serialized.contains("AGE-SECRET-KEY-1"));
        }

        #[test]
        fn test_age_identity_deserialize() {
            let age_identity = AgeIdentity::generate();
            let identity_string = age_identity.to_string().expose_secret().to_string();

            // Test that we can parse it back
            let parsed = identity_string.parse::<AgeIdentity>().unwrap();

            // Verify they produce the same string representation
            assert_eq!(
                age_identity.to_string().expose_secret(),
                parsed.to_string().expose_secret()
            );
        }

        #[test]
        fn test_age_identity_serde_roundtrip() {
            let original = AgeIdentity::generate();

            // Serialize
            let mut serializer = serde_json::Serializer::new(Vec::new());
            serialize(&original, &mut serializer).unwrap();
            let serialized_bytes = serializer.into_inner();
            let serialized_str = String::from_utf8(serialized_bytes).unwrap();

            // Deserialize
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized = deserialize(&mut deserializer).unwrap();

            // Verify round-trip equality
            assert_eq!(
                original.to_string().expose_secret(),
                deserialized.to_string().expose_secret()
            );
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Identity {
    pub secret_key: SecretKey,
    #[serde(with = "age_identity_serde")]
    pub age_key: AgeIdentity,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("secret_key", &"[SecretKey]")
            .field("age_key", &"[AgePrivateKey]")
            .finish()
    }
}

impl Identity {
    pub fn new() -> Self {
        debug!("Generating new identity with random secret key");
        let identity = Self {
            secret_key: SecretKey::generate(rngs::OsRng),
            age_key: AgeIdentity::generate(),
        };
        debug!(public_key = %identity.secret_key.public(), "Generated new identity");
        identity
    }

    pub fn id(&self) -> NodeId {
        self.secret_key.public()
    }

    pub async fn get() -> anyhow::Result<Self> {
        let db = get_db();

        // Try to get existing identity
        let row = sqlx::query!("SELECT secret_key, age_key FROM identities LIMIT 1")
            .fetch_optional(db)
            .await?;

        if let Some(row) = row {
            let secret_key = secret_key_from_hex(&row.secret_key)?;
            let age_key = if let Some(age_key_str) = &row.age_key {
                age_identity_from_str(age_key_str)?
            } else {
                // Handle legacy case where age_key might be NULL
                AgeIdentity::generate()
            };
            Ok(Self {
                secret_key,
                age_key,
            })
        } else {
            anyhow::bail!(
                "No identity found in database. Identity should be created during startup."
            )
        }
    }

    pub async fn get_or_create() -> anyhow::Result<Self> {
        let db = get_db();

        // Try to get existing identity
        let row = sqlx::query!("SELECT secret_key, age_key FROM identities LIMIT 1")
            .fetch_optional(db)
            .await?;

        if let Some(row) = row {
            let secret_key = secret_key_from_hex(&row.secret_key)?;
            let age_key = if let Some(age_key_str) = &row.age_key {
                age_identity_from_str(age_key_str)?
            } else {
                // Handle legacy case where age_key might be NULL
                AgeIdentity::generate()
            };
            Ok(Self {
                secret_key,
                age_key,
            })
        } else {
            // Create new identity
            let new_identity = Self::new();

            let secret_key_hex = secret_key_to_hex(&new_identity.secret_key);
            let age_key_str = age_identity_to_string(&new_identity.age_key);
            sqlx::query!(
                "INSERT INTO identities (secret_key, age_key) VALUES (?, ?)",
                secret_key_hex,
                age_key_str
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
    pub age_public_key: Option<String>, // Age public key as string
}

impl Peer {
    pub async fn list() -> anyhow::Result<Vec<Self>> {
        let db = get_db();
        let peers = sqlx::query_as!(
            Peer,
            "SELECT node_id, last_seen, hostname, age_public_key FROM peers ORDER BY last_seen DESC"
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
            "INSERT INTO peers (node_id, last_seen, hostname, age_public_key) VALUES (?, ?, ?, ?)",
            node_id_str,
            None::<NaiveDateTime>,
            None::<String>,
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
        age_public_key: Option<String>,
    ) -> anyhow::Result<()> {
        let db = get_db();
        let node_id_str = node_id_to_string(&node_id);
        let last_seen_naive = last_seen.map(|dt| dt.naive_utc());
        sqlx::query!(
            "INSERT INTO peers (node_id, last_seen, hostname, age_public_key) VALUES (?, ?, ?, ?)
             ON CONFLICT(node_id) DO UPDATE SET
             last_seen = COALESCE(excluded.last_seen, peers.last_seen),
             hostname = COALESCE(excluded.hostname, peers.hostname),
             age_public_key = COALESCE(excluded.age_public_key, peers.age_public_key)",
            node_id_str,
            last_seen_naive,
            hostname,
            age_public_key
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
                "INSERT OR IGNORE INTO peers (node_id, last_seen, hostname, age_public_key) VALUES (?, ?, ?, ?)",
                node_id_str,
                None::<NaiveDateTime>,
                None::<String>,
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

    pub async fn find_by_node_id(node_id: &str) -> anyhow::Result<Option<Self>> {
        let db = get_db();
        let peer = sqlx::query_as!(
            Peer,
            "SELECT node_id, last_seen, hostname, age_public_key FROM peers WHERE node_id = ?",
            node_id
        )
        .fetch_optional(db)
        .await?;
        Ok(peer)
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

fn validate_secret_name(name: &str) -> Result<()> {
    if !hostname_validator::is_valid(name) {
        anyhow::bail!("Secret name must be a valid DNS hostname (RFC 1123)");
    }
    Ok(())
}

fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

async fn encrypt_secret_for_node(secret_content: &[u8], target_node_id: NodeId) -> Result<Vec<u8>> {
    // Check if target is the current node
    let current_identity = Identity::get().await?;
    let age_public_key_str = if target_node_id == current_identity.id() {
        // Use current node's Age public key
        age_public_key_to_string(&current_identity.age_key)
    } else {
        // Look up peer in database
        let peers = Peer::list().await?;
        let target_peer = peers
            .into_iter()
            .find(|p| {
                if let Ok(peer_node_id) = node_id_from_string(&p.node_id) {
                    peer_node_id == target_node_id
                } else {
                    false
                }
            })
            .with_context(|| format!("Target node {target_node_id} not found in peers"))?;

        target_peer
            .age_public_key
            .with_context(|| format!("Target node {target_node_id} has no Age public key"))?
    };

    let recipient: age::x25519::Recipient = age_public_key_str
        .parse()
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("Invalid Age public key for {target_node_id}"))?;

    let recipients: Vec<&dyn age::Recipient> = vec![&recipient];
    let encryptor =
        Encryptor::with_recipients(recipients.into_iter()).context("Failed to create encryptor")?;
    let mut encrypted_data = Vec::new();
    let mut writer = encryptor.wrap_output(&mut encrypted_data)?;

    writer
        .write_all(secret_content)
        .context("Failed to write secret data")?;
    writer.finish().context("Failed to finish encryption")?;

    Ok(encrypted_data)
}

pub async fn decrypt_secret_for_identity(
    encrypted_data: &[u8],
    identity: &Identity,
) -> Result<Vec<u8>> {
    let decryptor = Decryptor::new(encrypted_data).context("Failed to create decryptor")?;

    let mut decrypted_data = Vec::new();
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity.age_key as &dyn age::Identity))
        .context("Failed to decrypt")?;

    reader
        .read_to_end(&mut decrypted_data)
        .context("Failed to read decrypted data")?;

    Ok(decrypted_data)
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Secret {
    pub name: String,
    pub encrypted_data: Vec<u8>,
    pub hash: String,
    pub target_node_id: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct GroupedSecret {
    pub name: String,
    pub hash: String,
    pub target_node_ids: String, // Comma-separated list from GROUP_CONCAT
    #[allow(dead_code)] // Keep for consistency with SQL schema
    pub encrypted_data: Vec<u8>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl GroupedSecret {
    pub fn get_target_node_ids(&self) -> Vec<String> {
        self.target_node_ids
            .split(',')
            .map(|s| s.to_string())
            .collect()
    }

    pub fn has_target_node(&self, target_node_id: &NodeId) -> bool {
        let target_str = node_id_to_string(target_node_id);
        self.get_target_node_ids().contains(&target_str)
    }

    pub fn has_target_node_str(&self, target_node_id_str: &str) -> bool {
        self.get_target_node_ids()
            .contains(&target_node_id_str.to_string())
    }

    pub fn get_created_at_utc(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.created_at, Utc)
    }

    pub fn get_updated_at_utc(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.updated_at, Utc)
    }
}

impl Secret {
    pub async fn create(
        name: String,
        secret_content: &[u8],
        target_node_id: NodeId,
    ) -> Result<Self> {
        validate_secret_name(&name)?;

        let hash = compute_hash(secret_content);
        let encrypted_data = encrypt_secret_for_node(secret_content, target_node_id).await?;
        let target_node_id_str = node_id_to_string(&target_node_id);
        let now = Utc::now().naive_utc();

        let db = get_db();
        sqlx::query!(
            "INSERT INTO secrets (name, encrypted_data, hash, target_node_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            name,
            encrypted_data,
            hash,
            target_node_id_str,
            now,
            now
        )
        .execute(db)
        .await?;

        // Log secret creation event
        Event::log(
            EventType::PeerMessage {
                message_type: "SECRET_CREATED".to_string(),
            },
            format!("Created new secret '{name}' for node {target_node_id}"),
            serde_json::json!({
                "secret_name": name,
                "target_node_id": target_node_id_str,
                "hash": hash
            })
            .into(),
        )
        .await?;

        Ok(Self {
            name,
            encrypted_data,
            hash,
            target_node_id: target_node_id_str,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn upsert(
        name: String,
        encrypted_data: Vec<u8>,
        hash: String,
        target_node_id: NodeId,
    ) -> Result<bool> {
        validate_secret_name(&name)?;

        let target_node_id_str = node_id_to_string(&target_node_id);
        let now = Utc::now().naive_utc();

        let db = get_db();

        // Check if we already have this secret with the same hash
        let existing = sqlx::query!(
            "SELECT hash FROM secrets WHERE name = ? AND target_node_id = ?",
            name,
            target_node_id_str
        )
        .fetch_optional(db)
        .await?;

        let was_new = existing.is_none();

        if let Some(existing_secret) = existing {
            if existing_secret.hash == hash {
                // Same hash, no update needed
                return Ok(false);
            }
        }

        // Insert or update the secret
        sqlx::query!(
            "INSERT INTO secrets (name, encrypted_data, hash, target_node_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(name, target_node_id) DO UPDATE SET
             encrypted_data = excluded.encrypted_data,
             hash = excluded.hash,
             updated_at = excluded.updated_at",
            name,
            encrypted_data,
            hash,
            target_node_id_str,
            now,
            now
        )
        .execute(db)
        .await?;

        // Log secret upsert event
        Event::log(
            EventType::PeerMessage {
                message_type: "SECRET_UPSERTED".to_string(),
            },
            format!("Updated secret '{name}' for node {target_node_id}"),
            serde_json::json!({
                "secret_name": name,
                "target_node_id": target_node_id_str,
                "hash": hash,
                "was_new": was_new
            })
            .into(),
        )
        .await?;

        Ok(true)
    }

    pub async fn list_all() -> Result<Vec<Self>> {
        let db = get_db();

        let secrets = sqlx::query_as!(
            Secret,
            "SELECT name, encrypted_data, hash, target_node_id, created_at, updated_at
             FROM secrets ORDER BY updated_at DESC"
        )
        .fetch_all(db)
        .await?;

        Ok(secrets)
    }

    pub async fn list_all_grouped() -> Result<Vec<GroupedSecret>> {
        let db = get_db();

        let rows = sqlx::query!(
            "SELECT
                name,
                hash,
                GROUP_CONCAT(target_node_id) as \"target_node_ids!\",
                encrypted_data,
                MIN(created_at) as created_at,
                MAX(updated_at) as updated_at
             FROM secrets
             GROUP BY name, hash
             ORDER BY MAX(updated_at) DESC"
        )
        .fetch_all(db)
        .await?;

        let mut grouped_secrets = Vec::new();
        for row in rows {
            grouped_secrets.push(GroupedSecret {
                name: row.name.unwrap_or_default(),
                hash: row.hash.unwrap_or_default(),
                target_node_ids: row.target_node_ids,
                encrypted_data: row.encrypted_data.unwrap_or_default(),
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }

        Ok(grouped_secrets)
    }

    pub async fn find_by_name_and_hash(name: &str, hash: &str) -> Result<Vec<Self>> {
        let db = get_db();

        let secrets = sqlx::query_as!(
            Secret,
            "SELECT name, encrypted_data, hash, target_node_id, created_at, updated_at
             FROM secrets
             WHERE name = ? AND hash = ?
             ORDER BY target_node_id",
            name,
            hash
        )
        .fetch_all(db)
        .await?;

        Ok(secrets)
    }

    pub fn get_target_node_id(&self) -> Result<NodeId> {
        node_id_from_string(&self.target_node_id)
    }

    pub fn get_created_at_utc(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.created_at, Utc)
    }

    pub fn get_updated_at_utc(&self) -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(self.updated_at, Utc)
    }

    pub async fn delete(name: &str, hash: &str, target_node_id: NodeId) -> Result<bool> {
        let db = get_db();
        let target_node_id_str = node_id_to_string(&target_node_id);

        let rows_affected = sqlx::query!(
            "DELETE FROM secrets WHERE name = ? AND hash = ? AND target_node_id = ?",
            name,
            hash,
            target_node_id_str
        )
        .execute(db)
        .await?
        .rows_affected();

        Ok(rows_affected > 0)
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
        .map_err(|_| anyhow::anyhow!("Database already initialized"))
        .context("Failed to initialize database")?;
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
        .map_err(|_| anyhow::anyhow!("Database already initialized"))
        .context("Failed to initialize database")?;
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
