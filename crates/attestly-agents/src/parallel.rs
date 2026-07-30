//! Parallel-verifier dispatcher — Phase 4 stretch task (Option B).
//!
//! Runs one Worker followed by **N Verifiers concurrently** against the
//! same Worker attestation, persists every produced attestation, and
//! flags any disagreement among the verifiers.
//!
//! ## Why this lives next to `Dispatcher` rather than replacing it
//!
//! The Phase 2 single-verifier `Dispatcher` is the canonical lifecycle
//! and the planning doc's "Coordination State" type
//! (`TaskExecution`/`ExecutionStatus`) is hard-coded to a single
//! verifier attestation. The Phase 4 brief is explicit:
//! "The Phase 1/2/3 code beyond the stretch task scope — this is an
//! evaluation phase, not a refactor." So the parallel path uses its own
//! result type [`ParallelExecution`] and leaves [`Dispatcher::run`]
//! unchanged.
//!
//! ## What disagreement means here
//!
//! A *disagreement* is any verifier whose `(attestation_valid,
//! answer_correct)` pair differs from any other verifier's pair. We
//! detect it by collecting the unique pairs across all verifier
//! attestations and flagging when more than one distinct pair appears.
//! The dispatcher logs a `parallel-dispatcher: verifier disagreement`
//! line (matching the Phase 2 convention) but does not halt — exactly
//! like the single-verifier path.
//!
//! ## What this surfaces about the framework
//!
//! With trait-object verifiers and `futures::future::try_join_all`, the
//! parallel run is mechanically straightforward. There is no actor, no
//! supervisor, and no message-passing required. See
//! `docs/PAIN_POINTS.md` Phase 4 → "Parallel Verifiers (stretch task)"
//! for what this implies for the framework comparison.

use std::sync::Arc;
use std::time::Duration;

use attestly_core::{
    append_attestation, append_execution, Attestation, AttestationError, AttestationStatus,
    ExecutionStatus, TaskExecution,
};
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::error::Elapsed;
use uuid::Uuid;

use crate::batcher::{Batcher, BatcherError};
use crate::dispatcher::DispatcherConfig;
use crate::verifier::VerifierAgent;
use crate::worker::{WorkerAgent, WorkerTask};

/// Errors raised by [`ParallelDispatcher::run`]. Per-stage failures
/// (worker error, worker timeout, any verifier error or timeout) are
/// recorded **inside** the returned [`ParallelExecution`]; only
/// persistence failures surface here.
#[derive(Debug, Error)]
pub enum ParallelDispatcherError {
    #[error("failed to persist attestation: {0}")]
    AttestationPersistence(AttestationError),
    #[error("failed to persist execution: {0}")]
    ExecutionPersistence(AttestationError),
    #[error("failed to submit attestation to batcher: {0}")]
    BatcherSubmit(BatcherError),
}

/// The result of a parallel-verifier run.
///
/// Mirrors [`TaskExecution`] for the worker side but holds a `Vec` of
/// verifier attestations rather than a single `Option`. The
/// `disagreement` flag is true iff at least two verifiers reported
/// different `(attestation_valid, answer_correct)` pairs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelExecution {
    pub task_id: Uuid,
    pub task_input: String,
    pub status: ExecutionStatus,
    pub worker_attestation: Option<Attestation>,
    pub verifier_attestations: Vec<Attestation>,
    /// True iff verifiers disagreed about the worker's attestation.
    /// Only meaningful when `status == ExecutionStatus::Complete`.
    pub disagreement: bool,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

impl ParallelExecution {
    fn new(task_input: impl Into<String>, started_at: i64) -> Self {
        Self {
            task_id: Uuid::new_v4(),
            task_input: task_input.into(),
            status: ExecutionStatus::Pending,
            worker_attestation: None,
            verifier_attestations: Vec::new(),
            disagreement: false,
            started_at,
            completed_at: None,
        }
    }

    /// Convert into a single-verifier [`TaskExecution`] for persistence
    /// to `data/executions.json`. The first verifier attestation is
    /// chosen as the "primary"; downstream readers wanting the full set
    /// of verifier attestations should read them by id from
    /// `data/attestations.json`.
    fn as_task_execution(&self) -> TaskExecution {
        TaskExecution {
            task_id: self.task_id,
            task_input: self.task_input.clone(),
            status: self.status.clone(),
            worker_attestation: self.worker_attestation.clone(),
            verifier_attestation: self.verifier_attestations.first().cloned(),
            started_at: self.started_at,
            completed_at: self.completed_at,
        }
    }
}

