//! Dispatcher — owns the lifecycle of a single Worker → Verifier task.
//!
//! Drives the state machine described in `planning/awp-prototype-plan.md`
//! → "Coordination State":
//!
//!   Pending → WorkerRunning → WorkerComplete → VerifierRunning →
//!     Complete { attestation_valid, answer_correct }
//!
//! On agent error or stage timeout the dispatcher transitions to
//! `Failed { stage, reason }` and returns. Verifier disagreements
//! (`answer_correct: false` or `attestation_valid: false`) are logged
//! at `eprintln` level but do **not** halt or retry — disagreement is a
//! data signal, not a control-flow event.
//!
//! The Dispatcher persists each `TaskExecution` to `data/executions.json`
//! (id-only references) and the underlying attestations to
//! `data/attestations.json`. See `awp-core::execution` for the join
//! semantics.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use awp_core::{
    append_attestation, append_execution, AttestationError, AttestationStatus, ExecutionStatus,
    TaskExecution,
};
use thiserror::Error;
use tokio::time::error::Elapsed;

use crate::batcher::{Batcher, BatcherError};
use crate::verifier::VerifierAgent;
use crate::worker::{WorkerAgent, WorkerTask};

/// Default per-stage timeout from the plan's exit criteria
/// ("Timeout handling works (Worker takes >30s)").
pub const DEFAULT_STAGE_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for [`Dispatcher`]. All timeouts are per stage (Worker
/// stage and Verifier stage are bounded independently).
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// Wall-clock timeout for the Worker stage. Defaults to 30 seconds.
    pub worker_timeout: Duration,
    /// Wall-clock timeout for the Verifier stage. Defaults to 30 seconds.
    pub verifier_timeout: Duration,
    /// Path to the append-only attestations log.
    pub attestations_path: PathBuf,
    /// Path to the append-only executions log.
    pub executions_path: PathBuf,
}

impl DispatcherConfig {
    /// Standard configuration: 30-second per-stage timeout, JSON logs in
    /// the conventional `data/` directory.
    pub fn standard() -> Self {
        Self {
            worker_timeout: DEFAULT_STAGE_TIMEOUT,
            verifier_timeout: DEFAULT_STAGE_TIMEOUT,
            attestations_path: PathBuf::from("data/attestations.json"),
            executions_path: PathBuf::from("data/executions.json"),
        }
    }

    pub fn with_paths(
        mut self,
        attestations_path: impl Into<PathBuf>,
        executions_path: impl Into<PathBuf>,
    ) -> Self {
        self.attestations_path = attestations_path.into();
        self.executions_path = executions_path.into();
        self
    }

    pub fn with_timeouts(mut self, worker: Duration, verifier: Duration) -> Self {
        self.worker_timeout = worker;
        self.verifier_timeout = verifier;
        self
    }
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self::standard()
    }
}

/// Errors that escape the Dispatcher to its caller. Per-stage failures
/// (agent error, timeout, verifier disagreement) are recorded **inside**
/// the returned [`TaskExecution`] — only persistence/io errors surface
/// here, since they prevent the Dispatcher from durably recording what
/// happened.
#[derive(Debug, Error)]
pub enum DispatcherError {
    #[error("failed to persist attestation: {0}")]
    AttestationPersistence(AttestationError),
    #[error("failed to persist execution: {0}")]
    ExecutionPersistence(AttestationError),
    #[error("failed to submit attestation to batcher: {0}")]
    BatcherSubmit(BatcherError),
}

/// The Dispatcher. Holds config and a wall-clock source. Worker and
/// Verifier are passed to [`Dispatcher::run`] so a single Dispatcher can
/// drive heterogeneous agent pairs across runs.
///
/// In Phase 3 the Dispatcher optionally forwards every produced
/// attestation to a [`Batcher`] (set via [`Dispatcher::with_batcher`]).
/// When no batcher is attached the Phase 1/2 contract is preserved: only
/// the JSON logs are written. The Phase 1/2 example binaries
/// (`simple_attestation`, `dispatcher_flow`) keep working unchanged.
pub struct Dispatcher {
    config: DispatcherConfig,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    batcher: Option<Arc<Batcher>>,
}

