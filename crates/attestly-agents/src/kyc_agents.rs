//! KYC-flavoured Worker and Verifier — parallel to [`crate::worker::Worker`]
//! and [`crate::verifier::Verifier`] but operating over [`KycRequest`] and
//! [`KycDecision`] instead of [`crate::worker::WorkerTask`].
//!
//! ## Why a separate type rather than a generic trait
//!
//! Per the GTM Phase 1 plan (Step 2 — KYC Receipts Demo), we deliberately
//! ship parallel structs rather than generalising
//! [`crate::worker::WorkerAgent`] / [`crate::verifier::VerifierAgent`] over
//! the task type. Trait generalisation (option (a) in the plan) is the
//! cleaner long-term design but it ripples through the Dispatcher,
//! ParallelDispatcher, Batcher, and four existing examples. The duplication
//! is small (~90 lines, mostly the build-then-sign attestation boilerplate)
//! and the GTM Phase 1 brief explicitly prefers the smaller refactor:
//! "Pick option (b) unless option (a) is cleaner than expected."

use async_trait::async_trait;
use attestly_core::{
    sha256, AgentIdentity, AgentKeypair, Attestation, AttestationStatus, KycDecision, KycRequest,
};
use thiserror::Error;

use crate::tools::{kyc_decide, verify_attestation_struct};

/// Errors a [`KycWorker`] can return. Decision-rule outcomes (Approve, Flag,
/// Reject) are *not* errors — they flow through the attestation. Only
/// signing/io problems would surface here; we keep the enum for symmetry
/// with [`crate::worker::WorkerError`] and to leave room for future
/// non-determinism (e.g. an LLM-backed rule).
#[derive(Debug, Error)]
pub enum KycWorkerError {}

/// Errors a [`KycVerifier`] can return. As with [`KycWorkerError`], expected
/// disagreements flow through the attestation rather than this enum.
#[derive(Debug, Error)]
pub enum KycVerifierError {}

/// A Worker that runs the KYC decision rule and emits a signed attestation.
pub struct KycWorker {
    agent_id: String,
    keypair: AgentKeypair,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl KycWorker {
    /// Construct a KycWorker with a freshly-generated **ephemeral** keypair.
    /// Convenience constructor for tests; the GTM Phase 1 KYC demo uses
    /// [`KycWorker::with_identity`] with a [`attestly_core::FileIdentityStore`]
    /// so the agent's `agent_pubkey` is stable across runs.
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self::with_identity(AgentIdentity::generate(agent_id))
    }

    /// Construct a KycWorker that signs with the given persisted (or
    /// ephemeral) identity. This is the constructor the kyc_receipts demo
    /// uses to produce stable `agent_pubkey` values across restarts.
    pub fn with_identity(identity: AgentIdentity) -> Self {
        Self {
            agent_id: identity.agent_id,
            keypair: identity.keypair,
            clock: Box::new(default_clock),
        }
    }

    pub fn with_clock<F>(mut self, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        self.clock = Box::new(clock);
        self
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.keypair.public_bytes()
    }
}

#[async_trait]
pub trait KycWorkerAgent {
    async fn run(&self, request: &KycRequest) -> Result<Attestation, KycWorkerError>;
}

#[async_trait]
impl KycWorkerAgent for KycWorker {
    async fn run(&self, request: &KycRequest) -> Result<Attestation, KycWorkerError> {
        let task_hash = sha256(&request.canonical_bytes());
        let timestamp = (self.clock)();

        let decision = kyc_decide(request);
        let output = decision_output(&decision);

        let mut a = Attestation::new(
            self.agent_id.clone(),
            task_hash,
            output,
            AttestationStatus::Completed,
            None,
            timestamp,
        );
        self.keypair.sign_attestation(&mut a);
        Ok(a)
    }
}

/// A Verifier that crypto-checks a Worker's KYC attestation, then
/// independently re-runs the decision rule and emits its own signed
/// attestation containing both judgements.
pub struct KycVerifier {
    agent_id: String,
    keypair: AgentKeypair,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl KycVerifier {
    /// Construct a KycVerifier with a freshly-generated **ephemeral**
    /// keypair. Convenience constructor for tests; production callers (and
    /// the GTM Phase 1 KYC demo) use [`KycVerifier::with_identity`].
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self::with_identity(AgentIdentity::generate(agent_id))
    }

    /// Construct a KycVerifier that signs with the given persisted (or
    /// ephemeral) identity.
    pub fn with_identity(identity: AgentIdentity) -> Self {
        Self {
            agent_id: identity.agent_id,
            keypair: identity.keypair,
            clock: Box::new(default_clock),
        }
    }

    pub fn with_clock<F>(mut self, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        self.clock = Box::new(clock);
        self
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.keypair.public_bytes()
    }
}

#[async_trait]
pub trait KycVerifierAgent {
    async fn run(
        &self,
        request: &KycRequest,
        worker_attestation: &Attestation,
    ) -> Result<Attestation, KycVerifierError>;
}

