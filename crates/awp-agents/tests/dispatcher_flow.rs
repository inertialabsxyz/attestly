//! Phase 2 integration test: drive a successful task end-to-end through
//! the Dispatcher and assert the TaskExecution reaches Complete with both
//! attestations linked and on disk.
//!
//! Mirrors the Phase 2 exit criteria:
//! - Dispatcher receives task, routes to Worker
//! - Dispatcher waits for Worker, triggers Verifier
//! - Both attestations collected and linked
//! - Full execution persisted as TaskExecution

use std::time::Duration;

use awp_agents::{Dispatcher, DispatcherConfig, Verifier, Worker, WorkerTask};
use awp_core::{
    load_execution, load_executions, verify_attestation_signature, ExecutionStatus, TaskExecution,
};
use tempfile::tempdir;

#[tokio::test]
async fn dispatcher_drives_worker_verifier_to_complete() {
    let dir = tempdir().unwrap();
    let attestations_path = dir.path().join("attestations.json");
    let executions_path = dir.path().join("executions.json");

    let dispatcher = Dispatcher::new(
        DispatcherConfig::standard()
            .with_paths(&attestations_path, &executions_path)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5)),
    )
    .with_clock(|| 1_700_000_000);

    let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
    let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);

    let exec = dispatcher
        .run(&worker, &verifier, WorkerTask::new("140 * 0.6"))
        .await
        .expect("happy-path dispatch should not error");

    // ---- In-memory contract -----------------------------------------------
    assert_eq!(
        exec.status,
        ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        }
    );
    assert!(exec.completed_at.is_some());

    let worker_att = exec.worker_attestation.as_ref().expect("worker attestation");
    let verifier_att = exec
        .verifier_attestation
        .as_ref()
        .expect("verifier attestation");
    assert_eq!(worker_att.output, "84");
    assert_eq!(verifier_att.references, Some(worker_att.id));
    assert!(verify_attestation_signature(worker_att));
    assert!(verify_attestation_signature(verifier_att));

    // ---- Persistence contract --------------------------------------------
    // load_executions joins both files; the rejoined record must equal the
    // in-memory execution exactly.
    let on_disk = load_executions(&executions_path, &attestations_path).unwrap();
    assert_eq!(on_disk.len(), 1, "exactly one execution on disk");
    assert_eq!(on_disk[0], exec);

    // load_execution by id finds the same record.
    let by_id: TaskExecution = load_execution(&executions_path, &attestations_path, exec.task_id)
        .unwrap()
        .expect("execution should exist");
    assert_eq!(by_id, exec);
}

#[tokio::test]
async fn dispatcher_persists_multiple_executions_in_order() {
    let dir = tempdir().unwrap();
    let attestations_path = dir.path().join("attestations.json");
    let executions_path = dir.path().join("executions.json");

    let dispatcher = Dispatcher::new(
        DispatcherConfig::standard().with_paths(&attestations_path, &executions_path),
    )
    .with_clock(|| 1_700_000_000);

    let worker = Worker::new("worker-1").with_clock(|| 1_700_000_000);
    let verifier = Verifier::new("verifier-1").with_clock(|| 1_700_000_001);

    let mut produced = Vec::new();
    for expr in ["1 + 2", "3 * 4", "(5 - 1) / 2"] {
        let exec = dispatcher
            .run(&worker, &verifier, WorkerTask::new(expr))
            .await
            .unwrap();
        produced.push(exec);
    }

    let on_disk = load_executions(&executions_path, &attestations_path).unwrap();
    assert_eq!(on_disk.len(), 3);
    for (got, want) in on_disk.iter().zip(produced.iter()) {
        assert_eq!(got, want);
    }
}
