use anyhow::Result;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use tokio::sync::OnceCell;

pub mod audit_event;
pub mod identity;
pub mod peer;

pub use audit_event::AuditEvent;
pub use identity::Identity;
pub use peer::{Peer, PeerExt};

use crate::args;

static DATABASE: OnceCell<Surreal<Db>> = OnceCell::const_new();

pub async fn db() -> Result<&'static Surreal<Db>> {
    DATABASE
        .get_or_try_init(|| async {
            let args = args::args().await;
            let db = Surreal::new::<SurrealKv>(args.db_path.clone()).await?;

            // TODO: handle better selecting of the NS/DB
            db.use_ns("prod").use_db("prod").await?;

            Ok(db)
        })
        .await
}
