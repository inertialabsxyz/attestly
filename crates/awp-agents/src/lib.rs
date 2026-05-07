//! Worker, Verifier, Dispatcher, and Batcher agents for AWP.
//!
//! Phase 1 shipped Worker, Verifier, and the shared tools they call. Phase 2
//! added the Dispatcher coordinator. Phase 3 adds the Batcher service which
//! consumes attestations and persists them as Merkle batches.

pub mod batcher;
pub mod dispatcher;
pub mod tools;
pub mod verifier;
pub mod worker;

pub use batcher::{
    Batcher, BatcherConfig, BatcherError, DEFAULT_MAX_BATCH_AGE_SECS, DEFAULT_MAX_BATCH_SIZE,
    DEFAULT_MIN_BATCH_SIZE,
};
pub use dispatcher::{Dispatcher, DispatcherConfig, DispatcherError, DEFAULT_STAGE_TIMEOUT};
pub use tools::{calculate, verify_attestation, verify_attestation_struct, VerificationResult};
pub use verifier::{Verifier, VerifierAgent, VerifierError};
pub use worker::{Worker, WorkerAgent, WorkerError, WorkerTask};
