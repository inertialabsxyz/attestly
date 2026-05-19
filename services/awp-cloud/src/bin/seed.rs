//! `make seed` entrypoint.
//!
//! Inserts 10k synthetic attestations under a single test account so the
//! search and pagination paths can be exercised by hand. Prints the API key
//! to stdout exactly once — capture it into `$TEST_KEY` for the verification
//! curl commands in the README.
//!
//! Requires `DATABASE_URL` and `BLOB_ROOT` to be set the same way `awp-cloud`
//! itself does.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use awp_core::{signing::AgentKeypair, Attestation, AttestationStatus};
use chrono::Utc;

use awp_cloud::auth::{generate_api_key, hash_api_key};
use awp_cloud::blob::filesystem::FsBlobStore;
use awp_cloud::blob::BlobStore;
use awp_cloud::canonical::{blob_sha256, canonical_blob_bytes};
use awp_cloud::store::postgres::PgDb;
use awp_cloud::store::{AttestationIndex, Db, Plan};

const SEED_COUNT: usize = 10_000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let blob_root: PathBuf = std::env::var("BLOB_ROOT")
        .unwrap_or_else(|_| "./data/blobs".to_string())
        .into();

    let db = PgDb::connect(&database_url, 8).await?;
    db.apply_migrations().await?;
    let blob = FsBlobStore::new(&blob_root);

    let account = db.create_account("seed@local.test", Plan::Team).await?;
    let cleartext = generate_api_key();
    let phc = hash_api_key(&cleartext)?;
    db.create_api_key(account.id, "seed-project", &phc).await?;

    println!("# AWP cloud seed");
    println!("# account_id : {}", account.id);
    println!("export TEST_KEY={cleartext}");
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