/// Dispatcher that runs N verifiers concurrently against one Worker.
///
/// Reuses [`DispatcherConfig`] for timeouts and persistence paths so
/// callers can swap between the single- and parallel-verifier paths
/// without re-configuring.
pub struct ParallelDispatcher {
    config: DispatcherConfig,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    batcher: Option<Arc<Batcher>>,
}

impl ParallelDispatcher {
    pub fn new(config: DispatcherConfig) -> Self {
        Self {
            config,
            clock: Box::new(default_clock),
            batcher: None,
        }
    }

    pub fn with_clock<F>(mut self, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        self.clock = Box::new(clock);
        self
    }

    pub fn with_batcher(mut self, batcher: Arc<Batcher>) -> Self {
        self.batcher = Some(batcher);
        self
    }

    pub fn config(&self) -> &DispatcherConfig {
        &self.config
    }

    /// Run a single task with one Worker and `verifiers.len()` parallel
    /// Verifiers. Every produced attestation is persisted; the
    /// resulting [`ParallelExecution`] also writes a
    /// [`TaskExecution`]-shaped record to the executions log so
    /// existing readers (Phase 2 contract) keep working.
    ///
    /// Returns `Err` only on unrecoverable persistence/io failure.
    pub async fn run<W, V>(
        &self,
        worker: &W,
        verifiers: &[&V],
        task: WorkerTask,
    ) -> Result<ParallelExecution, ParallelDispatcherError>
    where
        W: WorkerAgent + ?Sized,
        V: VerifierAgent + ?Sized,
    {
        let mut execution = ParallelExecution::new(task.expression.clone(), (self.clock)());

        if verifiers.is_empty() {
            return self
                .finalize_failure(
                    execution,
                    "VerifierRunning",
                    "no verifiers configured".into(),
                )
                .await;
        }

        // ---- Worker stage (sequential) ------------------------------------
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
            return Err(ParallelDispatcherError::AttestationPersistence(e));
        }
        execution.worker_attestation = Some(worker_attestation.clone());
        execution.status = ExecutionStatus::WorkerComplete;

        // ---- Verifier stage (parallel) ------------------------------------
        execution.status = ExecutionStatus::VerifierRunning;
        let verifier_timeout = self.config.verifier_timeout;
        let verifier_futures =
            verifiers.iter().map(|v| {
                let task_ref = &task;
                let worker_ref = &worker_attestation;
                async move {
                    run_verifier_with_timeout(*v, task_ref, worker_ref, verifier_timeout).await
                }
            });

        // try_join_all short-circuits on the first error and aborts the
        // remaining futures' polling. That matches our policy: any single
        // verifier failure (error or timeout) marks the whole parallel
        // run as Failed in the Verifier stage.
        let verifier_attestations: Vec<Attestation> = match try_join_all(verifier_futures).await {
            Ok(atts) => atts,
            Err(VerifierStageError::Agent(reason)) => {
                return self
                    .finalize_failure(execution, "VerifierRunning", reason)
                    .await;
            }
            Err(VerifierStageError::Timeout) => {
                return self
                    .finalize_failure(execution, "VerifierRunning", "timeout".into())
                    .await;
            }
        };

        for att in &verifier_attestations {
            if let Err(e) = append_attestation(&self.config.attestations_path, att) {
                return Err(ParallelDispatcherError::AttestationPersistence(e));
            }
        }
        execution.verifier_attestations = verifier_attestations.clone();

        // ---- Reasoning about the verifiers' verdicts ---------------------
        // Extract (attestation_valid, answer_correct) for each verifier.
        // A verifier returning a non-Verified status is treated as a
        // stage failure (matches the Phase 2 single-verifier semantics).
        let mut verdicts: Vec<(bool, bool)> = Vec::with_capacity(verifier_attestations.len());
        for v in &verifier_attestations {
            match &v.status {
                AttestationStatus::Verified {
                    attestation_valid,
                    answer_correct,
                } => verdicts.push((*attestation_valid, *answer_correct)),
                other => {
                    return self
                        .finalize_failure(
                            execution,
                            "VerifierRunning",
                            format!("verifier returned non-Verified status: {other:?}"),
                        )
                        .await;
                }
            }
        }

