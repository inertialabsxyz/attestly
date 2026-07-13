//! Batcher — collects attestations into Merkle batches and persists them to SQLite.
//!
//! ## Trigger semantics
//!
//! A flush fires when **either** condition is met:
//!
//! - **Count trigger** — the buffer reaches `max_batch_size`.
//! - **Age trigger** — the *oldest* buffered attestation has been waiting
//!   longer than `max_batch_age_secs`. A background tokio task polls this
//!   condition every second so the trigger has at most one-second of
//!   latency past the configured age.
//!
//! `min_batch_size` is honoured on **shutdown only** — a normal age- or
//! count-trigger ignores it (because the configured defaults set
//! `min_batch_size = 1` anyway). A non-default `min_batch_size > 1`
//! exists for callers that want shutdown to *drop* a too-small final
//! buffer rather than seal it.
//!
//! ## Storage contract
//!
//! On every flush the Batcher:
//!
//! 1. Computes leaf hashes via `attestly_core::attestation_leaf_hash`
//! 2. Builds a Merkle tree
//! 3. Computes per-leaf inclusion proofs
//! 4. Persists the [`Batch`] + every [`InclusionProof`] in one SQLite
//!    transaction (`Storage::insert_batch_with_proofs`)
//!
//! The same attestation submitted twice (same `id`) is accepted but
//! `INSERT OR IGNORE`d at the SQLite layer so the underlying row is not
//! duplicated.

use std::sync::Arc;
use std::time::Duration;

use attestly_core::{
    attestation_leaf_hash, build_tree, inclusion_proof, Attestation, Batch, InclusionProof,
    MerkleError, Storage, StorageError,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

/// Default batcher configuration from `planning/attestly-prototype-plan.md`
/// → "Batcher Configuration".
pub const DEFAULT_MAX_BATCH_SIZE: u32 = 10;
pub const DEFAULT_MAX_BATCH_AGE_SECS: u64 = 60;
pub const DEFAULT_MIN_BATCH_SIZE: u32 = 1;

/// Configuration for the Batcher service.
///
/// Defaults match the plan exactly:
/// `max_batch_size: 10, max_batch_age_secs: 60, min_batch_size: 1`.
#[derive(Debug, Clone)]
pub struct BatcherConfig {
    /// Trigger a flush when the buffer reaches this size.
    pub max_batch_size: u32,
    /// Trigger a flush when the oldest buffered attestation exceeds
    /// this age.
    pub max_batch_age_secs: u64,
    /// On **shutdown only**, refuse to seal a batch smaller than this.
    /// Normal triggers ignore this.
    pub min_batch_size: u32,
}

impl Default for BatcherConfig {
    fn default() -> Self {
        Self {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_batch_age_secs: DEFAULT_MAX_BATCH_AGE_SECS,
            min_batch_size: DEFAULT_MIN_BATCH_SIZE,
        }
    }
}

/// Errors raised by the Batcher service.
#[derive(Debug, Error)]
pub enum BatcherError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("merkle error: {0}")]
    Merkle(#[from] MerkleError),
    #[error("batcher channel closed")]
    Closed,
}

/// Internal command passed from `Batcher` (the public handle) to its
/// background task. `Submit` is boxed because an `Attestation` is much
/// larger than the other variants and clippy flags the size disparity.
enum Command {
    Submit(Box<Attestation>),
    /// Force an immediate flush regardless of trigger conditions.
    /// Used by tests and the shutdown path.
    Flush(oneshot::Sender<Result<Option<Uuid>, BatcherError>>),
    /// Final flush + drain. The background task exits after handling this.
    Shutdown(oneshot::Sender<Result<Option<Uuid>, BatcherError>>),
}

/// Public handle to the Batcher service.
///
/// Cloning the handle gives multiple producers a way to submit
/// attestations to the same underlying buffer; the background task is
/// owned by exactly one handle (the one returned from [`Batcher::start`]).
pub struct Batcher {
    tx: mpsc::Sender<Command>,
    join: Mutex<Option<JoinHandle<()>>>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Batcher {
    /// Spawn a Batcher with `config`, a `Storage` handle, and a
    /// wall-clock source. The wall clock provides `created_at` on each
    /// produced [`Batch`] — overridable for deterministic tests.
    pub fn start<F>(config: BatcherConfig, storage: Storage, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        let (tx, rx) = mpsc::channel::<Command>(64);
        let clock = Arc::new(clock);
        let task_clock = clock.clone();
        let join = tokio::spawn(run_batcher(config, storage, task_clock, rx));
        Self {
            tx,
            join: Mutex::new(Some(join)),
            clock,
        }
    }

    /// Convenience: start with the standard config and the system clock.
    pub fn standard(storage: Storage) -> Self {
        Self::start(BatcherConfig::default(), storage, default_clock)
    }

    /// Submit an attestation to the buffer. The attestation is durable
    /// only after the next flush.
    pub async fn submit(&self, attestation: Attestation) -> Result<(), BatcherError> {
        self.tx
            .send(Command::Submit(Box::new(attestation)))
            .await
            .map_err(|_| BatcherError::Closed)
    }

    /// Force a flush. Returns the new batch's id, or `Ok(None)` if the
    /// buffer was empty.
    pub async fn flush_now(&self) -> Result<Option<Uuid>, BatcherError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(Command::Flush(resp_tx))
            .await
            .map_err(|_| BatcherError::Closed)?;
        resp_rx.await.map_err(|_| BatcherError::Closed)?
    }

