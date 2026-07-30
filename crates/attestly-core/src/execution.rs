//! Append-only JSON storage for [`TaskExecution`] records.
//!
//! Mirrors the pattern in `attestation.rs`: one JSON-encoded record per
//! line, append-only, no in-place updates. `TaskExecution`s reference
//! their `Attestation`s by id — the attestations themselves live in
//! `attestations.json`. [`load_execution`] joins the two files.
//!
//! Phase 3 will collapse this and `attestations.json` into a single SQLite
//! database; until then, two side-by-side JSONL files keep the Phase 1
//! storage contract intact while letting the Dispatcher persist its
//! coordination state.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::attestation::{load_attestations, Attestation, AttestationError};
use crate::task::{ExecutionStatus, TaskExecution};

/// On-disk encoding of a [`TaskExecution`]. Attestations are referenced by
/// id rather than embedded so the two storage files (`attestations.json`,
/// `executions.json`) stay independently appendable and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskExecutionRecord {
    pub task_id: Uuid,
    pub task_input: String,
    pub status: ExecutionStatus,
    pub worker_attestation_id: Option<Uuid>,
    pub verifier_attestation_id: Option<Uuid>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

impl TaskExecutionRecord {
    pub fn from_execution(te: &TaskExecution) -> Self {
        Self {
            task_id: te.task_id,
            task_input: te.task_input.clone(),
            status: te.status.clone(),
            worker_attestation_id: te.worker_attestation.as_ref().map(|a| a.id),
            verifier_attestation_id: te.verifier_attestation.as_ref().map(|a| a.id),
            started_at: te.started_at,
            completed_at: te.completed_at,
        }
    }

    /// Rejoin the record with attestations loaded from the attestations
    /// log, returning a fully-populated [`TaskExecution`]. Missing
    /// attestations leave the corresponding field as `None` so a partial
    /// or corrupted log doesn't lose the rest of the execution context.
    pub fn into_execution(self, by_id: &HashMap<Uuid, Attestation>) -> TaskExecution {
        let worker_attestation = self
            .worker_attestation_id
            .and_then(|id| by_id.get(&id).cloned());
        let verifier_attestation = self
            .verifier_attestation_id
            .and_then(|id| by_id.get(&id).cloned());
        TaskExecution {
            task_id: self.task_id,
            task_input: self.task_input,
            status: self.status,
            worker_attestation,
            verifier_attestation,
            started_at: self.started_at,
            completed_at: self.completed_at,
        }
    }
}

