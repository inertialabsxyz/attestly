//! Merkle batching primitives for AWP attestations.
//!
//! Phase 3 of the prototype plan introduces batches: a Merkle tree built
//! over the leaf-hashes of attestations, plus per-leaf inclusion proofs
//! that can be verified given **only** the root, the proof, and the
//! attestation hash. That last property is the hard exit-criterion in
//! `planning/awp-prototype-plan.md` → "Weeks 5-6: Attestation Batching".
//!
//! ## Leaf-hash binding
//!
//! Each attestation's leaf hash is `SHA-256(signing_payload || signature)`.
//! Binding the signature into the leaf means a tampered attestation —
//! whether the payload or the signature was altered — produces a different
//! leaf and therefore a different root. This keeps the Phase 1 contract
//! intact: `signing_payload` is unchanged, signatures still bind the
//! attestation, and the Merkle layer only adds a cryptographically-strong
//! *witness* that an attestation was included in a batch.

use rs_merkle::algorithms::Sha256 as MerkleSha256;
#[cfg(test)]
use rs_merkle::MerkleProof;
use rs_merkle::{Hasher, MerkleTree as RsMerkleTree};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::attestation::Attestation;
use crate::signing::sha256;

/// Whether a sibling sits to the left or right of the node it pairs with.
///
/// The pair order matters for the parent hash — Merkle trees concatenate
/// `(left || right)` before hashing, so `Position::Left` means the
/// recorded sibling becomes the left half of the concatenation and the
/// "current" running hash becomes the right half (and vice versa).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    Left,
    Right,
}

/// One step in an inclusion proof — the sibling hash plus its side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofNode {
    #[serde(with = "hex_array_32")]
    pub hash: [u8; 32],
    pub position: Position,
}

/// A self-contained inclusion proof: given `attestation_hash`, the
/// `proof_path`, and a batch root, anyone can re-derive the root and
/// confirm membership without access to the rest of the batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InclusionProof {
    pub attestation_id: Uuid,
    #[serde(with = "hex_array_32")]
    pub attestation_hash: [u8; 32],
    pub batch_id: Uuid,
    pub proof_path: Vec<ProofNode>,
}

/// A batch of attestations and the Merkle root that summarises them.
///
/// `anchor_tx` and `anchor_chain` are reserved for the on-chain anchoring
/// phase deferred to Phase 2 of AWP overall. Phase 3 always writes them as
/// `None` / `NULL`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub id: Uuid,
    #[serde(with = "hex_array_32")]
    pub merkle_root: [u8; 32],
    pub attestation_ids: Vec<Uuid>,
    pub attestation_count: u32,
    pub created_at: i64,
    pub anchor_tx: Option<String>,
    pub anchor_chain: Option<String>,
}

/// Errors raised while building or verifying Merkle artifacts.
#[derive(Debug, Error)]
pub enum MerkleError {
    #[error("cannot build a tree from zero leaves")]
    Empty,
    #[error("leaf index {0} out of bounds for tree of {1} leaves")]
    IndexOutOfRange(usize, usize),
    #[error("rs_merkle could not produce a root for the supplied leaves")]
    MissingRoot,
}

/// Wrapper around `rs_merkle::MerkleTree` so callers don't need to depend
/// on `rs_merkle` directly. We keep the underlying tree plus the original
/// leaf hashes so we can produce proofs and answer questions like "how
/// many leaves are in this tree" without re-walking it.
pub struct MerkleTree {
    inner: RsMerkleTree<MerkleSha256>,
    leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    /// The 32-byte root hash. `None` if the tree is empty (zero leaves).
    pub fn root(&self) -> Option<[u8; 32]> {
        self.inner.root()
    }

    /// Total leaves indexed by this tree.
    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// Return the leaf hashes in insertion order.
    pub fn leaves(&self) -> &[[u8; 32]] {
        &self.leaves
    }
}

