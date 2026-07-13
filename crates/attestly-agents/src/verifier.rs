//! Verifier agent — checks a Worker attestation cryptographically, then
//! independently re-solves the task and produces its own signed
//! attestation.
//!
//! See `worker.rs` for the framework note on why these agents are plain
//! async types rather than wired through `AgentBuilder.run()`.

use async_trait::async_trait;
use attestly_core::{sha256, AgentIdentity, AgentKeypair, Attestation, AttestationStatus};
use thiserror::Error;

use crate::tools::{calculate, verify_attestation_struct, ToolError};
use crate::worker::{format_result, WorkerTask};

/// Errors a Verifier can return. As with the Worker, expected disagreements
/// (`answer_correct: false`) flow through the attestation, not this enum.
#[derive(Debug, Error)]
pub enum VerifierError {
    #[error("tool error: {0}")]
    Tool(#[from] ToolError),
}

pub struct Verifier {
    agent_id: String,
    keypair: AgentKeypair,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl Verifier {
    /// Construct a Verifier with a freshly-generated **ephemeral** keypair.
    /// Provided for backwards compatibility with the original four examples;
    /// for persistent identity use [`Verifier::with_identity`].
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self::with_identity(AgentIdentity::generate(agent_id))
    }

    /// Construct a Verifier that signs with the given persisted (or
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
pub trait VerifierAgent {
    async fn run(
        &self,
        task: &WorkerTask,
        worker_attestation: &Attestation,
    ) -> Result<Attestation, VerifierError>;
}

#[async_trait]
impl VerifierAgent for Verifier {
    /// Verify the Worker's attestation, then independently re-solve the task,
    /// then emit our own signed attestation containing both judgements.
    ///
    /// The flow matches the plan's "Verification Flow" diagram:
    ///   1. call `verify_attestation` tool (crypto check)
    ///   2. call `calculate` tool (independent solve)
    ///   3. reason about both → `Verified { attestation_valid, answer_correct }`
    async fn run(
        &self,
        task: &WorkerTask,
        worker_attestation: &Attestation,
    ) -> Result<Attestation, VerifierError> {
        // 1. Cryptographic check on the Worker's attestation.
        let crypto = verify_attestation_struct(worker_attestation);
        let attestation_valid = crypto.overall_valid;

        // 2. Independent re-solve. If the Worker's status was already Failed,
        //    we still attempt the independent solve so we can record whether
        //    we agree the task was unsolvable.
        let our_result = calculate(&task.expression);

        // 3. Reason about both. The Worker is "answer_correct" iff:
        //    - the worker emitted a Completed status, AND
        //    - our independently-computed result formats to the same string
        //      as the worker's output.
        let answer_correct = match (&worker_attestation.status, our_result) {
            (AttestationStatus::Completed, Ok(ours)) => {
                format_result(ours.result) == worker_attestation.output
            }
            // Worker said failed; if our solve also fails, we agree the task
            // is unsolvable, but we report `answer_correct: false` because
            // there is no correct answer to compare against. The plan does
            // not enumerate this case explicitly — see PAIN_POINTS.md Q4.
            _ => false,
        };

        let task_hash = sha256(task.canonical_bytes());
        let timestamp = (self.clock)();
        // The verifier's output is a short summary string capturing both
        // judgements; downstream readers parse the structured `status`
        // rather than this string.
        let output =
            format!("attestation_valid={attestation_valid} answer_correct={answer_correct}",);

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

fn default_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::{Worker, WorkerAgent, WorkerTask};
    use attestly_core::verify_attestation_signature;

    #[tokio::test]
    async fn verifier_new_produces_ephemeral_keypair_per_call() {
        // Mirrors the Worker backwards-compat test: `Verifier::new` must
        // keep producing a fresh ephemeral keypair on every call so the
        // four original examples continue to work unchanged.
        let a = Verifier::new("verifier-1");
        let b = Verifier::new("verifier-1");
        assert_ne!(
            a.public_key(),
            b.public_key(),
            "Verifier::new should generate a fresh keypair per call",
        );
    }

    #[tokio::test]
    async fn verifier_with_identity_uses_supplied_keypair() {
        let identity = AgentIdentity::generate("verifier-persistent");
        let public = identity.public_bytes();
        let v = Verifier::with_identity(identity).with_clock(|| 1_700_000_001);
        assert_eq!(v.public_key(), public);

        // Run end-to-end so we exercise the signing path.
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let task = WorkerTask::new("140 * 0.6");
        let worker_att = w.run(&task).await.unwrap();
        let verifier_att = v.run(&task, &worker_att).await.unwrap();

        assert_eq!(verifier_att.agent_pubkey, public);
        assert!(verify_attestation_signature(&verifier_att));
    }

    #[tokio::test]
    async fn verifier_agrees_with_correct_worker() {
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let v = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
        let task = WorkerTask::new("140 * 0.6");

        let worker_att = w.run(&task).await.unwrap();
        let verifier_att = v.run(&task, &worker_att).await.unwrap();

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
    async fn verifier_disagrees_with_tampered_output() {
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let v = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
        let task = WorkerTask::new("140 * 0.6");

        let mut worker_att = w.run(&task).await.unwrap();
        // Tamper after signing — corrupts both the signature and the
        // output_hash → output relationship.
        worker_att.output = "999".into();

        let verifier_att = v.run(&task, &worker_att).await.unwrap();
        assert_eq!(
            verifier_att.status,
            AttestationStatus::Verified {
                attestation_valid: false,
                answer_correct: false,
            }
        );
    }

    #[tokio::test]
    async fn verifier_flags_wrong_answer_with_valid_signature() {
        // Build a worker that returns a wrong-but-validly-signed answer:
        // sign first, then we don't tamper — instead we simulate a
        // miscalculation by swapping the worker's task.
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let v = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
        let solved_task = WorkerTask::new("100 * 0.6"); // 60
        let real_task = WorkerTask::new("140 * 0.6"); // 84

        // Worker happens to attest the wrong task. Sign over `solved_task`'s
        // hash so the signature is valid, but the verifier was asked
        // about `real_task`.
        let worker_att = w.run(&solved_task).await.unwrap();
        let verifier_att = v.run(&real_task, &worker_att).await.unwrap();

        match verifier_att.status {
            AttestationStatus::Verified {
                attestation_valid,
                answer_correct,
            } => {
                assert!(attestation_valid, "worker's signature is intact",);
                assert!(!answer_correct, "60 != 84");
            }
            other => panic!("expected Verified status, got {other:?}"),
        }
    }
}