/// Append a [`TaskExecution`] to `executions_path` as its compact
/// id-referencing record form. Attestations must already be persisted
/// separately via [`crate::append_attestation`] for [`load_execution`] to
/// rehydrate them.
pub fn append_execution(
    executions_path: &Path,
    execution: &TaskExecution,
) -> Result<(), AttestationError> {
    if let Some(parent) = executions_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(executions_path)?;
    let record = TaskExecutionRecord::from_execution(execution);
    let line = serde_json::to_string(&record)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Load every record from the executions log without rehydrating
/// attestations. Useful for callers that only need the lifecycle status.
pub fn load_execution_records(
    executions_path: &Path,
) -> Result<Vec<TaskExecutionRecord>, AttestationError> {
    if !executions_path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(executions_path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TaskExecutionRecord>(&line) {
            Ok(r) => out.push(r),
            Err(e) => {
                eprintln!(
                    "warn: skipping execution at {}:{} — parse error: {e}",
                    executions_path.display(),
                    lineno + 1
                );
            }
        }
    }
    Ok(out)
}

/// Load every [`TaskExecution`] from `executions_path`, rehydrating each
/// one's attestations by id from the attestations log at
/// `attestations_path`.
pub fn load_executions(
    executions_path: &Path,
    attestations_path: &Path,
) -> Result<Vec<TaskExecution>, AttestationError> {
    let records = load_execution_records(executions_path)?;
    let attestations = load_attestations(attestations_path)?;
    let by_id: HashMap<Uuid, Attestation> = attestations.into_iter().map(|a| (a.id, a)).collect();
    Ok(records
        .into_iter()
        .map(|r| r.into_execution(&by_id))
        .collect())
}

/// Load a single execution by id, rejoined with its attestations. Returns
/// `Ok(None)` if no execution with that id exists in the log.
pub fn load_execution(
    executions_path: &Path,
    attestations_path: &Path,
    task_id: Uuid,
) -> Result<Option<TaskExecution>, AttestationError> {
    let records = load_execution_records(executions_path)?;
    let Some(record) = records.into_iter().find(|r| r.task_id == task_id) else {
        return Ok(None);
    };
    let attestations = load_attestations(attestations_path)?;
    let by_id: HashMap<Uuid, Attestation> = attestations.into_iter().map(|a| (a.id, a)).collect();
    Ok(Some(record.into_execution(&by_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{append_attestation, AttestationStatus};
    use crate::signing::{sha256, AgentKeypair};
    use tempfile::tempdir;

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
    fn append_then_load_executions_round_trip() {
        let dir = tempdir().unwrap();
        let exec_path = dir.path().join("executions.json");
        let att_path = dir.path().join("attestations.json");
        let kp = AgentKeypair::generate();

        let worker_att = signed_completed(&kp, "84");
        let verifier_att = signed_completed(&kp, "ok");

        append_attestation(&att_path, &worker_att).unwrap();
        append_attestation(&att_path, &verifier_att).unwrap();

        let mut te = TaskExecution::new("140 * 0.6", 1_700_000_000);
        te.worker_attestation = Some(worker_att.clone());
        te.verifier_attestation = Some(verifier_att.clone());
        te.status = ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        };
        te.completed_at = Some(1_700_000_002);

        append_execution(&exec_path, &te).unwrap();

        let loaded = load_executions(&exec_path, &att_path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], te);
    }

    #[test]
    fn load_execution_finds_by_id() {
        let dir = tempdir().unwrap();
        let exec_path = dir.path().join("executions.json");
        let att_path = dir.path().join("attestations.json");

        let te1 = TaskExecution::new("first", 1_700_000_000);
        let te2 = TaskExecution::new("second", 1_700_000_001);
        append_execution(&exec_path, &te1).unwrap();
        append_execution(&exec_path, &te2).unwrap();

        let found = load_execution(&exec_path, &att_path, te2.task_id)
            .unwrap()
            .expect("execution should exist");
        assert_eq!(found.task_id, te2.task_id);
        assert_eq!(found.task_input, "second");

        let missing = load_execution(&exec_path, &att_path, Uuid::new_v4()).unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn load_executions_handles_missing_attestation() {
        let dir = tempdir().unwrap();
        let exec_path = dir.path().join("executions.json");
        let att_path = dir.path().join("attestations.json");
        let kp = AgentKeypair::generate();

        let worker_att = signed_completed(&kp, "84");
        // Note: not appending worker_att to att_path — simulates a stale
        // executions.json that references a missing attestation.
        let mut te = TaskExecution::new("140 * 0.6", 1_700_000_000);
        te.worker_attestation = Some(worker_att);
        te.status = ExecutionStatus::Complete {
            attestation_valid: true,
            answer_correct: true,
        };
        append_execution(&exec_path, &te).unwrap();

        let loaded = load_executions(&exec_path, &att_path).unwrap();
        assert_eq!(loaded.len(), 1);
        // worker_attestation rejoins as None because the attestations file
        // is empty; the execution metadata still loads.
        assert!(loaded[0].worker_attestation.is_none());
        assert_eq!(loaded[0].task_input, "140 * 0.6");
    }

    #[test]
    fn load_executions_returns_empty_for_missing_files() {
        let dir = tempdir().unwrap();
        let exec_path = dir.path().join("nope_exec.json");
        let att_path = dir.path().join("nope_att.json");
        let loaded = load_executions(&exec_path, &att_path).unwrap();
        assert!(loaded.is_empty());
    }
}
