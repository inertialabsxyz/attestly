//! GTM Phase 1 Step 2 integration test: drive the KYC Worker → Verifier
//! flow end-to-end and assert that the tampered scenario produces a
//! Verifier attestation with `attestation_valid: false`.
//!
//! Mirrors the Phase 1 plan exit criterion:
//! "Tampered scenario produces a Verifier attestation with
//!  `attestation_valid: false` and the demo's stdout flags it clearly."

use attestly_agents::{decision_output, KycVerifier, KycVerifierAgent, KycWorker, KycWorkerAgent};
use attestly_core::{
    append_attestation, load_attestations, verify_attestation_signature, AttestationStatus,
    KycDecision, KycRequest,
};
use tempfile::tempdir;

#[tokio::test]
async fn kyc_clean_path_verifier_agrees() {
    let worker = KycWorker::new("agent-kyc-01").with_clock(|| 1_700_000_000);
    let verifier = KycVerifier::new("agent-kyc-02").with_clock(|| 1_700_000_001);

    let req = KycRequest::new("4711", 8_999, "US", "card");
    let worker_att = worker.run(&req).await.unwrap();
    let verifier_att = verifier.run(&req, &worker_att).await.unwrap();

    assert_eq!(worker_att.status, AttestationStatus::Completed);
    assert_eq!(worker_att.output, decision_output(&KycDecision::Approve));
    assert!(verify_attestation_signature(&worker_att));

    assert_eq!(
        verifier_att.status,
        AttestationStatus::Verified {
            attestation_valid: true,
            answer_correct: true,
        }
    );
    assert_eq!(verifier_att.references, Some(worker_att.id));
    assert!(verify_attestation_signature(&verifier_att));
}

#[tokio::test]
async fn kyc_tampered_path_is_caught_by_verifier() {
    let worker = KycWorker::new("agent-kyc-01").with_clock(|| 1_700_000_000);
    let verifier = KycVerifier::new("agent-kyc-02").with_clock(|| 1_700_000_001);

    // A request that would correctly Flag (amount > threshold).
    let req = KycRequest::new("9999", 2_500_000, "US", "card");

    // Worker signs over the correct Flag decision.
    let mut worker_att = worker.run(&req).await.unwrap();
    let signed_decision: KycDecision = serde_json::from_str(&worker_att.output).unwrap();
    assert!(matches!(signed_decision, KycDecision::Flag { .. }));
    assert!(verify_attestation_signature(&worker_att));

    // ---- Tamper after signing --------------------------------------------
    // Mutate the in-memory `output` to look like an Approve. The signature
    // covers the original output_hash, so the Verifier's crypto check must
    // detect the mismatch.
    worker_att.output = decision_output(&KycDecision::Approve);
    // ----------------------------------------------------------------------

    // The signature itself should now be cryptographically invalid because
    // the signing payload includes `output` and `output_hash`.
    assert!(!verify_attestation_signature(&worker_att));

    let verifier_att = verifier.run(&req, &worker_att).await.unwrap();
    assert_eq!(
        verifier_att.status,
        AttestationStatus::Verified {
            attestation_valid: false,
            answer_correct: false,
        },
        "verifier must catch the post-signing tamper"
    );
    // The verifier's own attestation is still cryptographically valid — the
    // verifier signed an honest report of "the worker's receipt is broken".
    assert!(verify_attestation_signature(&verifier_att));
}

#[tokio::test]
async fn kyc_tampered_attestation_is_dropped_on_reload() {
    // Persist a tampered attestation; load_attestations should drop it
    // because its signature no longer verifies. This is the audit-viewer's
    // analogue: a tampered receipt is not just flagged, it is unreadable.
    let dir = tempdir().unwrap();
    let path = dir.path().join("attestations.json");

    let worker = KycWorker::new("agent-kyc-01").with_clock(|| 1_700_000_000);
    let req = KycRequest::new("9999", 2_500_000, "US", "card");
    let mut att = worker.run(&req).await.unwrap();
    att.output = decision_output(&KycDecision::Approve);

    append_attestation(&path, &att).unwrap();

    let loaded = load_attestations(&path).unwrap();
    assert!(
        loaded.is_empty(),
        "tampered attestation must not survive load_attestations"
    );
}
