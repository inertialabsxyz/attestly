//! `awp-cloud-seed` — synthetic data generator for hand-testing.
//!
//! Subcommands (passed as the first positional argument):
//!
//! - *(default)* — 10k attestations under the `seed@local.test` Team
//!   account, printing a fresh API key. Re-runnable: the account is reused
//!   if it already exists (a re-run appends another 10k rows). The default
//!   path is what `make seed` calls.
//! - `overage` — synthetic Team account with 1.05M attestation *usage*
//!   counters in the current calendar month. Drives the Step 4 verification
//!   command `make seed-overage`. We only bump the `usage` table — writing
//!   1.05M real attestations would take minutes and is unnecessary; the
//!   billing trigger reads usage counters, not raw rows.
//! - `old-attestations` — 100 attestations dated 400 days ago under a
//!   fresh `retention@local.test` Team account. Drives
//!   `make seed-old-attestations`, paired with
//!   `docker compose exec awp-cloud /usr/local/bin/sweeper run-once` in
//!   the spec.
//!
//! Requires `DATABASE_URL` and (for the default mode) `BLOB_ROOT` to be set
//! the same way `awp-cloud` itself does.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use awp_core::{signing::AgentKeypair, Attestation, AttestationStatus};
use chrono::{Duration, Utc};

use awp_cloud::auth::{generate_api_key, hash_api_key};
use awp_cloud::blob::filesystem::FsBlobStore;
use awp_cloud::blob::BlobStore;
use awp_cloud::canonical::{blob_sha256, canonical_blob_bytes};
use awp_cloud::store::postgres::PgDb;
use awp_cloud::store::{Account, AttestationIndex, Db, Plan};

