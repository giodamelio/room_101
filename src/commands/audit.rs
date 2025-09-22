use anyhow::{Context, Result};
use chrono_humanize::HumanTime;

use crate::args::{AuditArgs, AuditCommands};
use crate::db::AuditEvent;

#[allow(clippy::print_stdout)] // CLI output is appropriate here
pub async fn run(audit_args: &AuditArgs) -> Result<()> {
    match &audit_args.command {
        AuditCommands::List => {
            let events = AuditEvent::list()
                .await
                .context("Failed to retrieve audit events from database")?;

            if events.is_empty() {
                println!("No audit events found in database");
                return Ok(());
            }

            println!("Found {} audit event(s):", events.len());
            for event in events {
                let human_time = HumanTime::from(event.timestamp);

                println!("  Event Type: {}", event.event_type);
                println!("    Message: {}", event.message);
                println!("    Timestamp: {} ({})", human_time, event.timestamp);

                // Always show data field, use empty object if null or empty
                let data_to_show = match &event.data {
                    serde_json::Value::Null => serde_json::json!({}),
                    serde_json::Value::Object(map) if map.is_empty() => serde_json::json!({}),
                    _ => event.data,
                };

                let formatted_data = serde_json::to_string_pretty(&data_to_show)?;
                // Indent each line of the JSON
                let indented_data = formatted_data
                    .lines()
                    .map(|line| format!("      {}", line))
                    .collect::<Vec<_>>()
                    .join("\n");

                println!("    Data:\n{}", indented_data);
                println!();
            }
            Ok(())
        }
    }
}
