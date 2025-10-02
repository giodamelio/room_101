use anyhow::Result;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use tokio::sync::OnceCell;

pub mod audit_event;
pub mod identity;
pub mod peer;
pub mod secret;

pub use audit_event::AuditEvent;
pub use identity::Identity;
pub use peer::{Peer, PeerExt};
use tracing::{debug, trace};

use crate::args;

static DATABASE: OnceCell<Surreal<Db>> = OnceCell::const_new();

async fn initialize_schema(db: &Surreal<Db>) -> Result<()> {
    debug!("Initializing database schema");
    let schema_sql = include_str!("../../schema.surql");
    db.query(schema_sql).await?;
    trace!("Database schema initialization completed");
    Ok(())
}

#[cfg(not(test))]
pub async fn db() -> Result<&'static Surreal<Db>> {
    DATABASE
        .get_or_try_init(|| async {
            let args = args::args().await;
            let db = Surreal::new::<SurrealKv>(args.db_path.clone()).await?;

            // TODO: handle better selecting of the NS/DB
            db.use_ns("prod").use_db("prod").await?;

            // Initialize schema immediately after database setup
            initialize_schema(&db).await?;

            Ok(db)
        })
        .await
}

// In memory DB for the tests

#[cfg(test)]
static TEST_DATABASE: OnceCell<Surreal<Db>> = OnceCell::const_new();

#[cfg(test)]
pub async fn db() -> Result<&'static Surreal<Db>> {
    use surrealdb::engine::local::Mem;

    TEST_DATABASE
        .get_or_try_init(|| async {
            let db = Surreal::new::<Mem>(()).await?;
            db.use_ns("test").use_db("test").await?;

            // Initialize schema for test database too
            initialize_schema(&db).await?;

            Ok(db)
        })
        .await
}
