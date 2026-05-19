//! Retention sweeper.
//!
//! Once per `interval`, walk every account and delete attestations whose
//! `received_at` is older than the account's `retention_days`. Team accounts
//! default to 365 days; Enterprise to 2555. The implementation lives behind
//! a plain function so the standalone `sweeper` binary can drive a single
//! pass for the Verification block of Step 4
//! (`docker compose exec awp-cloud /usr/local/bin/sweeper run-once`).
//!
//! The blob store is intentionally *not* touched. Blobs are content-
//! addressed and may be referenced by attestations in other accounts;
//! orphan GC is a separate concern that lands later. The metadata row is
//! the gate that grants account-scoped access, so dropping it is enough to
//! enforce retention.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use crate::error::ApiError;
use crate::store::Db;

/// Single pass over every account. Returns `(accounts_visited, rows_deleted)`.
pub async fn sweep_once(db: Arc<dyn Db>) -> Result<(usize, u64), ApiError> {
    let accounts = db.list_accounts().await?;
    let now = Utc::now();
    let mut total_deleted: u64 = 0;
    for acct in &accounts {
        let cutoff = now - chrono::Duration::days(acct.retention_days as i64);
        let deleted = db.delete_attestations_older_than(acct.id, cutoff).await?;
        total_deleted += deleted;
        db.mark_swept(acct.id, now).await?;
        if deleted > 0 {
            tracing::info!(
                account_id = %acct.id,
                retention_days = acct.retention_days,
                deleted,
                "retention sweep removed attestations"
            );
        }
    }
    Ok((accounts.len(), total_deleted))
}

/// Long-running periodic loop. Wakes every `interval` and runs [`sweep_once`].
/// `main.rs` spawns this onto the tokio runtime as a fire-and-forget task.
pub async fn run_forever(db: Arc<dyn Db>, interval: Duration) {
    loop {
        match sweep_once(Arc::clone(&db)).await {
            Ok((visited, deleted)) => {
                tracing::info!(visited, deleted, "retention sweep complete");
            }
            Err(e) => {
                tracing::error!(error = %e, "retention sweep failed");
            }
        }
        tokio::time::sleep(interval).await;
    }
}
