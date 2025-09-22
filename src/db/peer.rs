use age::x25519::Recipient as AgeRecipient;
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use iroh::{NodeAddr, NodeId};
use iroh_base::ticket::NodeTicket;
use serde::{Deserialize, Serialize};

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub node_id: NodeId,
    pub ticket: NodeTicket,
    pub hostname: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(with = "crate::custom_serde::age_recipient_serde", default)]
    pub age_public_key: Option<AgeRecipient>,
}

impl From<NodeTicket> for Peer {
    fn from(ticket: NodeTicket) -> Self {
        Self {
            node_id: ticket.node_addr().node_id,
            ticket,
            hostname: None,
            last_seen: None,
            age_public_key: None,
        }
    }
}

pub trait PeerExt<Peer> {
    fn to_node_ids(self) -> Vec<NodeId>;
}

impl PeerExt<Peer> for Vec<Peer> {
    fn to_node_ids(self) -> Vec<NodeId> {
        self.into_iter().map(|peer| peer.node_id).collect()
    }
}

impl Peer {
    #[cfg(test)]
    pub fn from_string(node_id_str: &str, age_public_key_str: &str) -> Result<Peer> {
        let node_id = node_id_str.parse::<NodeId>()
            .context("Failed to parse NodeId from string")?;
        let age_public_key = age_public_key_str.parse::<AgeRecipient>()
            .map_err(|e| anyhow!("Failed to parse Age Recipient from string: {e}"))?;
        
        let ticket = NodeTicket::new(NodeAddr::new(node_id));
        
        Ok(Peer {
            node_id,
            ticket,
            hostname: None,
            last_seen: None,
            age_public_key: Some(age_public_key),
        })
    }

    pub async fn insert_from_ticket(ticket: NodeTicket) -> Result<Option<Peer>> {
        let peer: Peer = ticket.into();
        db().await?
            .upsert(("peer", peer.node_id.to_string()))
            .content::<Peer>(peer)
            .await
            .context("Failed to insert peer")
    }

    pub async fn insert_from_node_id(node_id: NodeId) -> Result<Option<Peer>> {
        // Create a new mostly empty ticket for the peer
        // TODO: can we fill in this data later?
        let ticket = NodeTicket::new(NodeAddr::new(node_id));
        Self::insert_from_ticket(ticket).await
    }

    pub async fn list() -> Result<Vec<Peer>> {
        db().await?
            .select("peer")
            .await
            .context("Failed to list peers")
    }

    pub async fn get(node_id: NodeId) -> Result<Peer> {
        db().await?
            .select::<Option<Peer>>(("peer", node_id.to_string()))
            .await?
            .ok_or(anyhow!("Could not find peer"))
    }

    pub async fn count() -> Result<usize> {
        #[derive(serde::Deserialize)]
        struct CountResult {
            total: usize,
        }

        let result = db()
            .await?
            .query("SELECT count() AS total FROM peer GROUP ALL")
            .await?
            .take::<Option<CountResult>>(0)?
            .ok_or(anyhow!("Could not count peers"))?;

        Ok(result.total)
    }

    pub async fn bump_last_seen(node_id: NodeId) -> Result<()> {
        #[derive(serde::Serialize)]
        struct UpdateLastSeen {
            last_seen: DateTime<Utc>,
        }

        let _peer: Option<Peer> = db()
            .await?
            .update(("peer", node_id.to_string()))
            .merge(UpdateLastSeen {
                last_seen: Utc::now(),
            })
            .await?;

        Ok(())
    }

    pub fn node_addr(&self) -> &NodeAddr {
        self.ticket.node_addr()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use iroh::NodeAddr;
    use iroh_base::ticket::NodeTicket;

    #[tokio::test]
    async fn test_insert_from_ticket_no_duplicates() {
        // Create a test ticket
        let secret_key = iroh::SecretKey::generate(rand::rngs::OsRng);
        let node_id = secret_key.public();
        let node_addr1 = NodeAddr::new(node_id);
        let ticket1 = NodeTicket::new(node_addr1);

        // Create a second ticket with same NodeId but potentially different endpoint info
        let node_addr2 =
            NodeAddr::new(node_id).with_direct_addresses(["127.0.0.1:8080".parse().unwrap()]);
        let ticket2 = NodeTicket::new(node_addr2);

        // Insert the first ticket
        let result1 = Peer::insert_from_ticket(ticket1.clone()).await.unwrap();
        assert!(result1.is_some());
        let peer1 = result1.unwrap();

        // Insert the second ticket with same NodeId but different endpoint info
        let result2 = Peer::insert_from_ticket(ticket2.clone()).await.unwrap();
        assert!(result2.is_some());
        let peer2 = result2.unwrap();

        // Verify they have the same node_id
        assert_eq!(peer1.node_id, peer2.node_id);
        assert_eq!(peer1.node_id, node_id);

        // Verify the ticket was updated to the second ticket
        assert_eq!(peer2.ticket.node_addr(), ticket2.node_addr());
        assert_ne!(peer1.ticket.node_addr(), peer2.ticket.node_addr());

        // Verify we only have one peer in the database
        let peers = Peer::list().await.unwrap();
        let matching_peers: Vec<_> = peers.into_iter().filter(|p| p.node_id == node_id).collect();
        assert_eq!(matching_peers.len(), 1);

        // Verify the final peer has the updated ticket (ticket2)
        assert_eq!(matching_peers[0].ticket.node_addr(), ticket2.node_addr());
    }
}
