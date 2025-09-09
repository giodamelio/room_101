use anyhow::{Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::{Datetime, Object};

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    event_type: String,
    message: String,
    data: Object,
    timestamp: Datetime,
}

impl AuditEvent {
    pub async fn log(event_type: String, message: String, data: Option<Object>) -> Result<()> {
        db().await?
            .create("audit_event")
            .content(AuditEvent {
                event_type,
                message,
                data: data.unwrap_or_default(),
                timestamp: Utc::now().into(),
            })
            .await?
            .ok_or(anyhow!("Failed to create audit log"))
    }
}