/// Compute the canonical leaf hash for an attestation: SHA-256 over the
/// attestation's signing payload concatenated with its signature.
///
/// Including the signature in the leaf means batches *attest to the
/// signed records themselves*, not merely the inputs that were signed.
/// A tampered signature changes the leaf → changes the root → fails the
/// inclusion check.
pub fn attestation_leaf_hash(att: &Attestation) -> [u8; 32] {
    let mut buf = att.signing_payload();
    buf.extend_from_slice(&att.signature);
    sha256(&buf)
}

/// Build a Merkle tree from `leaves`.
///
/// Returns `Err(MerkleError::Empty)` if `leaves` is empty — `rs_merkle`
/// can technically build a zero-leaf tree but it has no root, and a
/// rootless batch is meaningless to AWP.
pub fn build_tree(leaves: &[[u8; 32]]) -> Result<MerkleTree, MerkleError> {
    if leaves.is_empty() {
        return Err(MerkleError::Empty);
    }
    let inner = RsMerkleTree::<MerkleSha256>::from_leaves(leaves);
    if inner.root().is_none() {
        return Err(MerkleError::MissingRoot);
    }
    Ok(MerkleTree {
        inner,
        leaves: leaves.to_vec(),
    })
}

/// Produce an inclusion proof for the leaf at `index` in `tree`.
///
/// `attestation_id` and `batch_id` are propagated into the resulting
/// [`InclusionProof`] so the caller can store proofs keyed by the
/// attestation they witness. The proof itself is the sibling-hash path
/// derived from `rs_merkle`, decorated with explicit Left/Right positions
/// so [`verify_inclusion`] can re-derive the root without consulting the
/// tree.
pub fn inclusion_proof(
    tree: &MerkleTree,
    index: usize,
    attestation_id: Uuid,
    batch_id: Uuid,
) -> Result<InclusionProof, MerkleError> {
    let leaf_count = tree.leaf_count();
    if index >= leaf_count {
        return Err(MerkleError::IndexOutOfRange(index, leaf_count));
    }
    let leaf = tree.leaves[index];
    let proof = tree.inner.proof(&[index]);
    let proof_path = build_proof_path(&proof.proof_hashes_hex(), index, leaf_count);
    Ok(InclusionProof {
        attestation_id,
        attestation_hash: leaf,
        batch_id,
        proof_path,
    })
}

/// Verify that `attestation_hash` is a leaf of a Merkle tree whose root
/// is `expected_root`, using only `proof`. The function reconstructs the
/// root by folding each [`ProofNode`] into the running hash according to
/// its [`Position`], then compares to `expected_root`.
///
/// **Inputs intentionally minimal.** No tree, no batch, no attestation
/// list — just the three values listed in the prototype plan's exit
/// criterion. Any caller in possession of those three values can verify
/// inclusion offline.
pub fn verify_inclusion(
    expected_root: &[u8; 32],
    attestation_hash: &[u8; 32],
    proof: &InclusionProof,
) -> bool {
    if &proof.attestation_hash != attestation_hash {
        return false;
    }
    let mut current = *attestation_hash;
    for node in &proof.proof_path {
        current = match node.position {
            Position::Left => MerkleSha256::concat_and_hash(&node.hash, Some(&current)),
            Position::Right => MerkleSha256::concat_and_hash(&current, Some(&node.hash)),
        };
    }
    &current == expected_root
}

/// Re-derive the per-step Left/Right positions for a single-leaf proof.
///
/// `rs_merkle::MerkleProof::proof_hashes_hex` returns the sibling hashes
/// in bottom-up order but doesn't tell us which side of the pair each
/// sibling sat on. We reconstruct that from the leaf index: at each level
/// of the tree, an even index pairs with its right sibling and an odd
/// index pairs with its left sibling; the index is then halved for the
/// next level.
///
/// `rs_merkle` skips the proof step when a level has no sibling (an odd
/// last-leaf at that level), so we only emit a [`ProofNode`] when there
/// is a corresponding hash in `hashes_hex`.
fn build_proof_path(hashes_hex: &[String], leaf_index: usize, leaf_count: usize) -> Vec<ProofNode> {
    let mut path = Vec::with_capacity(hashes_hex.len());
    let mut hash_iter = hashes_hex.iter();
    let mut idx = leaf_index;
    let mut level_size = leaf_count;
    while level_size > 1 {
        let is_left_child = idx.is_multiple_of(2);
        let has_sibling = if is_left_child {
            idx + 1 < level_size
        } else {
            true
        };
        if has_sibling {
            let Some(hex_str) = hash_iter.next() else {
                break;
            };
            let mut bytes = [0u8; 32];
            // proof_hashes_hex is produced by rs_merkle from valid 32-byte
            // hashes; a parse failure here is a library bug, not a runtime
            // condition we can recover from.
            let raw = hex::decode(hex_str).expect("rs_merkle proof hash is valid hex");
            bytes.copy_from_slice(&raw);
            let position = if is_left_child {
                Position::Right
            } else {
                Position::Left
            };
            path.push(ProofNode {
                hash: bytes,
                position,
            });
        }
        idx /= 2;
        level_size = level_size.div_ceil(2);
    }
    path
}

