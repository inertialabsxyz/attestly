//! Core data structures, signing, and storage for the Agent Work Protocol (AWP).
//!
//! This crate is intentionally framework-agnostic — it knows nothing about
//! agent runtimes, tools, or LLMs. Higher-level coordination lives in
//! `awp-agents`.

pub mod attestation;
pub mod merkle;
pub mod signing;
pub mod storage;

pub use attestation::{
    append_attestation, load_attestations, Attestation, AttestationError, AttestationStatus,
};
pub use signing::{sha256, verify_attestation_signature, AgentKeypair};
