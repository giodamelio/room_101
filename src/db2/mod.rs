use anyhow::Result;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use tokio::sync::OnceCell;

pub mod identity;
pub mod peer;

pub use identity::Identity;
pub use peer::{Peer, PeerExt};

static DATABASE: OnceCell<Surreal<Db>> = OnceCell::const_new();

pub async fn db() -> Result<&'static Surreal<Db>> {
    DATABASE
        .get_or_try_init(|| async {
            // TODO: allow saving to the FS with SurrealKV
            let db = Surreal::new::<Mem>(()).await?;

            // TODO: handle better selecting of the NS/DB
            db.use_ns("prod").use_db("prod").await?;

            Ok(db)
        })
        .await
}
