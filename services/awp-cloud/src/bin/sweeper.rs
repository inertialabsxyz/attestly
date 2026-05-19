//! Standalone retention sweeper. Usually runs as a background task inside
//! the `awp-cloud` server, but Step 4 calls it out as a separate binary so
//! the Verification block can do
//!
//! ```bash
//! docker compose exec awp-cloud /usr/local/bin/sweeper run-once
//! ```
//!
//! `run-once` runs a single pass and exits. With no argument, the binary
//! loops at the same cadence the in-process sweeper uses (24h).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use awp_cloud::store::postgres::PgDb;
use awp_cloud::store::Db;
use awp_cloud::sweeper;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,awp_cloud=debug".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let _blob_root: PathBuf = std::env::var("BLOB_ROOT")
        .unwrap_or_else(|_| "./data/blobs".to_string())
        .into();

    let db = PgDb::connect(&database_url, 4).await?;
    db.apply_migrations().await?;
    let db: Arc<dyn Db> = Arc::new(db);

    let once = std::env::args().nth(1).as_deref() == Some("run-once");
    if once {
        let (visited, deleted) = sweeper::sweep_once(db).await?;
        eprintln!("# sweep: visited {visited} accounts, deleted {deleted} rows");
        return Ok(());
    }

    sweeper::run_forever(db, Duration::from_secs(24 * 60 * 60)).await;
    Ok(())
}