        // Disagreement: more than one distinct verdict tuple across the set.
        let first = verdicts[0];
        let disagreement = verdicts.iter().any(|v| v != &first);

        // The "consensus" verdict reported in `ExecutionStatus::Complete`
        // is the *unanimous* judgement when there's no disagreement.
        // When verifiers disagree, we report `attestation_valid=false`
        // and `answer_correct=false` — disagreement itself is a failure
        // mode of the verification process even if a majority would have
        // said "valid". The full per-verifier verdicts remain in
        // `verifier_attestations` for downstream readers that want to
        // run majority logic.
        let (attestation_valid, answer_correct) = if disagreement { (false, false) } else { first };

        if disagreement {
            // Per Phase 2 convention: log, do not halt. Operators grep
            // for this line.
            eprintln!(
                "parallel-dispatcher: verifier disagreement task_id={} worker_attestation={} verdicts={:?}",
                execution.task_id, worker_attestation.id, verdicts,
            );
        } else if !attestation_valid || !answer_correct {
            eprintln!(
                "parallel-dispatcher: unanimous failed verdict task_id={} worker_attestation={} attestation_valid={} answer_correct={}",
                execution.task_id, worker_attestation.id, attestation_valid, answer_correct,
            );
        }

        execution.status = ExecutionStatus::Complete {
            attestation_valid,
            answer_correct,
        };
        execution.disagreement = disagreement;
        execution.completed_at = Some((self.clock)());

        // Persist a single-verifier-shaped record to the executions log
        // for compatibility with the Phase 2 reader.
        if let Err(e) =
            append_execution(&self.config.executions_path, &execution.as_task_execution())
        {
            return Err(ParallelDispatcherError::ExecutionPersistence(e));
        }

        // Phase 3 wiring: forward worker + every verifier attestation to
        // the optional batcher.
        if let Some(batcher) = &self.batcher {
            batcher
                .submit(worker_attestation)
                .await
                .map_err(ParallelDispatcherError::BatcherSubmit)?;
            for att in verifier_attestations {
                batcher
                    .submit(att)
                    .await
                    .map_err(ParallelDispatcherError::BatcherSubmit)?;
            }
        }

        Ok(execution)
    }

    async fn finalize_failure(
        &self,
        mut execution: ParallelExecution,
        stage: &str,
        reason: String,
    ) -> Result<ParallelExecution, ParallelDispatcherError> {
        execution.status = ExecutionStatus::Failed {
            stage: stage.into(),
            reason,
        };
        execution.completed_at = Some((self.clock)());
        if let Err(e) =
            append_execution(&self.config.executions_path, &execution.as_task_execution())
        {
            return Err(ParallelDispatcherError::ExecutionPersistence(e));
        }
        Ok(execution)
    }
}

/// Internal error type fed into `try_join_all` so the first verifier
/// failure short-circuits the parallel set with an actionable reason.
enum VerifierStageError {
    Agent(String),
    Timeout,
}