    /// Drive the final flush and stop the background task. After this
    /// returns, `submit` will fail with `BatcherError::Closed`.
    pub async fn shutdown(self) -> Result<Option<Uuid>, BatcherError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(Command::Shutdown(resp_tx))
            .await
            .map_err(|_| BatcherError::Closed)?;
        let result = resp_rx.await.map_err(|_| BatcherError::Closed)?;
        if let Some(handle) = self.join.lock().await.take() {
            // We don't propagate join errors — the background task either
            // returned cleanly (no JoinError) or we'd already see it via
            // the response channel.
            let _ = handle.await;
        }
        result
    }

    /// Read the wall-clock value the Batcher was configured with — used
    /// by tests/examples that want to share the same clock.
    pub fn clock(&self) -> i64 {
        (self.clock)()
    }
}

fn default_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn run_batcher(
    config: BatcherConfig,
    mut storage: Storage,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    mut rx: mpsc::Receiver<Command>,
) {
    let mut buffer: Vec<Attestation> = Vec::new();
    let mut oldest_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    // The first tick fires immediately, which would cause a no-op
    // age-check before any work has arrived. Skip it.
    tick.tick().await;

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(Command::Submit(att)) => {
                        if buffer.is_empty() {
                            oldest_at = Some(Instant::now());
                        }
                        buffer.push(*att);
                        if buffer.len() as u32 >= config.max_batch_size {
                            let _ = flush(&mut storage, &mut buffer, &mut oldest_at, &clock).await;
                        }
                    }
                    Some(Command::Flush(resp)) => {
                        let result = flush(&mut storage, &mut buffer, &mut oldest_at, &clock).await;
                        let _ = resp.send(result);
                    }
                    Some(Command::Shutdown(resp)) => {
                        // On shutdown, flush only if the buffer meets min_batch_size.
                        let result = if (buffer.len() as u32) >= config.min_batch_size {
                            flush(&mut storage, &mut buffer, &mut oldest_at, &clock).await
                        } else {
                            // Discard the small final buffer — caller
                            // configured min_batch_size > current depth.
                            buffer.clear();
                            Ok(None)
                        };
                        let _ = resp.send(result);
                        return;
                    }
                    None => {
                        // All senders dropped; perform a best-effort final
                        // flush respecting min_batch_size, then exit.
                        if (buffer.len() as u32) >= config.min_batch_size {
                            let _ = flush(&mut storage, &mut buffer, &mut oldest_at, &clock).await;
                        }
                        return;
                    }
                }
            }
            _ = tick.tick() => {
                if let Some(start) = oldest_at {
                    if start.elapsed() >= Duration::from_secs(config.max_batch_age_secs) {
                        let _ = flush(&mut storage, &mut buffer, &mut oldest_at, &clock).await;
                    }
                }
            }
        }
    }
}