#[async_trait]
impl KycVerifierAgent for KycVerifier {
    async fn run(
        &self,
        request: &KycRequest,
        worker_attestation: &Attestation,
    ) -> Result<Attestation, KycVerifierError> {
        // 1. Cryptographic check on the Worker's attestation.
        let crypto = verify_attestation_struct(worker_attestation);
        let attestation_valid = crypto.overall_valid;

        // 2. Independent re-decision. The decision rule is pure and
        //    deterministic — if both agents see the same request, they
        //    must reach the same KycDecision. If the verifier sees a
        //    different decision string than the worker emitted, that is
        //    the verifier's signal that the worker reported a wrong
        //    answer.
        let our_decision = kyc_decide(request);
        let our_output = decision_output(&our_decision);

        // 3. answer_correct ⇔ worker emitted Completed and we reached
        //    the same decision string.
        let answer_correct = matches!(worker_attestation.status, AttestationStatus::Completed)
            && our_output == worker_attestation.output;

        let task_hash = sha256(&request.canonical_bytes());
        let timestamp = (self.clock)();
        let output =
            format!("attestation_valid={attestation_valid} answer_correct={answer_correct}");

        let mut a = Attestation::new(
            self.agent_id.clone(),
            task_hash,
            output,
            AttestationStatus::Verified {
                attestation_valid,
                answer_correct,
            },
            Some(worker_attestation.id),
            timestamp,
        );
        self.keypair.sign_attestation(&mut a);
        Ok(a)
    }
}

/// Canonical string encoding of a [`KycDecision`] used as the attestation
/// `output` field. Both Worker and Verifier go through this so their
/// outputs are byte-comparable.
pub fn decision_output(decision: &KycDecision) -> String {
    serde_json::to_string(decision).expect("KycDecision serialization is infallible")
}

fn default_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use attestly_core::verify_attestation_signature;

    #[tokio::test]
    async fn kyc_worker_new_produces_ephemeral_keypair() {
        // Symmetry with the Worker test: `KycWorker::new` must remain a
        // fresh-keypair-per-call constructor so it keeps working in tests
        // that don't want filesystem state.
        let a = KycWorker::new("kyc-worker");
        let b = KycWorker::new("kyc-worker");
        assert_ne!(a.public_key(), b.public_key());
    }

    #[tokio::test]
    async fn kyc_worker_with_identity_uses_supplied_keypair() {
        let identity = AgentIdentity::generate("kyc-worker-persistent");
        let public = identity.public_bytes();
        let w = KycWorker::with_identity(identity).with_clock(|| 1_700_000_000);
        assert_eq!(w.public_key(), public);

        let req = KycRequest::new("cust-1", 499, "US", "card");
        let att = w.run(&req).await.unwrap();
        assert_eq!(att.agent_pubkey, public);
        assert!(verify_attestation_signature(&att));
    }

    #[tokio::test]
    async fn kyc_verifier_with_identity_uses_supplied_keypair() {
        let identity = AgentIdentity::generate("kyc-verifier-persistent");
        let public = identity.public_bytes();
        let v = KycVerifier::with_identity(identity).with_clock(|| 1_700_000_001);
        assert_eq!(v.public_key(), public);

        let w = KycWorker::new("kyc-worker").with_clock(|| 1_700_000_000);
        let req = KycRequest::new("cust-1", 499, "US", "card");
        let worker_att = w.run(&req).await.unwrap();
        let verifier_att = v.run(&req, &worker_att).await.unwrap();

        assert_eq!(verifier_att.agent_pubkey, public);
        assert!(verify_attestation_signature(&verifier_att));
    }

    #[tokio::test]
    async fn kyc_worker_signs_approve_decision() {
        let w = KycWorker::new("kyc-worker-1").with_clock(|| 1_700_000_000);
        let req = KycRequest::new("cust-1", 499, "US", "card");
        let att = w.run(&req).await.unwrap();
        assert_eq!(att.agent_id, "kyc-worker-1");
        assert_eq!(att.status, AttestationStatus::Completed);
        assert_eq!(att.output, decision_output(&KycDecision::Approve));
        assert!(verify_attestation_signature(&att));
    }

    #[tokio::test]
    async fn kyc_worker_signs_flag_decision() {
        let w = KycWorker::new("kyc-worker-1").with_clock(|| 1_700_000_000);
        let req = KycRequest::new("cust-2", 5_000_000, "US", "card");
        let att = w.run(&req).await.unwrap();
        let decoded: KycDecision = serde_json::from_str(&att.output).unwrap();
        match decoded {
            KycDecision::Flag { reasons } => {
                assert_eq!(reasons, vec!["amount exceeds threshold".to_string()]);
            }
            other => panic!("expected Flag, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn kyc_verifier_agrees_with_correct_worker() {
        let w = KycWorker::new("kyc-worker-1").with_clock(|| 1_700_000_000);
        let v = KycVerifier::new("kyc-verifier-1").with_clock(|| 1_700_000_001);
        let req = KycRequest::new("cust-3", 499, "US", "card");

        let worker_att = w.run(&req).await.unwrap();
        let verifier_att = v.run(&req, &worker_att).await.unwrap();

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
    async fn kyc_verifier_disagrees_with_tampered_output() {
        let w = KycWorker::new("kyc-worker-1").with_clock(|| 1_700_000_000);
        let v = KycVerifier::new("kyc-verifier-1").with_clock(|| 1_700_000_001);
        let req = KycRequest::new("cust-4", 5_000_000, "US", "card");

        let mut worker_att = w.run(&req).await.unwrap();
        // Tamper after signing — both signature and output_hash become invalid.
        worker_att.output = decision_output(&KycDecision::Approve);

        let verifier_att = v.run(&req, &worker_att).await.unwrap();
        assert_eq!(
            verifier_att.status,
            AttestationStatus::Verified {
                attestation_valid: false,
                answer_correct: false,
            }
        );
    }
}
