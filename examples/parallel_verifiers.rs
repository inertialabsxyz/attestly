//! Phase 4 stretch-task example: parallel verifiers with disagreement detection.
//!
//! Drives one task through the `ParallelDispatcher` against:
//!
//!   - run 1: three identical `Verifier`s (unanimous agreement)
//!   - run 2: two identical `Verifier`s plus one *adversarial* verifier
//!     that always reports `answer_correct: false` regardless of the
//!     worker's actual output. This is the disagreement case the Phase 4
//!     stretch task asks the Dispatcher to detect.
//!
//! Run with:
//!
//! ```text
//! cargo run --example parallel_verifiers
//! ```
//!
//! The run is *non-destructive* — appends to the same JSON logs the
//! Phase 1/2 examples write to so the existing rollover behaviour is
//! preserved.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use awp_agents::{
    DispatcherConfig, ParallelDispatcher, Verifier, VerifierAgent, VerifierError, Worker,
    WorkerTask,
};
use awp_core::{sha256, AgentKeypair, Attestation, AttestationStatus};

/// A verifier that always disagrees — used to drive the disagreement
/// detection path. Its output is *signed*, just like a real verifier,
/// so the disagreement is purely about the verdict, not crypto.
struct AdversarialVerifier {
    agent_id: String,
    keypair: AgentKeypair,
}

impl AdversarialVerifier {
    fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            keypair: AgentKeypair::generate(),
        }
    }
}

#[async_trait]
impl VerifierAgent for AdversarialVerifier {
    async fn run(
        &self,
        task: &WorkerTask,
        worker_attestation: &Attestation,
    ) -> Result<Attestation, VerifierError> {
        let mut a = Attestation::new(
            &self.agent_id,
            sha256(task.canonical_bytes()),
            "adversarial verdict".to_string(),
            AttestationStatus::Verified {
                attestation_valid: true,
                answer_correct: false,
            },
            Some(worker_attestation.id),
            chrono::Utc::now().timestamp(),
        );
        self.keypair.sign_attestation(&mut a);
        Ok(a)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let attestations_path = PathBuf::from("data/attestations.json");
    let executions_path = PathBuf::from("data/executions.json");
    if let Some(parent) = attestations_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let dispatcher = ParallelDispatcher::new(
        DispatcherConfig::standard()
            .with_paths(&attestations_path, &executions_path)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5)),
    );

    let worker = Worker::new("worker-1");
    let v1 = Verifier::new("verifier-1");
    let v2 = Verifier::new("verifier-2");
    let v3 = Verifier::new("verifier-3");

    println!("--- run 1: three honest verifiers (expect unanimous agreement) ---");
    let exec1 = dispatcher
        .run(
            &worker,
            &[
                &v1 as &dyn VerifierAgent,
                &v2 as &dyn VerifierAgent,
                &v3 as &dyn VerifierAgent,
            ],
            WorkerTask::new("140 * 0.6"),
        )
        .await?;
    println!(
        "  task_id={} status={:?} disagreement={}",
        exec1.task_id, exec1.status, exec1.disagreement
    );
    println!(
        "  worker={} verifiers={:?}",
        exec1.worker_attestation.as_ref().unwrap().id,
        exec1
            .verifier_attestations
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>()
    );
    assert!(!exec1.disagreement, "honest verifiers should agree");
    println!("  → unanimous agreement detected.");

    println!("\n--- run 2: two honest + one adversarial verifier (expect disagreement) ---");
    let adversary = AdversarialVerifier::new("adversary-1");
    let exec2 = dispatcher
        .run(
            &worker,
            &[
                &v1 as &dyn VerifierAgent,
                &v2 as &dyn VerifierAgent,
                &adversary as &dyn VerifierAgent,
            ],
            WorkerTask::new("140 * 0.6"),
        )
        .await?;
    println!(
        "  task_id={} status={:?} disagreement={}",
        exec2.task_id, exec2.status, exec2.disagreement
    );
    for v in &exec2.verifier_attestations {
        if let AttestationStatus::Verified {
            attestation_valid,
            answer_correct,
        } = &v.status
        {
            println!(
                "    verifier={} attestation_valid={} answer_correct={}",
                v.agent_id, attestation_valid, answer_correct
            );
        }
    }
    assert!(
        exec2.disagreement,
        "adversarial verifier should produce disagreement"
    );
    println!("  → disagreement detected; all 3 verifier attestations persisted.");

    println!("\n--- attestations log: {}", attestations_path.display());
    println!("--- executions   log: {}", executions_path.display());
    Ok(())
}