/// Given a `MerkleProof` and the index it covers, reconstruct the root by
/// re-applying the proof against `leaf`. Useful for cross-checking our
/// `verify_inclusion` against `rs_merkle`'s own verifier; not exposed as
/// part of the public API.
#[cfg(test)]
fn rs_merkle_verify(
    expected_root: &[u8; 32],
    proof: &MerkleProof<MerkleSha256>,
    leaf_index: usize,
    leaf: &[u8; 32],
    leaf_count: usize,
) -> bool {
    proof.verify(*expected_root, &[leaf_index], &[*leaf], leaf_count)
}

mod hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(de: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(de)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        v.try_into()
            .map_err(|_| serde::de::Error::custom("expected 32 bytes hex"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::AttestationStatus;
    use crate::signing::AgentKeypair;

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
    fn empty_leaves_rejected() {
        let leaves: Vec<[u8; 32]> = Vec::new();
        assert!(matches!(build_tree(&leaves), Err(MerkleError::Empty)));
    }

    #[test]
    fn single_leaf_tree_root_is_leaf() {
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        let leaf = attestation_leaf_hash(&a);
        let tree = build_tree(&[leaf]).unwrap();
        // A single-leaf tree's root is the leaf itself per rs_merkle's
        // convention (and the standard).
        assert_eq!(tree.root().unwrap(), leaf);
        let proof = inclusion_proof(&tree, 0, a.id, Uuid::new_v4()).unwrap();
        assert!(proof.proof_path.is_empty());
        assert!(verify_inclusion(&tree.root().unwrap(), &leaf, &proof));
    }

    #[test]
    fn two_leaf_tree_proof_verifies_for_each_leaf() {
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        let b = signed(&kp, "126");
        let leaves = [attestation_leaf_hash(&a), attestation_leaf_hash(&b)];
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let batch_id = Uuid::new_v4();
        let p0 = inclusion_proof(&tree, 0, a.id, batch_id).unwrap();
        let p1 = inclusion_proof(&tree, 1, b.id, batch_id).unwrap();
        assert!(verify_inclusion(&root, &leaves[0], &p0));
        assert!(verify_inclusion(&root, &leaves[1], &p1));
        assert_eq!(p0.proof_path.len(), 1);
        assert_eq!(p1.proof_path.len(), 1);
        assert_eq!(p0.proof_path[0].position, Position::Right);
        assert_eq!(p1.proof_path[0].position, Position::Left);
    }

    #[test]
    fn three_leaf_odd_tree_proof_verifies_for_every_leaf() {
        let kp = AgentKeypair::generate();
        let leaves: Vec<[u8; 32]> = (0..3)
            .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
            .collect();
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let batch_id = Uuid::new_v4();
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = inclusion_proof(&tree, i, Uuid::new_v4(), batch_id).unwrap();
            assert!(
                verify_inclusion(&root, leaf, &proof),
                "leaf {i} should verify"
            );
        }
    }

    #[test]
    fn sixteen_leaf_tree_proof_verifies_for_every_leaf() {
        let kp = AgentKeypair::generate();
        let leaves: Vec<[u8; 32]> = (0..16)
            .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
            .collect();
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let batch_id = Uuid::new_v4();
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = inclusion_proof(&tree, i, Uuid::new_v4(), batch_id).unwrap();
            assert!(
                verify_inclusion(&root, leaf, &proof),
                "leaf {i} should verify"
            );
            assert_eq!(proof.proof_path.len(), 4, "16 leaves → depth 4");
        }
    }

    #[test]
    fn tampered_leaf_fails_verification() {
        let kp = AgentKeypair::generate();
        let leaves: Vec<[u8; 32]> = (0..8)
            .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
            .collect();
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let proof = inclusion_proof(&tree, 3, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        // Flip a single bit in the attestation hash supplied to the verifier.
        let mut tampered = leaves[3];
        tampered[0] ^= 0x01;
        assert!(!verify_inclusion(&root, &tampered, &proof));
    }

    #[test]
    fn tampered_proof_path_fails_verification() {
        let kp = AgentKeypair::generate();
        let leaves: Vec<[u8; 32]> = (0..8)
            .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
            .collect();
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let mut proof = inclusion_proof(&tree, 3, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        // Flip a bit in the first sibling hash.
        proof.proof_path[0].hash[0] ^= 0x01;
        assert!(!verify_inclusion(&root, &leaves[3], &proof));
    }

    #[test]
    fn out_of_range_index_rejected() {
        let kp = AgentKeypair::generate();
        let leaves = [attestation_leaf_hash(&signed(&kp, "84"))];
        let tree = build_tree(&leaves).unwrap();
        assert!(matches!(
            inclusion_proof(&tree, 5, Uuid::new_v4(), Uuid::new_v4()),
            Err(MerkleError::IndexOutOfRange(5, 1))
        ));
    }

    #[test]
    fn our_verifier_agrees_with_rs_merkle() {
        // Cross-check: for every tree size we test, our verify_inclusion
        // and rs_merkle's own MerkleProof::verify must give the same answer.
        let kp = AgentKeypair::generate();
        for size in [1usize, 2, 3, 4, 5, 8, 16] {
            let leaves: Vec<[u8; 32]> = (0..size)
                .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
                .collect();
            let tree = build_tree(&leaves).unwrap();
            let root = tree.root().unwrap();
            for (i, leaf) in leaves.iter().enumerate() {
                let our_proof = inclusion_proof(&tree, i, Uuid::new_v4(), Uuid::new_v4()).unwrap();
                let lib_proof = tree.inner.proof(&[i]);
                assert!(verify_inclusion(&root, leaf, &our_proof));
                assert!(rs_merkle_verify(&root, &lib_proof, i, leaf, size));
            }
        }
    }

    #[test]
    fn proof_round_trips_through_serde() {
        let kp = AgentKeypair::generate();
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|i| attestation_leaf_hash(&signed(&kp, &format!("o{i}"))))
            .collect();
        let tree = build_tree(&leaves).unwrap();
        let root = tree.root().unwrap();
        let proof = inclusion_proof(&tree, 2, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        let json = serde_json::to_string(&proof).unwrap();
        let back: InclusionProof = serde_json::from_str(&json).unwrap();
        assert_eq!(proof, back);
        assert!(verify_inclusion(&root, &leaves[2], &back));
    }

    #[test]
    fn batch_round_trips_through_serde() {
        let batch = Batch {
            id: Uuid::new_v4(),
            merkle_root: [42u8; 32],
            attestation_ids: vec![Uuid::new_v4(), Uuid::new_v4()],
            attestation_count: 2,
            created_at: 1_700_000_000,
            anchor_tx: None,
            anchor_chain: None,
        };
        let json = serde_json::to_string(&batch).unwrap();
        let back: Batch = serde_json::from_str(&json).unwrap();
        assert_eq!(batch, back);
    }

    #[test]
    fn leaf_hash_changes_when_signature_changes() {
        // Defends the "leaf binds the signature" invariant — an attacker
        // who tampers with the signature must not be able to keep the
        // leaf hash stable.
        let kp = AgentKeypair::generate();
        let mut a = signed(&kp, "84");
        let h1 = attestation_leaf_hash(&a);
        a.signature[0] ^= 0xff;
        let h2 = attestation_leaf_hash(&a);
        assert_ne!(h1, h2);
    }
}