async fn flush(
    storage: &mut Storage,
    buffer: &mut Vec<Attestation>,
    oldest_at: &mut Option<Instant>,
    clock: &Arc<dyn Fn() -> i64 + Send + Sync>,
) -> Result<Option<Uuid>, BatcherError> {
    if buffer.is_empty() {
        return Ok(None);
    }
    let attestations = std::mem::take(buffer);
    *oldest_at = None;

    let leaves: Vec<[u8; 32]> = attestations.iter().map(attestation_leaf_hash).collect();
    let tree = build_tree(&leaves)?;
    let batch_id = Uuid::new_v4();
    let proofs: Vec<InclusionProof> = attestations
        .iter()
        .enumerate()
        .map(|(i, a)| inclusion_proof(&tree, i, a.id, batch_id))
        .collect::<Result<Vec<_>, _>>()?;
    let batch = Batch {
        id: batch_id,
        merkle_root: tree.root().expect("non-empty tree always has a root"),
        attestation_ids: attestations.iter().map(|a| a.id).collect(),
        attestation_count: attestations.len() as u32,
        created_at: clock(),
        anchor_tx: None,
        anchor_chain: None,
    };
    storage.insert_batch_with_proofs(&batch, &attestations, &proofs)?;
    Ok(Some(batch_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use attestly_core::{sha256, AgentKeypair, AttestationStatus};

    fn signed(kp: &AgentKeypair, output: &str) -> Attestation {
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
    fn default_config_matches_plan() {
        let cfg = BatcherConfig::default();
        assert_eq!(cfg.max_batch_size, 10);
        assert_eq!(cfg.max_batch_age_secs, 60);
        assert_eq!(cfg.min_batch_size, 1);
    }

    #[tokio::test]
    async fn count_trigger_seals_a_batch_at_max_batch_size() {
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(
            BatcherConfig {
                max_batch_size: 3,
                max_batch_age_secs: 3600,
                min_batch_size: 1,
            },
            storage,
            || 1_700_000_000,
        );
        let kp = AgentKeypair::generate();
        let atts: Vec<_> = (0..3).map(|i| signed(&kp, &format!("o{i}"))).collect();
        for a in &atts {
            batcher.submit(a.clone()).await.unwrap();
        }
        // Give the background task a moment to drain the channel.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Verify by forcing another flush — buffer should be empty.
        let id = batcher.flush_now().await.unwrap();
        assert!(id.is_none(), "buffer should already be flushed by count");
        let _ = batcher.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn flush_now_returns_none_when_buffer_empty() {
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(BatcherConfig::default(), storage, || 1_700_000_000);
        assert!(batcher.flush_now().await.unwrap().is_none());
        let _ = batcher.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn flush_now_seals_partial_buffer() {
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(
            BatcherConfig {
                max_batch_size: 100,
                max_batch_age_secs: 3600,
                min_batch_size: 1,
            },
            storage,
            || 1_700_000_000,
        );
        let kp = AgentKeypair::generate();
        for i in 0..3 {
            batcher.submit(signed(&kp, &format!("o{i}"))).await.unwrap();
        }
        let id = batcher.flush_now().await.unwrap().expect("a batch");
        // Second flush is a no-op.
        assert!(batcher.flush_now().await.unwrap().is_none());
        let _ = batcher.shutdown().await.unwrap();
        // Exhaustively check via a fresh handle? Storage was moved.
        // For verification we rely on the integration test in tests/.
        // Here we just confirm the id is real.
        assert_ne!(id, Uuid::nil());
    }

    #[tokio::test]
    async fn shutdown_flushes_outstanding_buffer() {
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(
            BatcherConfig {
                max_batch_size: 100,
                max_batch_age_secs: 3600,
                min_batch_size: 1,
            },
            storage,
            || 1_700_000_000,
        );
        let kp = AgentKeypair::generate();
        for i in 0..2 {
            batcher.submit(signed(&kp, &format!("o{i}"))).await.unwrap();
        }
        let id = batcher.shutdown().await.unwrap();
        assert!(id.is_some(), "shutdown should seal final partial batch");
    }

    #[tokio::test]
    async fn shutdown_drops_buffer_smaller_than_min_batch_size() {
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(
            BatcherConfig {
                max_batch_size: 100,
                max_batch_age_secs: 3600,
                min_batch_size: 5,
            },
            storage,
            || 1_700_000_000,
        );
        let kp = AgentKeypair::generate();
        for i in 0..2 {
            batcher.submit(signed(&kp, &format!("o{i}"))).await.unwrap();
        }
        // 2 < min_batch_size of 5 → shutdown discards the buffer rather
        // than sealing it.
        let id = batcher.shutdown().await.unwrap();
        assert!(id.is_none());
    }

    #[tokio::test]
    async fn age_trigger_fires_after_max_batch_age() {
        // Use a 1-second age trigger so the test runs quickly. Background
        // ticker polls every 1s, so worst case we wait ~2s.
        let storage = Storage::open_in_memory().unwrap();
        let batcher = Batcher::start(
            BatcherConfig {
                max_batch_size: 100,
                max_batch_age_secs: 1,
                min_batch_size: 1,
            },
            storage,
            || 1_700_000_000,
        );
        let kp = AgentKeypair::generate();
        batcher.submit(signed(&kp, "first")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        // The age trigger should have flushed the buffer; flush_now returns None.
        let id = batcher.flush_now().await.unwrap();
        assert!(
            id.is_none(),
            "age trigger should have already sealed the batch"
        );
        let _ = batcher.shutdown().await.unwrap();
    }
}
