//! Phase 2 checkpoint binary.
//!
//! Drives three tasks through the Dispatcher to exercise every branch of
//! its state machine:
//!
//! 1. **Normal** — vanilla Worker → Verifier, both agree, Complete.
//! 2. **Worker timeout** — wraps the real Worker in a fault-injecting
//!    sleep, with a sub-second worker timeout configured. The Dispatcher
//!    transitions to Failed { stage: "WorkerRunning", reason: "timeout" }.
//! 3. **Verifier disagreement** — wraps the real Worker in an output
//!    rewriter that returns "999" instead of the calculated value, then
//!    re-signs the attestation. The Verifier flags answer_correct: false;
//!    the Dispatcher logs the disagreement and records Complete with the
//!    bits unchanged.
//!
//! Each run appends to `data/attestations.json` and `data/executions.json`.
//! Run multiple times to grow the logs.

use std::time::Duration;

use async_trait::async_trait;
use attestly_agents::{
    Dispatcher, DispatcherConfig, Verifier, Worker, WorkerAgent, WorkerError, WorkerTask,
};
use attestly_core::{AgentKeypair, Attestation, AttestationStatus, ExecutionStatus, TaskExecution};

/// Wraps another worker and sleeps before delegating, used to drive the
/// dispatcher's worker-stage timeout path.
struct DelayedWorker<W: WorkerAgent> {
    inner: W,
    delay: Duration,
}

#[async_trait]
impl<W: WorkerAgent + Send + Sync> WorkerAgent for DelayedWorker<W> {
    async fn run(&self, task: &WorkerTask) -> Result<Attestation, WorkerError> {
        tokio::time::sleep(self.delay).await;
        self.inner.run(task).await
    }
}

/// Wraps another worker, runs it, then forges a wrong-but-validly-signed
/// output. Used to drive the verifier-disagreement path.
struct WrongAnswerWorker {
    keypair: AgentKeypair,
    forced_output: String,
}

#[async_trait]
impl WorkerAgent for WrongAnswerWorker {
    async fn run(&self, task: &WorkerTask) -> Result<Attestation, WorkerError> {
        // Build a Completed attestation that *signs over* the wrong output —
        // the Worker's signature is intact, so attestation_valid will be true,
        // but answer_correct will be false because the verifier's independent
        // re-solve disagrees.
        let mut a = Attestation::new(
            "wrong-answer-worker",
            attestly_core::sha256(task.canonical_bytes()),
            self.forced_output.clone(),
            AttestationStatus::Completed,
            None,
            chrono::Utc::now().timestamp(),
        );
        self.keypair.sign_attestation(&mut a);
        Ok(a)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Real agents reused across tasks 1 + 3.
    let real_worker = Worker::new("worker-1");
    let verifier = Verifier::new("verifier-1");

    // -- Task 1: normal happy-path through the Dispatcher --
    {
        let dispatcher = Dispatcher::new(DispatcherConfig::standard());
        let exec = dispatcher
            .run(&real_worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await?;
        print_execution("Task 1 (normal)", &exec);
    }

    // -- Task 2: worker timeout. Wrap the real worker in a 2s sleep and
    //    configure a 200ms worker timeout.
    {
        let dispatcher = Dispatcher::new(DispatcherConfig::standard().with_timeouts(
            Duration::from_millis(200),
            attestly_agents::DEFAULT_STAGE_TIMEOUT,
        ));
        let slow_worker = DelayedWorker {
            inner: Worker::new("worker-1-slow"),
            delay: Duration::from_secs(2),
        };
        let exec = dispatcher
            .run(&slow_worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await?;
        print_execution("Task 2 (worker timeout)", &exec);
    }

    // -- Task 3: verifier disagreement. Worker signs over "999" but the
    //    verifier independently calculates 84.
    {
        let dispatcher = Dispatcher::new(DispatcherConfig::standard());
        let wrong_worker = WrongAnswerWorker {
            keypair: AgentKeypair::generate(),
            forced_output: "999".into(),
        };
        let exec = dispatcher
            .run(&wrong_worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await?;
        print_execution("Task 3 (verifier disagreement)", &exec);
    }

    println!();
    println!("All three executions persisted to data/executions.json");
    println!("Underlying attestations persisted to data/attestations.json");
    Ok(())
}

fn print_execution(label: &str, exec: &TaskExecution) {
    println!();
    println!("{label}:  task_id={}", exec.task_id);
    println!("  status: {}", format_status(&exec.status));
    if let Some(att) = &exec.worker_attestation {
        println!("  worker_attestation:   {}", att.id);
    }
    if let Some(att) = &exec.verifier_attestation {
        println!("  verifier_attestation: {}", att.id);
    }
}

fn format_status(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Pending => "Pending".into(),
        ExecutionStatus::WorkerRunning => "WorkerRunning".into(),
        ExecutionStatus::WorkerComplete => "WorkerComplete".into(),
        ExecutionStatus::VerifierRunning => "VerifierRunning".into(),
        ExecutionStatus::Complete {
            attestation_valid,
            answer_correct,
        } => format!(
            "Complete {{ attestation_valid: {attestation_valid}, answer_correct: {answer_correct} }}"
        ),
        ExecutionStatus::Failed { stage, reason } => {
            format!("Failed {{ stage: {stage:?}, reason: {reason:?} }}")
        }
    }
}
