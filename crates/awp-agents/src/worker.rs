//! Worker agent — performs a task and emits a signed attestation.
//!
//! ## Framework note
//!
//! The Phase 1 brief instructed: "If AutoAgents has an example worker-verifier
//! pattern in its docs, base your scaffolding on that and cite the source in a
//! code comment." A scan of the AutoAgents repository
//! (<https://github.com/liquidos-ai/AutoAgents>, v0.3.7 as of 2026-05) does
//! not include a documented worker-verifier example — its examples cover
//! single agents (`basic`, `coding_agent`), pipelines, and design patterns,
//! but not a two-agent worker-verifier handshake.
//!
//! Wiring AutoAgents' `AgentBuilder<_, DirectAgent>::new(...).run()` requires
//! a live LLM provider, which would make `make check` non-hermetic and the
//! Phase 1 example unable to run without an API key. Per the prototype plan's
//! risk-mitigation row "Minimal framework coupling in awp-core", Worker and
//! Verifier are implemented here as plain async types whose method signatures
//! mirror what an AutoAgents agent looks like (struct + tools + async run).
//! Phase 4's evaluation revisits whether to swap in `AgentBuilder.run()` and
//! drive these via an LLM. See `docs/PAIN_POINTS.md` for the open question.

use async_trait::async_trait;
use awp_core::{sha256, AgentKeypair, Attestation, AttestationStatus};
use thiserror::Error;

use crate::tools::{calculate, ToolError};

/// What a Worker accepts as input. The plan's example task is arithmetic, so
/// the simplest task input is the expression string itself.
#[derive(Debug, Clone)]
pub struct WorkerTask {
    pub expression: String,
}

impl WorkerTask {
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
        }
    }

    /// Canonical bytes used as the task input — what `task_hash` covers.
    /// We hash the expression string so the Verifier can re-derive it from
    /// the same task input.
    pub fn canonical_bytes(&self) -> &[u8] {
        self.expression.as_bytes()
    }
}

/// Errors a Worker can return. A failed tool execution still yields an
/// attestation (with `AttestationStatus::Failed(reason)`); only signing/io
/// problems surface here.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("tool error: {0}")]
    Tool(#[from] ToolError),
}

/// The Worker agent. Holds an ephemeral keypair plus a wall-clock source
/// (injectable in tests via [`Worker::with_clock`]).
pub struct Worker {
    agent_id: String,
    keypair: AgentKeypair,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl Worker {
    /// Construct a Worker with a freshly-generated keypair.
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            keypair: AgentKeypair::generate(),
            clock: Box::new(default_clock),
        }
    }

    /// Override the wall-clock source — used by tests for deterministic
    /// timestamps.
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
pub trait WorkerAgent {
    async fn run(&self, task: &WorkerTask) -> Result<Attestation, WorkerError>;
}

#[async_trait]
impl WorkerAgent for Worker {
    /// Execute the task via the `calculate` tool, then build and sign an
    /// attestation. Tool failures are folded into
    /// `AttestationStatus::Failed(reason)` so downstream readers always
    /// see a signed record of what happened.
    async fn run(&self, task: &WorkerTask) -> Result<Attestation, WorkerError> {
        let task_hash = sha256(task.canonical_bytes());
        let timestamp = (self.clock)();

        let (output, status) = match calculate(&task.expression) {
            Ok(r) => (format_result(r.result), AttestationStatus::Completed),
            Err(e) => (String::new(), AttestationStatus::Failed(e.to_string())),
        };

        let mut a = Attestation::new(
            self.agent_id.clone(),
            task_hash,
            output,
            status,
            None,
            timestamp,
        );
        self.keypair.sign_attestation(&mut a);
        Ok(a)
    }
}

/// Format a numeric tool result as the canonical output string the Verifier
/// will compare against. We use `{:.10}` then trim trailing zeros so e.g.
/// `84.0` → `"84"` and `0.123456789` keeps useful precision.
pub(crate) fn format_result(value: f64) -> String {
    let s = format!("{value:.10}");
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        s
    }
}

fn default_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use awp_core::verify_attestation_signature;

    #[tokio::test]
    async fn worker_produces_signed_completed_attestation() {
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let task = WorkerTask::new("140 * 0.6");
        let att = w.run(&task).await.unwrap();
        assert_eq!(att.agent_id, "worker-1");
        assert_eq!(att.agent_pubkey, w.public_key());
        assert_eq!(att.status, AttestationStatus::Completed);
        assert_eq!(att.output, "84");
        assert_eq!(att.timestamp, 1_700_000_000);
        assert!(verify_attestation_signature(&att));
    }

    #[tokio::test]
    async fn worker_records_failed_status_on_bad_expression() {
        let w = Worker::new("worker-1").with_clock(|| 1_700_000_000);
        let task = WorkerTask::new("not an expression");
        let att = w.run(&task).await.unwrap();
        assert!(matches!(att.status, AttestationStatus::Failed(_)));
        assert!(verify_attestation_signature(&att));
    }

    #[test]
    fn format_result_strips_trailing_zeros() {
        assert_eq!(format_result(84.0), "84");
        assert_eq!(format_result(84.5), "84.5");
        assert_eq!(format_result(0.1), "0.1");
    }
}
