use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

use crate::db;

#[derive(Debug)]
pub enum HeartbeatMessage {
    Shutdown,
}

pub async fn heartbeat_task(_db: db::DB, interval: Duration, mut rx: mpsc::Receiver<HeartbeatMessage>) {
    info!("Heartbeat task started");
    
    let mut ticker = tokio::time::interval(interval);
    
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Perform heartbeat logic here
                info!("BEAT");
            }
            
            message = rx.recv() => {
                match message {
                    Some(HeartbeatMessage::Shutdown) => {
                        info!("Heartbeat received shutdown signal");
                        break;
                    }
                    None => {
                        info!("Heartbeat channel closed, shutting down");
                        break;
                    }
                }
            }
        }
    }
    
    info!("Heartbeat task stopped");
}