use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::{Datetime, RecordId};

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    #[serde(skip_serializing)] // Don't send id when inserting, it will be auto generated
    pub id: Option<RecordId>,
    pub event_type: String,
    pub message: String,
    pub data: serde_json::Value,
    pub timestamp: Datetime,
}

impl AuditEvent {
    pub async fn log(event_type: String, message: String, data: serde_json::Value) -> Result<()> {
        let event = AuditEvent {
            id: None,
            event_type,
            message,
            data,
            timestamp: Utc::now().into(),
        };

        let created: Option<Self> = db().await?.create("audit_event").content(event).await?;
        created.ok_or_else(|| anyhow!("Failed to create audit log"))?;
        Ok(())
    }

    pub async fn list() -> Result<Vec<Self>> {
        db().await?
            .select("audit_event")
            .await
            .context("Failed to list audit events")
    }
}
