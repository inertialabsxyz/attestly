//! Phase 1 integration test: drive Worker → Verifier through the same
//! code path the example uses, persist both attestations, reload, and
//! confirm everything cryptographically links up.
//!
//! Mirrors the Phase 1 exit criteria:
//! - Worker produces signed attestation
//! - Verifier calls verify_attestation (crypto check) + calculate (re-solve)
//! - Verifier emits attestation with attestation_valid + answer_correct
//! - Both persist to JSON-lines log and survive a round trip

use awp_agents::{Verifier, VerifierAgent, Worker, WorkerAgent, WorkerTask};
use awp_core::{
    append_attestation, load_attestations, verify_attestation_signature, AttestationStatus,
};
use tempfile::tempdir;

#[tokio::test]
async fn worker_verifier_flow_end_to_end() {
    let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
    let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
    let task = WorkerTask::new("140 * 0.6");

    let worker_att = worker.run(&task).await.unwrap();
    let verifier_att = verifier.run(&task, &worker_att).await.unwrap();

    assert_eq!(worker_att.status, AttestationStatus::Completed);
    assert_eq!(worker_att.output, "84");
    assert!(verify_attestation_signature(&worker_att));

    assert_eq!(verifier_att.references, Some(worker_att.id));
    assert_eq!(
        verifier_att.status,
        AttestationStatus::Verified {
            attestation_valid: true,
            answer_correct: true,
        }
    );
    assert!(verify_attestation_signature(&verifier_att));
}

#[tokio::test]
async fn attestations_round_trip_through_jsonl_log() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("attestations.json");

    let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
    let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
    let task = WorkerTask::new("(2 + 3) * 4");

    let worker_att = worker.run(&task).await.unwrap();
    let verifier_att = verifier.run(&task, &worker_att).await.unwrap();

    append_attestation(&path, &worker_att).unwrap();
    append_attestation(&path, &verifier_att).unwrap();

    let loaded = load_attestations(&path).unwrap();
    assert_eq!(loaded.len(), 2, "both attestations survive reload");
    assert_eq!(loaded[0], worker_att);
    assert_eq!(loaded[1], verifier_att);

    // Append a tampered third attestation; load should drop it but keep the
    // earlier two.
    let mut tampered = verifier_att.clone();
    tampered.output = "evil".into();
    append_attestation(&path, &tampered).unwrap();

    let after_tamper = load_attestations(&path).unwrap();
    assert_eq!(
        after_tamper.len(),
        2,
        "tampered record dropped on reload, original two preserved"
    );
}

#[tokio::test]
async fn second_run_appends_and_all_verify() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("attestations.json");

    let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
    let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
    let task = WorkerTask::new("140 * 0.6");

    for _ in 0..2 {
        let w_att = worker.run(&task).await.unwrap();
        let v_att = verifier.run(&task, &w_att).await.unwrap();
        append_attestation(&path, &w_att).unwrap();
        append_attestation(&path, &v_att).unwrap();
    }

    let loaded = load_attestations(&path).unwrap();
    assert_eq!(loaded.len(), 4, "two runs append two pairs of records");
    for att in &loaded {
        assert!(verify_attestation_signature(att));
    }
}