/// Look up the seed account by email, or create it if this is the first run.
/// Keeps every `make seed*` target re-runnable: `create_account` alone would
/// fail the second time on the `accounts_email_key` unique constraint.
async fn account_or_create(db: &PgDb, email: &str, plan: Plan) -> anyhow::Result<Account> {
    if let Some(existing) = db.find_account_by_email(email).await? {
        eprintln!("# reusing existing account for {email}");
        return Ok(existing);
    }
    Ok(db.create_account(email, plan).await?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let blob_root: PathBuf = std::env::var("BLOB_ROOT")
        .unwrap_or_else(|_| "./data/blobs".to_string())
        .into();
    let db = PgDb::connect(&database_url, 8).await?;
    db.apply_migrations().await?;

    let mode = std::env::args().nth(1).unwrap_or_else(|| "default".into());
    match mode.as_str() {
        "default" => seed_default(db, blob_root).await,
        "overage" => seed_overage(db).await,
        "old-attestations" => seed_old_attestations(db, blob_root).await,
        other => {
            anyhow::bail!(
                "unknown seed subcommand `{other}`. expected one of: \
                 default, overage, old-attestations"
            );
        }
    }
}

async fn seed_default(db: PgDb, blob_root: PathBuf) -> anyhow::Result<()> {
    const SEED_COUNT: usize = 10_000;
    let blob = FsBlobStore::new(&blob_root);

    let account = account_or_create(&db, "seed@local.test", Plan::Team).await?;
    let cleartext = generate_api_key();
    let phc = hash_api_key(&cleartext)?;
    db.create_api_key(account.id, "seed-project", &phc).await?;

    println!("# AWP cloud seed");
    println!("# account_id : {}", account.id);
    println!("export AWP_API_KEY={cleartext}");
    println!("# Inserting {SEED_COUNT} attestations...");

    let kp = AgentKeypair::generate();
    let agent_ids = ["agent-kyc-01", "agent-kyc-02", "agent-summary-01"];

    let db: Arc<dyn Db> = Arc::new(db);
    let blob: Arc<dyn BlobStore> = Arc::new(blob);

    for i in 0..SEED_COUNT {
        let agent_id = agent_ids[i % agent_ids.len()];
        let customer_id = format!("CUST-{:05}", i % 200);
        let task_hash = awp_core::signing::sha256(format!("task-{i}").as_bytes());
        let output = serde_json::json!({
            "customer_id": customer_id,
            "decision": if i % 7 == 0 { "Deny" } else { "Approve" },
            "seed_index": i,
        })
        .to_string();
        let mut att = Attestation::new(
            agent_id,
            task_hash,
            output,
            AttestationStatus::Completed,
            None,
            Utc::now().timestamp(),
        );
        kp.sign_attestation(&mut att);

        let canonical = canonical_blob_bytes(&att);
        let sha = hex::encode(blob_sha256(&canonical));
        blob.put(&sha, &canonical).await?;
        let index = AttestationIndex {
            id: att.id,
            account_id: account.id,
            agent_id: att.agent_id.clone(),
            agent_pubkey_hex: hex::encode(att.agent_pubkey),
            customer_id: Some(customer_id),
            received_at: Utc::now(),
            blob_sha256_hex: sha,
        };
        db.insert_attestation(index).await?;
        db.record_usage(account.id, Utc::now()).await?;
        if i % 1000 == 0 && i > 0 {
            eprintln!("# seeded {i}/{SEED_COUNT}");
        }
    }
    eprintln!("# done");
    Ok(())
}

async fn seed_overage(db: PgDb) -> anyhow::Result<()> {
    const OVERAGE_TOTAL: usize = 1_050_000;
    let account = account_or_create(&db, "overage@local.test", Plan::Team).await?;
    let cleartext = generate_api_key();
    let phc = hash_api_key(&cleartext)?;
    db.create_api_key(account.id, "overage-project", &phc)
        .await?;

    // Link a synthetic Stripe customer so the bill-period trigger has a
    // subscription to charge against. The mock client doesn't care about
    // realism here; the field just has to be populated for the bill-period
    // code path to reach the metered post.
    db.set_account_stripe_ids(account.id, "cus_synthetic_overage", Some("sub_synthetic"))
        .await?;

    let at = Utc::now();
    println!("# AWP cloud seed (overage)");
    println!("# account_id : {}", account.id);
    println!("export AWP_API_KEY={cleartext}");
    println!(
        "# Recording {OVERAGE_TOTAL} usage points on {}...",
        at.date_naive()
    );

    // `record_usage` is one row-modification per call; for 1.05M that's
    // measurable but still under a minute on a local Postgres. We keep it
    // honest (no bulk-fudging) so the bill-period code reads exactly what
    // production would.
    for i in 0..OVERAGE_TOTAL {
        db.record_usage(account.id, at).await?;
        if i % 50_000 == 0 && i > 0 {
            eprintln!("# usage points recorded: {i}/{OVERAGE_TOTAL}");
        }
    }
    eprintln!("# done. Expected overage: 50,000 attestations → 50 units of 1k.");
    Ok(())
}

async fn seed_old_attestations(db: PgDb, blob_root: PathBuf) -> anyhow::Result<()> {
    const STALE_COUNT: usize = 100;
    let blob = FsBlobStore::new(&blob_root);

    let account = account_or_create(&db, "retention@local.test", Plan::Team).await?;
    let cleartext = generate_api_key();
    let phc = hash_api_key(&cleartext)?;
    db.create_api_key(account.id, "retention-project", &phc)
        .await?;
    println!("# AWP cloud seed (old-attestations)");
    println!("# account_id : {}", account.id);
    println!("export AWP_API_KEY={cleartext}");
    println!("# Inserting {STALE_COUNT} attestations dated 400 days ago...");

    let kp = AgentKeypair::generate();
    let stale_at = Utc::now() - Duration::days(400);

    for i in 0..STALE_COUNT {
        let customer_id = format!("STALE-{i:03}");
        let task_hash = awp_core::signing::sha256(format!("stale-task-{i}").as_bytes());
        let output = serde_json::json!({
            "customer_id": customer_id,
            "decision": "Approve",
        })
        .to_string();
        let mut att = Attestation::new(
            "agent-retention",
            task_hash,
            output,
            AttestationStatus::Completed,
            None,
            stale_at.timestamp(),
        );
        kp.sign_attestation(&mut att);
        let canonical = canonical_blob_bytes(&att);
        let sha = hex::encode(blob_sha256(&canonical));
        blob.put(&sha, &canonical).await?;
        let index = AttestationIndex {
            id: att.id,
            account_id: account.id,
            agent_id: att.agent_id.clone(),
            agent_pubkey_hex: hex::encode(att.agent_pubkey),
            customer_id: Some(customer_id),
            received_at: stale_at,
            blob_sha256_hex: sha,
        };
        db.insert_attestation(index).await?;
    }
    eprintln!(
        "# done. Run `docker compose exec awp-cloud /usr/local/bin/sweeper run-once` to verify."
    );
    Ok(())
}
