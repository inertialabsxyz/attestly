//! Core data structures, signing, and storage for the Agent Work Protocol (AWP).
//!
//! This crate is intentionally framework-agnostic — it knows nothing about
//! agent runtimes, tools, or LLMs. Higher-level coordination lives in
//! `awp-agents`.

pub mod attestation;
pub mod execution;
pub mod kyc;
pub mod merkle;
pub mod signing;
pub mod storage;
pub mod task;

pub use attestation::{
    append_attestation, load_attestations, Attestation, AttestationError, AttestationStatus,
};
pub use execution::{
    append_execution, load_execution, load_execution_records, load_executions, TaskExecutionRecord,
};
pub use kyc::{KycDecision, KycRequest};
pub use merkle::{
    attestation_leaf_hash, build_tree, inclusion_proof, verify_inclusion, Batch, InclusionProof,
    MerkleError, MerkleTree, Position, ProofNode,
};
pub use signing::{sha256, verify_attestation_signature, AgentKeypair};
pub use storage::{Storage, StorageError};
pub use task::{ExecutionStatus, TaskExecution};
