use anyhow::Result;

use crate::db::Peer;

#[allow(clippy::print_stdout)] // CLI output is appropriate here
pub async fn run() -> Result<()> {
    let peers_count = Peer::count().await?;

    println!("Status:");
    println!("  Peers count: {}", peers_count);

    Ok(())
}
