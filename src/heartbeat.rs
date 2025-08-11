use std::time::Duration;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::db;

pub async fn heartbeat_subsystem(
    subsys: SubsystemHandle,
    _db: db::DB,
    interval: Duration,
) -> anyhow::Result<()> {
    info!("Heartbeat subsystem started");

    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Perform heartbeat logic here
                info!("BEAT");
            }

            _ = subsys.on_shutdown_requested() => {
                info!("Heartbeat received shutdown signal");
                break;
            }
        }
    }

    info!("Heartbeat subsystem stopped");
    Ok(())
}
