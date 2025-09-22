use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    #[serde(skip_serializing)] // Don't send id when inserting, it will be auto generated
    pub id: Option<RecordId>,
    pub event_type: String,
    pub message: String,
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl AuditEvent {
    pub async fn log(event_type: String, message: String, data: serde_json::Value) -> Result<()> {
        let event = AuditEvent {
            id: None,
            event_type,
            message,
            data,
            timestamp: Utc::now(),
        };

        let created: Option<Self> = db().await?.create("audit_event").content(event).await?;
        created.ok_or_else(|| anyhow!("Failed to create audit log"))?;
        Ok(())
    }

    pub async fn list() -> Result<Vec<Self>> {
        db().await?
            .query("SELECT * FROM audit_event ORDER BY timestamp ASC")
            .await?
            .take(0)
            .context("Failed to list audit events")
    }
}
