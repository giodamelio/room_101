pub mod gossip;
pub mod supervisor;
// pub mod systemd;

// Re-export the main types from supervisor for easier access
pub use supervisor::{AppConfig, SupervisorActor, SystemdSecretsConfig};
