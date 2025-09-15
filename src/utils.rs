use anyhow::{Context, Result, bail};
use iroh_base::ticket::NodeTicket;
use std::path::Path;
use std::str::FromStr;
use tokio::fs;
use tracing::{debug, trace};

/// Creates a TopicId from a string literal, padding with zeros to 32 bytes at compile time
/// Fails at compile time if the string is longer than 32 bytes
macro_rules! topic_id {
    ($s:literal) => {{
        const BYTES: &[u8] = $s.as_bytes();
        const LEN: usize = BYTES.len();
        const _: () = assert!(LEN <= 32, "Topic string is too long (max 32 bytes)");
        const PADDED: [u8; 32] = {
            let mut arr = [0u8; 32];
            let mut i = 0;
            while i < LEN {
                arr[i] = BYTES[i];
                i += 1;
            }
            arr
        };
        TopicId::from_bytes(PADDED)
    }};
}

pub(crate) use topic_id;

/// Check if a file contains a valid ticket format
async fn is_ticket_file(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)
        .await
        .context("Failed to read file content")?;

    // Try to parse the content as a NodeTicket
    let is_valid = NodeTicket::from_str(content.trim()).is_ok();
    Ok(is_valid)
}

/// Safely write a ticket to a file, only overwriting if the existing file contains a valid ticket
pub async fn write_ticket_to_file(ticket: &NodeTicket, path: &Path) -> Result<()> {
    let ticket_string = ticket.to_string();

    // Check if file exists and if so, whether it's a ticket file
    // TODO: I know this is a potential race condition, but I can't be bothered to fix it since it
    // is not just checking if it exists, but also what it contains. Do it "right" and atomically
    // would need a lock and whatnot
    if path.exists() {
        let is_existing_ticket = is_ticket_file(path).await.unwrap_or(false);
        if !is_existing_ticket {
            bail!(
                "File exists at '{path:?}' but does not contain a valid ticket. Refusing to overwrite."
            );
        }
        trace!("Overwriting existing ticket file at '{path:?}'");
    } else {
        trace!("Writing new ticket file at '{path:?}'");

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create parent directory for '{path:?}'"))?;
        }
    }

    fs::write(path, ticket_string)
        .await
        .with_context(|| format!("Failed to write ticket to file '{path:?}'"))?;

    trace!("Successfully wrote ticket to '{path:?}'");

    Ok(())
}