impl Dispatcher {
    pub fn new(config: DispatcherConfig) -> Self {
        Self {
            config,
            clock: Box::new(default_clock),
            batcher: None,
        }
    }

    /// Override the wall-clock source — used by tests for deterministic
    /// `started_at`/`completed_at` values.
    pub fn with_clock<F>(mut self, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        self.clock = Box::new(clock);
        self
    }

    /// Attach a [`Batcher`] so that every Worker and Verifier
    /// attestation produced by [`Dispatcher::run`] is also forwarded to
    /// the batcher. Wrapped in `Arc` so the same batcher can be shared
    /// across multiple Dispatcher instances.
    pub fn with_batcher(mut self, batcher: Arc<Batcher>) -> Self {
        self.batcher = Some(batcher);
        self
    }

    pub fn config(&self) -> &DispatcherConfig {
        &self.config
    }

    /// Run a single task end-to-end, persisting all produced records.
    ///
    /// Returns the final [`TaskExecution`] regardless of outcome —
    /// including timeouts and verifier disagreements. The only `Err`
    /// path is unrecoverable persistence failure; in that case the
    /// in-memory execution is lost.
    pub async fn run<W, V>(
        &self,
        worker: &W,
        verifier: &V,
        task: WorkerTask,
    ) -> Result<TaskExecution, DispatcherError>
    where
        W: WorkerAgent + ?Sized,
        V: VerifierAgent + ?Sized,
    {
        let mut execution = TaskExecution::new(task.expression.clone(), (self.clock)());

        // ---- Worker stage ------------------------------------------------
        execution.status = ExecutionStatus::WorkerRunning;
        let worker_outcome =
            tokio::time::timeout(self.config.worker_timeout, worker.run(&task)).await;

        let worker_attestation = match worker_outcome {
            Ok(Ok(att)) => att,
            Ok(Err(e)) => {
                return self
                    .finalize_failure(execution, "WorkerRunning", e.to_string())
                    .await;
            }
            Err(Elapsed { .. }) => {
                return self
                    .finalize_failure(execution, "WorkerRunning", "timeout".into())
                    .await;
            }
        };
        if let Err(e) = append_attestation(&self.config.attestations_path, &worker_attestation) {
            return Err(DispatcherError::AttestationPersistence(e));
        }
        execution.worker_attestation = Some(worker_attestation.clone());
        execution.status = ExecutionStatus::WorkerComplete;

        // ---- Verifier stage ----------------------------------------------
        execution.status = ExecutionStatus::VerifierRunning;
        let verifier_outcome = tokio::time::timeout(
            self.config.verifier_timeout,
            verifier.run(&task, &worker_attestation),
        )
        .await;

        let verifier_attestation = match verifier_outcome {
            Ok(Ok(att)) => att,
            Ok(Err(e)) => {
                return self
                    .finalize_failure(execution, "VerifierRunning", e.to_string())
                    .await;
            }
            Err(Elapsed { .. }) => {
                return self
                    .finalize_failure(execution, "VerifierRunning", "timeout".into())
                    .await;
            }
        };
        if let Err(e) = append_attestation(&self.config.attestations_path, &verifier_attestation) {
            return Err(DispatcherError::AttestationPersistence(e));
        }
        execution.verifier_attestation = Some(verifier_attestation.clone());

        // ---- Reasoning about the verifier outcome ------------------------
        let (attestation_valid, answer_correct) = match &verifier_attestation.status {
            AttestationStatus::Verified {
                attestation_valid,
                answer_correct,
            } => (*attestation_valid, *answer_correct),
            // The Verifier emitted something other than a Verified status —
            // treat as a failure of the Verifier stage.
            other => {
                return self
                    .finalize_failure(
                        execution,
                        "VerifierRunning",
                        format!("verifier returned non-Verified status: {other:?}"),
                    )
                    .await;
            }
        };

        if !attestation_valid || !answer_correct {
            // Disagreement is a data signal, not a control-flow event.
            // Logged so an operator can grep `dispatcher: verifier
            // disagreement` to find them.
            eprintln!(
                "dispatcher: verifier disagreement task_id={} worker_attestation={} attestation_valid={} answer_correct={}",
                execution.task_id,
                worker_attestation.id,
                attestation_valid,
                answer_correct,
            );
        }

        execution.status = ExecutionStatus::Complete {
            attestation_valid,
            answer_correct,
        };
        execution.completed_at = Some((self.clock)());

        if let Err(e) = append_execution(&self.config.executions_path, &execution) {
            return Err(DispatcherError::ExecutionPersistence(e));
        }

        // Phase 3 wiring: once both attestations are durably on disk and
        // the execution record is saved, hand them to the Batcher (if
        // configured) so they enter the next Merkle batch.
        if let Some(batcher) = &self.batcher {
            batcher
                .submit(worker_attestation)
                .await
                .map_err(DispatcherError::BatcherSubmit)?;
            batcher
                .submit(verifier_attestation)
                .await
                .map_err(DispatcherError::BatcherSubmit)?;
        }

        Ok(execution)
    }

