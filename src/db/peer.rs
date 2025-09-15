use age::x25519::Recipient as AgeRecipient;
use anyhow::{Context, Result, anyhow};
use iroh::{NodeAddr, NodeId};
use iroh_base::ticket::NodeTicket;
use serde::{Deserialize, Serialize};
use surrealdb::Datetime;

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub node_id: NodeId,
    pub ticket: NodeTicket,
    pub hostname: Option<String>,
    pub last_seen: Option<Datetime>,
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
    pub async fn insert_from_ticket(ticket: NodeTicket) -> Result<Option<Peer>> {
        let peer: Peer = ticket.into();
        db().await?
            .upsert(("peer", peer.node_id.to_string()))
            .content::<Peer>(peer)
            .await
            .context("Failed to insert peer")
    }

    pub async fn list() -> Result<Vec<Peer>> {
        db().await?
            .select("peer")
            .await
            .context("Failed to list peers")
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

    pub fn node_addr(&self) -> &NodeAddr {
        self.ticket.node_addr()
    }
}
