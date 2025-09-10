use clap::Parser;
use tokio::sync::OnceCell;

#[derive(Parser, Debug)]
#[command(name = "room_101")]
#[command(about = "A peer-to-peer networking application")]
pub struct Args {
    /// Tickets of bootstrap nodes to connect to connect to (hex strings)
    pub bootstrap: Vec<String>,

    /// Start the web server
    #[arg(long)]
    pub start_web: bool,

    /// Web server port (default: 3000)
    #[arg(long, default_value = "3000")]
    pub port: u16,

    /// Path to database location
    #[arg(long)]
    pub db_path: String,

    /// Directory to store systemd credentials (default: /var/lib/credstore)
    #[arg(long, default_value = "/var/lib/credstore")]
    pub systemd_secrets_path: String,

    /// Use user-scope systemd credentials instead of system-scope (default: system-scope)
    #[arg(long)]
    pub systemd_user_scope: bool,
}

static ARGS: OnceCell<Args> = OnceCell::const_new();

pub async fn args() -> &'static Args {
    ARGS.get_or_init(|| async { Args::parse() }).await
}
