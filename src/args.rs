use clap::{Parser, Subcommand};
use tokio::sync::OnceCell;

#[derive(Parser, Debug)]
#[command(name = "room_101")]
#[command(about = "A peer-to-peer networking application")]
pub struct Args {
    /// Path to database location
    #[arg(long)]
    pub db_path: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the P2P networking server
    Server(ServerArgs),
    /// Manage peers
    Peers(PeersArgs),
}

#[derive(Parser, Debug)]
pub struct ServerArgs {
    /// Tickets of bootstrap nodes to connect to (hex strings)
    pub bootstrap: Vec<String>,

    /// Directory to store systemd credentials (default: /var/lib/credstore)
    #[arg(long, default_value = "/var/lib/credstore")]
    pub systemd_secrets_path: String,

    /// Use user-scope systemd credentials instead of system-scope (default: system-scope)
    #[arg(long)]
    pub systemd_user_scope: bool,
}

#[derive(Parser, Debug)]
pub struct PeersArgs {
    #[command(subcommand)]
    pub command: PeerCommands,
}

#[derive(Subcommand, Debug)]
pub enum PeerCommands {
    /// List all peers in the database
    List,
}

static ARGS: OnceCell<Args> = OnceCell::const_new();

pub async fn args() -> &'static Args {
    ARGS.get_or_init(|| async { Args::parse() }).await
}
