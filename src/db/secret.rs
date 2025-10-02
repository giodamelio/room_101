// use anyhow::{Result, anyhow};
// use chrono::{DateTime, Utc};
// use iroh::NodeId;
// use serde::{Deserialize, Serialize};
//
// use crate::db::Peer;

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct EncryptedData(pub Vec<u8>);

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Secret {
//     pub name: String,
//     #[serde(with = "crate::custom_serde::node_id_serde")]
//     pub node_id: NodeId,
//     #[serde(with = "crate::custom_serde::chrono_datetime_as_sql")]
//     pub created_at: DateTime<Utc>,
//     pub data: EncryptedData,
// }
//
// impl Secret {
//     async fn for_peer(node_id: NodeId, name: String, data: Vec<u8>) -> Result<Self> {
//         let recipient = Peer::get(node_id)
//             .await?
//             .age_public_key
//             .ok_or_else(|| anyhow!("Peer {node_id} has no age public key"))?;
//
//         let encrypted_data: EncryptedData = EncryptedData(age::encrypt(&recipient, &data)?);
//
//         Ok(Self {
//             name,
//             node_id,
//             created_at: Utc::now(),
//             data: encrypted_data,
//         })
//     }
// }
//
// #[cfg(test)]
// #[allow(clippy::unwrap_used, clippy::expect_used)]
// mod tests {
//     use super::*;
//
//     #[tokio::test]
//     async fn test_encrypt_secret_for_a_peer() -> Result<()> {
//         use age::x25519::Identity as AgeIdentity;
//         use iroh::SecretKey;
//
//         // Create test keys
//         let secret_key = SecretKey::from_bytes(&[1u8; 32]);
//         let node_id = secret_key.public();
//         let age_identity = AgeIdentity::generate();
//         let age_public_key = age_identity.to_public();
//
//         // Create peer and save to database
//         let peer = Peer::from_string(&node_id.to_string(), &age_public_key.to_string())?;
//         let _: Option<Peer> = super::db()
//             .await?
//             .upsert(("peer", node_id.to_string()))
//             .content::<Peer>(peer)
//             .await
//             .context("Failed to save peer")?;
//
//         let test_data = b"test data".to_vec();
//         let secret_name = "test_secret".to_string();
//
//         // Use for_peer function to create encrypted secret
//         let secret = Secret::for_peer(node_id, secret_name, test_data).await?;
//
//         // Verify the Age format header is present
//         let encrypted_str = String::from_utf8_lossy(&secret.data.0);
//         assert!(
//             encrypted_str.starts_with("age-encryption.org/v1"),
//             "Encrypted data should start with Age format header"
//         );
//
//         Ok(())
//     }
// }
