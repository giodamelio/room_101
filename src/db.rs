use std::sync::OnceLock;

use anyhow::Result;
use native_db::{Builder, Database, Models};

pub mod data {
    use native_db::{Key, ToKey, native_db};
    use native_model::{Model, native_model};
    use serde::{Deserialize, Serialize};

    pub type Identity = v1::Identity;
    pub type Peer = v1::Peer;
    pub type EventType = v1::EventType;
    pub type Event = v1::Event;

    pub mod v1 {
        use super::*;

        use chrono::Utc;
        use iroh::{NodeId as OriginalNodeId, PublicKey, SecretKey as OriginalSecretKey};
        use rand::rngs;
        use tracing::debug;

        pub type DateTime = chrono::DateTime<Utc>;

        #[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash)]
        pub struct Uuid(uuid::Uuid);

        impl ToKey for Uuid {
            fn to_key(&self) -> Key {
                Key::new(self.0.as_bytes().to_vec())
            }

            fn key_names() -> Vec<String> {
                vec!["Uuid".to_string()]
            }
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct SecretKey(OriginalSecretKey);

        impl ToKey for SecretKey {
            fn to_key(&self) -> native_db::Key {
                Key::new(self.0.public().as_bytes().to_vec())
            }

            fn key_names() -> Vec<String> {
                vec!["SecretKey".to_string()]
            }
        }

        impl SecretKey {
            fn generate() -> Self {
                let og_secret_key = OriginalSecretKey::generate(rngs::OsRng);
                Self(og_secret_key)
            }

            pub fn inner(&self) -> &OriginalSecretKey {
                &self.0
            }

            pub fn public(&self) -> PublicKey {
                self.0.public()
            }
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct NodeId(OriginalNodeId);

        impl ToKey for NodeId {
            fn to_key(&self) -> native_db::Key {
                Key::new(self.0.as_bytes().to_vec())
            }

            fn key_names() -> Vec<String> {
                vec!["NodeId".to_string()]
            }
        }

        impl From<OriginalNodeId> for NodeId {
            fn from(value: OriginalNodeId) -> Self {
                Self(value)
            }
        }

        impl NodeId {
            pub fn inner(&self) -> OriginalNodeId {
                self.0
            }
        }

        impl std::fmt::Display for NodeId {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for NodeId {
            type Err = anyhow::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(OriginalNodeId::from_str(s)?))
            }
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        #[native_model(id = 1, version = 1)]
        #[native_db]
        pub struct Identity {
            #[primary_key]
            pub secret_key: SecretKey,
        }

        impl Identity {
            pub fn new() -> Self {
                debug!("Generating new identity with random secret key");
                let identity = Self {
                    secret_key: SecretKey::generate(),
                };
                debug!(public_key = %identity.secret_key.public(), "Generated new identity");
                identity
            }

            pub fn id(&self) -> OriginalNodeId {
                self.secret_key.public().into()
            }

            pub async fn get_or_create() -> anyhow::Result<Self> {
                let db = crate::db::get_db();
                let r = db.r_transaction()?;
                let mut identities: Vec<Identity> = Vec::new();
                for item in r.scan().primary()?.all()? {
                    identities.push(item?);
                }

                if let Some(identity) = identities.first() {
                    Ok(identity.clone())
                } else {
                    let new_identity = Identity::new();
                    let rw = db.rw_transaction()?;
                    rw.insert(new_identity.clone())?;
                    rw.commit()?;
                    Ok(new_identity)
                }
            }
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[native_model(id = 2, version = 1)]
        #[native_db]
        pub struct Peer {
            #[serde(
                serialize_with = "serialize_node_id",
                deserialize_with = "deserialize_node_id"
            )]
            #[primary_key]
            pub node_id: NodeId,
            pub last_seen: Option<DateTime>,
            pub hostname: Option<String>,
        }

        fn serialize_node_id<S>(node_id: &NodeId, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            serializer.serialize_str(&node_id.0.to_string())
        }

