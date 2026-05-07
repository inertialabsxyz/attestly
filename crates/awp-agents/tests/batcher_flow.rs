//! Phase 3 integration test for the Batcher.
//!
//! Exercises the Batcher end-to-end: submit attestations, force a
//! flush, then re-open the database from a fresh `Storage` handle and
//! confirm a proof retrieved from disk verifies cryptographically.
//!
//! These cover the two assertions the Phase 3 prompt calls out:
//!
//! 1. Submitting `max_batch_size` attestations triggers a count-based
//!    flush automatically.
//! 2. A proof loaded from SQLite by id verifies given only root + proof
//!    + the (re-loaded) attestation.

use std::sync::Arc;
use std::time::Duration;

use awp_agents::{Batcher, BatcherConfig};
use awp_core::{sha256, verify_inclusion, AgentKeypair, Attestation, AttestationStatus, Storage};
use tempfile::tempdir;

fn signed(kp: &AgentKeypair, output: &str) -> Attestation {
    let mut a = Attestation::new(
        "agent",
        sha256(b"task"),
        output,
        AttestationStatus::Completed,
        None,
        1_700_000_000,
    );
    kp.sign_attestation(&mut a);
    a
}

#[tokio::test]
async fn count_trigger_persists_a_batch_with_verifiable_proofs() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("awp.db");

    let storage = Storage::open(&db_path).unwrap();
    let batcher = Arc::new(Batcher::start(
        BatcherConfig {
            max_batch_size: 5,
            max_batch_age_secs: 3600,
            min_batch_size: 1,
        },
        storage,
        || 1_700_000_000,
    ));

    let kp = AgentKeypair::generate();
    let attestations: Vec<_> = (0..5).map(|i| signed(&kp, &format!("o{i}"))).collect();
    for a in &attestations {
        batcher.submit(a.clone()).await.unwrap();
    }
    // Wait briefly for the background task to drain and flush.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // No further attestations to flush — the count trigger fired.
    assert!(batcher.flush_now().await.unwrap().is_none());

    Arc::try_unwrap(batcher)
        .map_err(|_| "still referenced")
        .unwrap()
        .shutdown()
        .await
        .unwrap();

    // Re-open from disk and confirm every submitted attestation is
    // batched + cryptographically verifiable.
    let storage = Storage::open(&db_path).unwrap();
    for a in &attestations {
        assert!(
            storage.verify_attestation_inclusion(a.id).unwrap(),
            "attestation {} should verify inclusion",
            a.id
        );
        // Independent re-verification using the bare verify_inclusion API:
        // load the attestation and proof, recompute the leaf, and check
        // against the batch root.
        let proof = storage.get_proof(a.id).unwrap();
        let batch = storage.get_batch(proof.batch_id).unwrap();
        let stored_att = storage.get_attestation(a.id).unwrap();
        let leaf = awp_core::attestation_leaf_hash(&stored_att);
        assert!(
            verify_inclusion(&batch.merkle_root, &leaf, &proof),
            "bare verify_inclusion should accept root + proof + leaf"
        );
    }
}

#[tokio::test]
async fn shutdown_flushes_a_partial_buffer() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("awp.db");
    let storage = Storage::open(&db_path).unwrap();
    let batcher = Batcher::start(
        BatcherConfig {
            max_batch_size: 100,
            max_batch_age_secs: 3600,
            min_batch_size: 1,
        },
        storage,
        || 1_700_000_000,
    );
    let kp = AgentKeypair::generate();
    for i in 0..3 {
        batcher.submit(signed(&kp, &format!("o{i}"))).await.unwrap();
    }
    let id = batcher.shutdown().await.unwrap();
    assert!(id.is_some(), "shutdown must seal the partial buffer");

    // Reopen and confirm the batch exists with 3 attestations.
    let storage = Storage::open(&db_path).unwrap();
    let batch = storage.get_batch(id.unwrap()).unwrap();
    assert_eq!(batch.attestation_count, 3);
    assert_eq!(batch.attestation_ids.len(), 3);
}

#[tokio::test]
async fn force_flush_after_partial_submit_seals_a_batch() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("awp.db");
    let storage = Storage::open(&db_path).unwrap();
    let batcher = Batcher::start(
        BatcherConfig {
            max_batch_size: 100,
            max_batch_age_secs: 3600,
            min_batch_size: 1,
        },
        storage,
        || 1_700_000_000,
    );
    let kp = AgentKeypair::generate();
    let attestations: Vec<_> = (0..4).map(|i| signed(&kp, &format!("o{i}"))).collect();
    for a in &attestations {
        batcher.submit(a.clone()).await.unwrap();
    }
    let batch_id = batcher.flush_now().await.unwrap().expect("a batch");
    let final_id = batcher.shutdown().await.unwrap();
    assert!(
        final_id.is_none(),
        "no buffered attestations remained after flush_now"
    );

    let storage = Storage::open(&db_path).unwrap();
    let batch = storage.get_batch(batch_id).unwrap();
    assert_eq!(batch.attestation_count, 4);
    for a in &attestations {
        assert!(storage.verify_attestation_inclusion(a.id).unwrap());
    }
}
