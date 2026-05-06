//! Worker, Verifier, and (later) Dispatcher and Batcher agents for AWP.
//!
//! Phase 1 ships Worker, Verifier, and the shared tools they call. The
//! `dispatcher` and `batcher` modules are stub-only until Phase 2/3.

pub mod batcher;
pub mod dispatcher;
pub mod tools;
pub mod verifier;
pub mod worker;
