use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    #[serde(skip_serializing)]
    pub id: Option<RecordId>,
    pub event_type: String,
    pub message: String,
    pub data: serde_json::Value,
    #[serde(with = "crate::custom_serde::chrono_datetime_as_sql")]
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

        let _: Option<AuditEvent> = db().await?
            .create("audit_event")
            .content(event)
            .await
            .context("Failed to create audit log")?;
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