        fn deserialize_node_id<'de, D>(deserializer: D) -> Result<NodeId, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            s.parse().map_err(serde::de::Error::custom)
        }

        impl Peer {
            pub async fn list() -> anyhow::Result<Vec<Self>> {
                let db = crate::db::get_db();
                let r = db.r_transaction()?;
                let mut peers = Vec::new();
                for item in r.scan().primary()?.all()? {
                    peers.push(item?);
                }
                Ok(peers)
            }

            pub async fn create(node_id: OriginalNodeId) -> anyhow::Result<()> {
                let peer = Self {
                    node_id: node_id.into(),
                    last_seen: None,
                    hostname: None,
                };
                let db = crate::db::get_db();
                let rw = db.rw_transaction()?;
                rw.insert(peer)?;
                rw.commit()?;
                Ok(())
            }

            pub async fn upsert_peer(
                node_id: OriginalNodeId,
                last_seen: Option<DateTime>,
                hostname: Option<String>,
            ) -> anyhow::Result<()> {
                let peer = Self {
                    node_id: node_id.into(),
                    last_seen,
                    hostname,
                };
                let db = crate::db::get_db();
                let rw = db.rw_transaction()?;
                rw.upsert(peer)?;
                rw.commit()?;
                Ok(())
            }

            pub async fn insert_bootstrap_nodes(nodes: Vec<OriginalNodeId>) -> anyhow::Result<()> {
                let db = crate::db::get_db();
                let rw = db.rw_transaction()?;
                for node_id in nodes {
                    let peer = Self {
                        node_id: node_id.into(),
                        last_seen: None,
                        hostname: None,
                    };
                    rw.insert(peer)?;
                }
                rw.commit()?;
                Ok(())
            }

            pub async fn list_node_ids() -> anyhow::Result<Vec<OriginalNodeId>> {
                let db = crate::db::get_db();
                let r = db.r_transaction()?;
                let mut peers = Vec::new();
                for item in r.scan().primary()?.all()? {
                    let peer: Self = item?;
                    peers.push(peer.node_id.inner());
                }
                Ok(peers)
            }
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub enum EventType {
            PeerMessage { message_type: String },
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[native_model(id = 3, version = 1)]
        #[native_db]
        pub struct Event {
            #[primary_key]
            pub id: Uuid,
            pub event_type: EventType,
            pub message: String,
            pub time: DateTime,
            pub data: serde_json::Value,
        }

        impl Event {
            pub async fn list() -> anyhow::Result<Vec<Self>> {
                let db = crate::db::get_db();
                let r = db.r_transaction()?;
                let mut events = Vec::new();
                for item in r.scan().primary()?.all()? {
                    events.push(item?);
                }
                Ok(events)
            }

            pub async fn log(
                event_type: EventType,
                message: String,
                data: Option<serde_json::Value>,
            ) -> anyhow::Result<()> {
                let event = Self {
                    id: Uuid(uuid::Uuid::new_v4()),
                    event_type,
                    message,
                    time: chrono::Utc::now(),
                    data: data.unwrap_or(serde_json::Value::Null),
                };

                let db = crate::db::get_db();
                let rw = db.rw_transaction()?;
                rw.insert(event)?;
                rw.commit()?;

                Ok(())
            }
        }
    }
}

static MODELS: OnceLock<Models> = OnceLock::new();
static DATABASE: OnceLock<Database<'static>> = OnceLock::new();

fn get_models() -> &'static Models {
    MODELS.get_or_init(|| {
        let mut models = Models::new();
        models.define::<data::v1::Identity>().unwrap();
        models.define::<data::v1::Peer>().unwrap();
        models.define::<data::v1::Event>().unwrap();
        models
    })
}

pub fn init_db() -> Result<()> {
    let db = Builder::new().create_in_memory(get_models())?;
    DATABASE
        .set(db)
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;
    Ok(())
}

pub fn get_db() -> &'static Database<'static> {
    DATABASE
        .get()
        .expect("Database not initialized. Call init_db() first.")
}
