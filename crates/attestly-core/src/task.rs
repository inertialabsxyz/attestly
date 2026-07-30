//! `TaskExecution` — the Dispatcher's record of a single Worker → Verifier
//! lifecycle.
//!
//! The struct mirrors the "Coordination State" subsection of
//! `planning/attestly-prototype-plan.md`. It lives in `attestly-core` (and not in
//! `attestly-agents`) so any consumer — the Dispatcher, an external dashboard,
//! a Phase 3 Batcher — can deserialize and reason about an execution
//! without depending on the agents crate. See PAIN_POINTS Phase 2 Q1.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::attestation::Attestation;

/// A single Dispatcher-coordinated task lifecycle, from Pending through
/// Complete (or Failed).
///
/// `worker_attestation` and `verifier_attestation` are populated as each
/// agent finishes. On disk we serialize the full attestations so a
/// `TaskExecution` is self-contained; on the wire the Dispatcher persists
/// only the attestation ids (see [`TaskExecutionRecord`] in `storage.rs`)
/// and rejoins them on load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskExecution {
    pub task_id: Uuid,
    pub task_input: String,
    pub status: ExecutionStatus,

    pub worker_attestation: Option<Attestation>,
    pub verifier_attestation: Option<Attestation>,

    pub started_at: i64,
    pub completed_at: Option<i64>,
}

/// Lifecycle states for a [`TaskExecution`].
///
/// The plan defines `Failed { stage, reason }` and `Complete { ... }` as
/// struct variants; the intermediate `Pending`/`WorkerRunning`/... unit
/// variants drive the Dispatcher state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    WorkerRunning,
    WorkerComplete,
    VerifierRunning,
    Complete {
        attestation_valid: bool,
        answer_correct: bool,
    },
    Failed {
        stage: String,
        reason: String,
    },
}

impl TaskExecution {
    /// Construct a fresh `TaskExecution` in the `Pending` state.
    pub fn new(task_input: impl Into<String>, started_at: i64) -> Self {
        Self {
            task_id: Uuid::new_v4(),
            task_input: task_input.into(),
            status: ExecutionStatus::Pending,
            worker_attestation: None,
            verifier_attestation: None,
            started_at,
            completed_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{Attestation, AttestationStatus};
    use crate::signing::{sha256, AgentKeypair};

    fn signed_completed(kp: &AgentKeypair, output: &str) -> Attestation {
        let mut a = Attestation::new(
            "agent",
            sha256(b"task"),
            output,
            AttestationStatus::Completed,
            None,
            1_700_000_000,
        );
        kp.sign_attestation(&mut a);
        a
    }

    #[test]
    fn new_starts_pending_with_no_attestations() {
        let te = TaskExecution::new("140 * 0.6", 1_700_000_000);
        assert_eq!(te.status, ExecutionStatus::Pending);
        assert!(te.worker_attestation.is_none());
        assert!(te.verifier_attestation.is_none());
        assert_eq!(te.completed_at, None);
        assert_eq!(te.task_input, "140 * 0.6");
    }

    #[test]
    fn complete_status_round_trips_through_serde() {
        let kp = AgentKeypair::generate();
        let mut te = TaskExecution::new("140 * 0.6", 1_700_000_000);
        te.worker_attestation = Some(signed_completed(&kp, "84"));
        te.verifier_attestation = Some(signed_completed(&kp, "ok"));
        te.status = ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        };
        te.completed_at = Some(1_700_000_002);

        let s = serde_json::to_string(&te).unwrap();
        let back: TaskExecution = serde_json::from_str(&s).unwrap();
        assert_eq!(te, back);
    }

    #[test]
    fn failed_status_round_trips_through_serde() {
        let mut te = TaskExecution::new("slow task", 1_700_000_000);
        te.status = ExecutionStatus::Failed {
            stage: "WorkerRunning".into(),
            reason: "timeout".into(),
        };
        te.completed_at = Some(1_700_000_030);
        let s = serde_json::to_string(&te).unwrap();
        let back: TaskExecution = serde_json::from_str(&s).unwrap();
        assert_eq!(te, back);
    }
}