    async fn finalize_failure(
        &self,
        mut execution: TaskExecution,
        stage: &str,
        reason: String,
    ) -> Result<TaskExecution, DispatcherError> {
        execution.status = ExecutionStatus::Failed {
            stage: stage.into(),
            reason,
        };
        execution.completed_at = Some((self.clock)());
        if let Err(e) = append_execution(&self.config.executions_path, &execution) {
            return Err(DispatcherError::ExecutionPersistence(e));
        }
        Ok(execution)
    }
}

fn default_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verifier::{Verifier, VerifierAgent, VerifierError};
    use crate::worker::{Worker, WorkerAgent, WorkerError};
    use async_trait::async_trait;
    use awp_core::{load_executions, sha256, AgentKeypair, Attestation};
    use tempfile::tempdir;

    fn dispatcher_for(dir: &std::path::Path, w: Duration, v: Duration) -> Dispatcher {
        Dispatcher::new(
            DispatcherConfig::standard()
                .with_paths(dir.join("attestations.json"), dir.join("executions.json"))
                .with_timeouts(w, v),
        )
        .with_clock(|| 1_700_000_000)
    }

    #[tokio::test]
    async fn happy_path_completes_with_both_attestations_persisted() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path(), Duration::from_secs(5), Duration::from_secs(5));
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);

        let exec = dispatcher
            .run(&worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await
            .unwrap();

        assert_eq!(
            exec.status,
            ExecutionStatus::Complete {
                attestation_valid: true,
                answer_correct: true,
            }
        );
        let worker_att = exec
            .worker_attestation
            .as_ref()
            .expect("worker attestation set");
        let verifier_att = exec
            .verifier_attestation
            .as_ref()
            .expect("verifier attestation set");
        assert_eq!(verifier_att.references, Some(worker_att.id));
        assert!(exec.completed_at.is_some());

        let on_disk = load_executions(
            &dispatcher.config.executions_path,
            &dispatcher.config.attestations_path,
        )
        .unwrap();
        assert_eq!(on_disk.len(), 1);
        assert_eq!(on_disk[0], exec);
    }

    /// Slow worker that ignores the task and just sleeps — used to exercise
    /// the worker-stage timeout path.
    struct SleepyWorker {
        delay: Duration,
    }
    #[async_trait]
    impl WorkerAgent for SleepyWorker {
        async fn run(&self, _task: &WorkerTask) -> Result<Attestation, WorkerError> {
            tokio::time::sleep(self.delay).await;
            // Build a minimal completed attestation (we never reach this in
            // the timeout test, but the trait requires returning one).
            let kp = AgentKeypair::generate();
            let mut a = Attestation::new(
                "sleepy",
                sha256(b"x"),
                "0",
                AttestationStatus::Completed,
                None,
                0,
            );
            kp.sign_attestation(&mut a);
            Ok(a)
        }
    }

    #[tokio::test]
    async fn worker_timeout_records_failed_at_worker_stage() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(
            dir.path(),
            Duration::from_millis(50),
            Duration::from_secs(5),
        );
        let worker = SleepyWorker {
            delay: Duration::from_secs(2),
        };
        let verifier = Verifier::new("verifier-1");

        let exec = dispatcher
            .run(&worker, &verifier, WorkerTask::new("anything"))
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "WorkerRunning");
                assert_eq!(reason, "timeout");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(exec.worker_attestation.is_none());
        assert!(exec.verifier_attestation.is_none());
        assert!(exec.completed_at.is_some());
    }

    /// Worker that signs a deliberately wrong output, used to drive the
    /// verifier-disagreement path through the dispatcher.
    struct WrongAnswerWorker {
        keypair: AgentKeypair,
    }
    #[async_trait]
    impl WorkerAgent for WrongAnswerWorker {
        async fn run(&self, task: &WorkerTask) -> Result<Attestation, WorkerError> {
            let mut a = Attestation::new(
                "wrong-worker",
                sha256(task.canonical_bytes()),
                "999",
                AttestationStatus::Completed,
                None,
                1_700_000_000,
            );
            self.keypair.sign_attestation(&mut a);
            Ok(a)
        }
    }

    #[tokio::test]
    async fn verifier_disagreement_yields_complete_with_false_flags() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path(), Duration::from_secs(5), Duration::from_secs(5));
        let worker = WrongAnswerWorker {
            keypair: AgentKeypair::generate(),
        };
        let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);

        let exec = dispatcher
            .run(&worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Complete {
                attestation_valid,
                answer_correct,
            } => {
                assert!(attestation_valid, "the Worker's signature is intact");
                assert!(!answer_correct, "999 != 84");
            }
            other => panic!("expected Complete (with disagreement), got {other:?}"),
        }
        // Both attestations produced; disagreement does not abort.
        assert!(exec.worker_attestation.is_some());
        assert!(exec.verifier_attestation.is_some());
    }

    /// Worker that always returns an error — exercises the
    /// non-timeout failure path.
    struct ErroringWorker;
    #[async_trait]
    impl WorkerAgent for ErroringWorker {
        async fn run(&self, _task: &WorkerTask) -> Result<Attestation, WorkerError> {
            Err(WorkerError::Tool(crate::tools::ToolError::Calc(
                "boom".into(),
            )))
        }
    }

    #[tokio::test]
    async fn worker_error_records_failed_at_worker_stage() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path(), Duration::from_secs(5), Duration::from_secs(5));
        let verifier = Verifier::new("verifier-1");

        let exec = dispatcher
            .run(&ErroringWorker, &verifier, WorkerTask::new("x"))
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "WorkerRunning");
                assert!(reason.contains("boom"), "reason should propagate: {reason}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// Slow verifier — used to exercise the verifier-stage timeout path.
    struct SleepyVerifier {
        delay: Duration,
    }
    #[async_trait]
    impl VerifierAgent for SleepyVerifier {
        async fn run(
            &self,
            _task: &WorkerTask,
            _worker: &Attestation,
        ) -> Result<Attestation, VerifierError> {
            tokio::time::sleep(self.delay).await;
            unreachable!("test should time out before this resolves");
        }
    }

    #[tokio::test]
    async fn verifier_timeout_records_failed_at_verifier_stage() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(
            dir.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let verifier = SleepyVerifier {
            delay: Duration::from_secs(2),
        };

        let exec = dispatcher
            .run(&worker, &verifier, WorkerTask::new("140 * 0.6"))
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "VerifierRunning");
                assert_eq!(reason, "timeout");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        // Worker attestation was produced and persisted before the verifier
        // stage ran out the clock.
        assert!(exec.worker_attestation.is_some());
        assert!(exec.verifier_attestation.is_none());
    }
}
