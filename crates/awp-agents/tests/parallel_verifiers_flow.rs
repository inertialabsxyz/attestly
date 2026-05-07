//! Phase 4 stretch-task integration test: drive a task through the
//! `ParallelDispatcher` with multiple Verifiers running concurrently, and
//! assert that every attestation is on disk and the
//! `TaskExecution`-shaped record persisted to the executions log keeps
//! the Phase 2 reader working.

use std::time::Duration;

use awp_agents::{
    DispatcherConfig, ParallelDispatcher, Verifier, VerifierAgent, Worker, WorkerTask,
};
use awp_core::{load_attestations, load_executions, verify_attestation_signature, ExecutionStatus};
use tempfile::tempdir;

#[tokio::test]
async fn parallel_dispatcher_drives_three_verifiers_to_complete() {
    let dir = tempdir().unwrap();
    let attestations_path = dir.path().join("attestations.json");
    let executions_path = dir.path().join("executions.json");

    let dispatcher = ParallelDispatcher::new(
        DispatcherConfig::standard()
            .with_paths(&attestations_path, &executions_path)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5)),
    )
    .with_clock(|| 1_700_000_000);

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
        .expect("happy-path parallel dispatch should not error");

    // ---- In-memory contract ---------------------------------------------
    assert!(!exec.disagreement);
    assert_eq!(
        exec.status,
        ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        }
    );
    assert_eq!(exec.verifier_attestations.len(), 3);

    let worker_att = exec
        .worker_attestation
        .as_ref()
        .expect("worker attestation");
    assert_eq!(worker_att.output, "84");
    assert!(verify_attestation_signature(worker_att));
    for v in &exec.verifier_attestations {
        assert_eq!(v.references, Some(worker_att.id));
        assert!(verify_attestation_signature(v));
    }

    // ---- Persistence contract -------------------------------------------
    // All four attestations (1 worker + 3 verifier) should be on disk.
    let on_disk_atts = load_attestations(&attestations_path).unwrap();
    assert_eq!(on_disk_atts.len(), 4);

    // The executions log holds a single TaskExecution-shaped record.
    // Phase 2's reader keeps working because we persist that shape with
    // the first verifier as the "primary".
    let on_disk_execs = load_executions(&executions_path, &attestations_path).unwrap();
    assert_eq!(on_disk_execs.len(), 1);
    assert_eq!(
        on_disk_execs[0].status,
        ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        }
    );
    assert!(on_disk_execs[0].verifier_attestation.is_some());
}
