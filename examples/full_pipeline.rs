//! Phase 3 checkpoint binary.
//!
//! Drives 12 tasks through the full Dispatcher → Batcher → SQLite
//! pipeline, then verifies every batched attestation's inclusion in its
//! Merkle batch using only the stored root + proof. Finally, tampers
//! with one attestation row in SQLite and confirms that verification
//! fails after tampering.
//!
//! Each task produces a Worker attestation and a Verifier attestation,
//! so the pipeline submits 24 attestations in total. With the default
//! `max_batch_size = 10` this triggers two count-based batches plus a
//! final shutdown flush of the remaining 4 — exercising both the size-10
//! count boundary and the shutdown-flush path.
//!
//! Run with:
//!
//! ```text
//! cargo run --example full_pipeline
//! ```
//!
//! The run is destructive: it deletes any existing `data/awp.db` so each
//! run starts from a known-empty database. The Phase 1/2 JSON logs
//! (`data/attestations.json`, `data/executions.json`) are appended to as
//! before — preserving the Phase 1/2 contract.

use std::path::PathBuf;
use std::sync::Arc;

use awp_agents::{
    Batcher, BatcherConfig, Dispatcher, DispatcherConfig, Verifier, Worker, WorkerTask,
};
use awp_core::Storage;
use rusqlite::{params, Connection};
use uuid::Uuid;

const DB_PATH: &str = "data/awp.db";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Reset the SQLite database so the assertions below see a clean
    // slate. The JSON logs continue to grow across runs (Phase 1/2
    // contract).
    let db_path = PathBuf::from(DB_PATH);
    if db_path.exists() {
        std::fs::remove_file(&db_path)?;
    }

    let storage = Storage::open(&db_path)?;
    let batcher = Arc::new(Batcher::start(BatcherConfig::default(), storage, || {
        chrono::Utc::now().timestamp()
    }));

    let dispatcher = Dispatcher::new(DispatcherConfig::standard()).with_batcher(batcher.clone());

    let worker = Worker::new("worker-1");
    let verifier = Verifier::new("verifier-1");

    println!("--- Phase 3 full pipeline: 12 tasks → 24 attestations → SQLite batches");

    for i in 0..12 {
        let task = WorkerTask::new(format!("{i} * 7"));
        let exec = dispatcher.run(&worker, &verifier, task).await?;
        println!(
            "task {i:>2}: {}  worker={}  verifier={}",
            short_status(&exec.status),
            exec.worker_attestation
                .as_ref()
                .map(|a| a.id.to_string())
                .unwrap_or_else(|| "—".into()),
            exec.verifier_attestation
                .as_ref()
                .map(|a| a.id.to_string())
                .unwrap_or_else(|| "—".into()),
        );
    }

    // We need a separate Storage handle for the assertion phase since the
    // batcher owns its own. The batcher will be shut down first to ensure
    // its final flush lands. The Dispatcher must be dropped before
    // try_unwrap so the only remaining Arc is `batcher` itself.
    drop(dispatcher);
    let final_batch = Arc::try_unwrap(batcher)
        .map_err(|_| "batcher Arc still has other references")?
        .shutdown()
        .await?;
    if let Some(id) = final_batch {
        println!("\nshutdown flushed final batch: {id}");
    } else {
        println!("\nshutdown: no remaining buffered attestations to flush");
    }

    let assert_storage = Storage::open(&db_path)?;

    // -- Assertion 1: batches exist and inclusion verifies for every batched
    //    attestation, given only root + proof + attestation.
    let batch_ids = list_batch_ids(&db_path)?;
    println!(
        "\nSQLite contains {} batch(es): {:?}",
        batch_ids.len(),
        batch_ids
    );
    assert!(
        !batch_ids.is_empty(),
        "expected at least one batch to be sealed"
    );

    let batched_attestation_ids = list_batched_attestation_ids(&db_path)?;
    println!(
        "checking inclusion for {} batched attestation(s)…",
        batched_attestation_ids.len()
    );
    for id in &batched_attestation_ids {
        assert!(
            assert_storage.verify_attestation_inclusion(*id)?,
            "attestation {id} should verify"
        );
    }
    println!("all batched attestations verified given only root + proof.");

    // -- Assertion 2: tampering with one attestation's output breaks
    //    inclusion verification.
    let target = batched_attestation_ids[0];
    println!("\ntampering with attestation {target}…");
    {
        let conn = Connection::open(&db_path)?;
        conn.execute(
            "UPDATE attestations SET output = 'evil' WHERE id = ?1",
            params![target.to_string()],
        )?;
    }
    let after_tamper = Storage::open(&db_path)?;
    let post_tamper_result = after_tamper.verify_attestation_inclusion(target)?;
    println!("verify_attestation_inclusion({target}) after tamper = {post_tamper_result}");
    assert!(
        !post_tamper_result,
        "tampered attestation must not pass inclusion verification"
    );

    println!("\n--- Phase 3 pipeline complete. SQLite at {DB_PATH}");
    Ok(())
}

fn list_batch_ids(db: &PathBuf) -> Result<Vec<Uuid>, Box<dyn std::error::Error>> {
    let conn = Connection::open(db)?;
    let mut stmt = conn.prepare("SELECT id FROM batches ORDER BY created_at")?;
    let ids: Vec<Uuid> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(ids)
}

fn list_batched_attestation_ids(db: &PathBuf) -> Result<Vec<Uuid>, Box<dyn std::error::Error>> {
    let conn = Connection::open(db)?;
    let mut stmt =
        conn.prepare("SELECT id FROM attestations WHERE batch_id IS NOT NULL ORDER BY rowid")?;
    let ids: Vec<Uuid> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();
    Ok(ids)
}

fn short_status(status: &awp_core::ExecutionStatus) -> String {
    match status {
        awp_core::ExecutionStatus::Pending => "Pending".into(),
        awp_core::ExecutionStatus::WorkerRunning => "WorkerRunning".into(),
        awp_core::ExecutionStatus::WorkerComplete => "WorkerComplete".into(),
        awp_core::ExecutionStatus::VerifierRunning => "VerifierRunning".into(),
        awp_core::ExecutionStatus::Complete {
            attestation_valid,
            answer_correct,
        } => format!("Complete{{av={attestation_valid},ac={answer_correct}}}"),
        awp_core::ExecutionStatus::Failed { stage, reason } => format!("Failed[{stage}:{reason}]"),
    }
}