async fn run_verifier_with_timeout<V>(
    verifier: &V,
    task: &WorkerTask,
    worker_attestation: &Attestation,
    timeout: Duration,
) -> Result<Attestation, VerifierStageError>
where
    V: VerifierAgent + ?Sized,
{
    match tokio::time::timeout(timeout, verifier.run(task, worker_attestation)).await {
        Ok(Ok(att)) => Ok(att),
        Ok(Err(e)) => Err(VerifierStageError::Agent(e.to_string())),
        Err(Elapsed { .. }) => Err(VerifierStageError::Timeout),
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
    use attestly_core::{sha256, AgentKeypair, Attestation, AttestationStatus};
    use tempfile::tempdir;

    fn dispatcher_for(dir: &std::path::Path) -> ParallelDispatcher {
        ParallelDispatcher::new(
            DispatcherConfig::standard()
                .with_paths(dir.join("attestations.json"), dir.join("executions.json"))
                .with_timeouts(Duration::from_secs(5), Duration::from_secs(5)),
        )
        .with_clock(|| 1_700_000_000)
    }

    #[tokio::test]
    async fn three_verifiers_unanimous_agree_with_correct_worker() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let v1 = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);
        let v2 = Verifier::new("verifier-2").with_clock(|| 1_700_000_002);
        let v3 = Verifier::new("verifier-3").with_clock(|| 1_700_000_003);

        let exec = dispatcher
            .run(
                &worker,
                &[
                    &v1 as &dyn VerifierAgent,
                    &v2 as &dyn VerifierAgent,
                    &v3 as &dyn VerifierAgent,
                ],
                WorkerTask::new("140 * 0.6"),
            )
            .await
            .unwrap();

        assert!(!exec.disagreement, "all three verifiers should agree");
        assert_eq!(
            exec.status,
            ExecutionStatus::Complete {
                attestation_valid: true,
                answer_correct: true,
            }
        );
        assert_eq!(exec.verifier_attestations.len(), 3);
        // Each verifier attestation references the same worker attestation.
        let worker_id = exec.worker_attestation.as_ref().unwrap().id;
        for v_att in &exec.verifier_attestations {
            assert_eq!(v_att.references, Some(worker_id));
            assert!(matches!(
                v_att.status,
                AttestationStatus::Verified {
                    attestation_valid: true,
                    answer_correct: true,
                }
            ));
        }
    }

    /// A scripted verifier that emits a Verified attestation with whatever
    /// `(attestation_valid, answer_correct)` pair we want — used to
    /// induce disagreement deterministically.
    struct ScriptedVerifier {
        agent_id: String,
        keypair: AgentKeypair,
        attestation_valid: bool,
        answer_correct: bool,
    }
    impl ScriptedVerifier {
        fn new(agent_id: &str, attestation_valid: bool, answer_correct: bool) -> Self {
            Self {
                agent_id: agent_id.to_string(),
                keypair: AgentKeypair::generate(),
                attestation_valid,
                answer_correct,
            }
        }
    }
    #[async_trait]
    impl VerifierAgent for ScriptedVerifier {
        async fn run(
            &self,
            task: &WorkerTask,
            worker: &Attestation,
        ) -> Result<Attestation, VerifierError> {
            let mut a = Attestation::new(
                &self.agent_id,
                sha256(task.canonical_bytes()),
                format!(
                    "scripted av={} ac={}",
                    self.attestation_valid, self.answer_correct
                ),
                AttestationStatus::Verified {
                    attestation_valid: self.attestation_valid,
                    answer_correct: self.answer_correct,
                },
                Some(worker.id),
                1_700_000_001,
            );
            self.keypair.sign_attestation(&mut a);
            Ok(a)
        }
    }

    #[tokio::test]
    async fn disagreement_is_flagged_when_verifiers_split() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        // Two verifiers say "all good", one says "answer wrong".
        let v_yes = ScriptedVerifier::new("ok-1", true, true);
        let v_yes2 = ScriptedVerifier::new("ok-2", true, true);
        let v_no = ScriptedVerifier::new("dissent", true, false);

        let exec = dispatcher
            .run(
                &worker,
                &[
                    &v_yes as &dyn VerifierAgent,
                    &v_yes2 as &dyn VerifierAgent,
                    &v_no as &dyn VerifierAgent,
                ],
                WorkerTask::new("140 * 0.6"),
            )
            .await
            .unwrap();

        assert!(exec.disagreement, "verifiers split → disagreement flag set");
        // Disagreement reports as Complete{false,false} per the policy
        // documented above; the per-verifier verdicts are preserved in
        // `verifier_attestations`.
        assert_eq!(
            exec.status,
            ExecutionStatus::Complete {
                attestation_valid: false,
                answer_correct: false,
            }
        );
        assert_eq!(exec.verifier_attestations.len(), 3);
        // The dissenting verdict is still recoverable from the
        // attestation list.
        let dissent = exec
            .verifier_attestations
            .iter()
            .find(|a| a.agent_id == "dissent")
            .expect("dissenting verifier persisted");
        assert!(matches!(
            dissent.status,
            AttestationStatus::Verified {
                attestation_valid: true,
                answer_correct: false,
            }
        ));
    }

    /// Verifier that takes a measurable amount of wall-clock time —
    /// used to confirm that running N of them concurrently is faster
    /// than running them sequentially. This is the property a future
    /// actor-model swap would need to preserve.
    struct SlowVerifier {
        agent_id: String,
        keypair: AgentKeypair,
        delay: Duration,
    }
    #[async_trait]
    impl VerifierAgent for SlowVerifier {
        async fn run(
            &self,
            task: &WorkerTask,
            worker: &Attestation,
        ) -> Result<Attestation, VerifierError> {
            tokio::time::sleep(self.delay).await;
            let mut a = Attestation::new(
                &self.agent_id,
                sha256(task.canonical_bytes()),
                "slow ok".to_string(),
                AttestationStatus::Verified {
                    attestation_valid: true,
                    answer_correct: true,
                },
                Some(worker.id),
                1_700_000_001,
            );
            self.keypair.sign_attestation(&mut a);
            Ok(a)
        }
    }

    #[tokio::test]
    async fn parallel_execution_is_concurrent_not_sequential() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let delay = Duration::from_millis(200);
        let s1 = SlowVerifier {
            agent_id: "slow-1".into(),
            keypair: AgentKeypair::generate(),
            delay,
        };
        let s2 = SlowVerifier {
            agent_id: "slow-2".into(),
            keypair: AgentKeypair::generate(),
            delay,
        };
        let s3 = SlowVerifier {
            agent_id: "slow-3".into(),
            keypair: AgentKeypair::generate(),
            delay,
        };

        let started = std::time::Instant::now();
        let exec = dispatcher
            .run(
                &worker,
                &[
                    &s1 as &dyn VerifierAgent,
                    &s2 as &dyn VerifierAgent,
                    &s3 as &dyn VerifierAgent,
                ],
                WorkerTask::new("140 * 0.6"),
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();

        // 3 verifiers × 200ms sequential = 600ms. Parallel must finish
        // in well under 2× one verifier's delay (allowing slack for
        // worker work and persistence).
        assert!(
            elapsed < Duration::from_millis(450),
            "parallel run took {elapsed:?}, expected < 450ms"
        );
        assert!(!exec.disagreement);
        assert_eq!(exec.verifier_attestations.len(), 3);
    }

    /// Verifier that always errors — used to confirm that one bad
    /// verifier fails the whole stage.
    struct ErroringVerifier;
    #[async_trait]
    impl VerifierAgent for ErroringVerifier {
        async fn run(
            &self,
            _task: &WorkerTask,
            _worker: &Attestation,
        ) -> Result<Attestation, VerifierError> {
            Err(VerifierError::Tool(crate::tools::ToolError::Calc(
                "scripted failure".into(),
            )))
        }
    }

    #[tokio::test]
    async fn one_verifier_error_fails_the_whole_stage() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let v_ok = Verifier::new("v-ok").with_clock(|| 1_700_000_001);

        let exec = dispatcher
            .run(
                &worker,
                &[
                    &v_ok as &dyn VerifierAgent,
                    &ErroringVerifier as &dyn VerifierAgent,
                ],
                WorkerTask::new("140 * 0.6"),
            )
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "VerifierRunning");
                assert!(
                    reason.contains("scripted failure"),
                    "reason should propagate: {reason}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_verifier_set_is_a_failure() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);

        let exec = dispatcher
            .run::<_, Verifier>(&worker, &[], WorkerTask::new("140 * 0.6"))
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "VerifierRunning");
                assert!(reason.contains("no verifiers configured"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(exec.worker_attestation.is_none());
    }

    /// Worker that errors — confirms worker failure short-circuits
    /// before any verifier runs.
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
    async fn worker_failure_short_circuits_verifier_stage() {
        let dir = tempdir().unwrap();
        let dispatcher = dispatcher_for(dir.path());
        let v1 = Verifier::new("v-1").with_clock(|| 1_700_000_001);

        let exec = dispatcher
            .run(
                &ErroringWorker,
                &[&v1 as &dyn VerifierAgent],
                WorkerTask::new("anything"),
            )
            .await
            .unwrap();

        match exec.status {
            ExecutionStatus::Failed { stage, reason } => {
                assert_eq!(stage, "WorkerRunning");
                assert!(reason.contains("boom"), "reason should propagate: {reason}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(exec.verifier_attestations.is_empty());
    }
}
