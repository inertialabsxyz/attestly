//! Worker, Verifier, Dispatcher, and (later) Batcher agents for AWP.
//!
//! Phase 1 shipped Worker, Verifier, and the shared tools they call. Phase 2
//! adds the Dispatcher coordinator. The `batcher` module is stub-only until
//! Phase 3.

pub mod batcher;
pub mod dispatcher;
pub mod tools;
pub mod verifier;
pub mod worker;

pub use dispatcher::{Dispatcher, DispatcherConfig, DispatcherError, DEFAULT_STAGE_TIMEOUT};
pub use tools::{calculate, verify_attestation, verify_attestation_struct, VerificationResult};
pub use verifier::{Verifier, VerifierAgent, VerifierError};
pub use worker::{Worker, WorkerAgent, WorkerError, WorkerTask};
