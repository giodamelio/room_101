use std::collections::HashMap;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::{
    Datetime, Number, RecordId,
    sql::{self, Value},
};

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    #[serde(skip_serializing)] // Don't send id when inserting, it will be auto generated
    pub id: Option<RecordId>,
    pub event_type: String,
    pub message: String,
    pub data: HashMap<String, String>,
    pub timestamp: Datetime,
}

impl AuditEvent {
    pub async fn log(event_type: String, message: String, data: serde_json::Value) -> Result<()> {
        let event = AuditEvent {
            id: None,
            event_type,
            message,
            data: value_to_hashmap(data)?,
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

// TODO: watch this issue, then maybe we could store a Value in the DB instead of a HashMap of
// string string pairs
// https://github.com/surrealdb/surrealdb/issues/5754
fn value_to_hashmap(value: serde_json::Value) -> Result<HashMap<String, String>> {
    let serde_map = value.as_object().ok_or(anyhow!("data is not an object"))?;
    let mut output: HashMap<String, String> = HashMap::new();
    for (key, val) in serde_map.iter() {
        let mapped_val = match val {
            serde_json::Value::Null => "Null".to_string(),
            serde_json::Value::Bool(v) => v.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(v) => v.to_string(),
            serde_json::Value::Array(_values) => bail!("Nested arrays are not supported"),
            serde_json::Value::Object(_map) => bail!("Nested maps are not supported"),
        };

        output.insert(key.clone(), mapped_val);
    }
    Ok(output)
}
