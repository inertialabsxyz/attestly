//! Worker, Verifier, Dispatcher, and Batcher agents for Attestly.
//!
//! Phase 1 shipped Worker, Verifier, and the shared tools they call. Phase 2
//! added the Dispatcher coordinator. Phase 3 adds the Batcher service which
//! consumes attestations and persists them as Merkle batches. Phase 4 adds
//! the [`parallel::ParallelDispatcher`] stretch task — N verifiers running
//! concurrently against one Worker with disagreement detection.

pub mod batcher;
pub mod dispatcher;
pub mod kyc_agents;
pub mod parallel;
pub mod tools;
pub mod verifier;
pub mod worker;

pub use batcher::{
    Batcher, BatcherConfig, BatcherError, DEFAULT_MAX_BATCH_AGE_SECS, DEFAULT_MAX_BATCH_SIZE,
    DEFAULT_MIN_BATCH_SIZE,
};
pub use dispatcher::{Dispatcher, DispatcherConfig, DispatcherError, DEFAULT_STAGE_TIMEOUT};
pub use kyc_agents::{
    decision_output, KycVerifier, KycVerifierAgent, KycVerifierError, KycWorker, KycWorkerAgent,
    KycWorkerError,
};
pub use parallel::{ParallelDispatcher, ParallelDispatcherError, ParallelExecution};
pub use tools::{
    calculate, kyc_decide, verify_attestation, verify_attestation_struct, VerificationResult,
    FLAG_AMOUNT_CENTS, HIGH_RISK_JURISDICTIONS, WIRE_REJECT_AMOUNT_CENTS,
};
pub use verifier::{Verifier, VerifierAgent, VerifierError};
pub use worker::{Worker, WorkerAgent, WorkerError, WorkerTask};
