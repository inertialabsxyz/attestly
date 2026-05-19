//! Canonical re-encoding and re-verification helpers.
//!
//! All verification flows through `awp-core::Attestation::signing_payload`
//! and `awp-core::verify_attestation_signature` — there is exactly one
//! canonical encoder in the repo, and this service is a consumer of it. If a
//! cross-language byte-equality test ever fails, the failure is between the
//! client and `awp-core`, not between the client and this service.

use awp_core::{verify_attestation_signature, Attestation};
use sha2::{Digest, Sha256};

/// Canonical bytes of the attestation as stored at rest. These are the bytes
/// content-addressed by `blob_sha256` in the database.
///
/// We store the *full* `Attestation` JSON (including the signature) rather
/// than just the signing payload — the signature is part of the record, and
/// the viewer needs it to re-verify. The signing payload is derivable from
/// the full record at read time via `Attestation::signing_payload`.
///
/// The encoding is deterministic for a given `Attestation` because every
/// field is either a fixed-width type, a `String`, or a hex-encoded byte
/// array, and `serde_json::to_vec` preserves struct field order.
pub fn canonical_blob_bytes(att: &Attestation) -> Vec<u8> {
    // `to_vec` is infallible for `Attestation` (no maps with non-string keys,
    // no infinite floats, etc.).
    serde_json::to_vec(att).expect("Attestation JSON serialization is infallible")
}

/// SHA-256 of the canonical blob bytes. Used as the content address.
pub fn blob_sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Re-verify an attestation against its own embedded `agent_pubkey`. Wraps
/// `awp-core::verify_attestation_signature` so the call site reads
/// intention-revealingly at the handler layer.
pub fn reverify(att: &Attestation) -> bool {
    verify_attestation_signature(att)
}

/// Parse canonical bytes back into an `Attestation`, returning `None` on any
/// failure. Failure is treated by callers as a tamper signal — a blob we
/// wrote must always parse, so a parse failure indicates the bytes have been
/// mutated at rest.
pub fn parse_blob(bytes: &[u8]) -> Option<Attestation> {
    serde_json::from_slice(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use awp_core::{signing::AgentKeypair, AttestationStatus};

    fn fixture_signed() -> Attestation {
        let kp = AgentKeypair::generate();
        let task_hash = awp_core::signing::sha256(b"task-canonical");
        let mut a = Attestation::new(
            "agent-fixture",
            task_hash,
            "{\"customer_id\":\"4711\"}",
            AttestationStatus::Completed,
            None,
            1_700_000_001,
        );
        kp.sign_attestation(&mut a);
        a
    }

    #[test]
    fn canonical_blob_round_trips() {
        let a = fixture_signed();
        let bytes = canonical_blob_bytes(&a);
        let parsed = parse_blob(&bytes).unwrap();
        assert_eq!(parsed, a);
    }

    #[test]
    fn reverify_passes_for_genuine_attestation() {
        let a = fixture_signed();
        assert!(reverify(&a));
    }

    #[test]
    fn reverify_fails_for_tampered_output() {
        let mut a = fixture_signed();
        a.output = "evil".into();
        assert!(!reverify(&a));
    }

    #[test]
    fn parse_blob_returns_none_on_garbage() {
        assert!(parse_blob(b"not-json").is_none());
        assert!(parse_blob(b"{\"id\":\"not-a-uuid\"}").is_none());
    }

    #[test]
    fn blob_sha256_is_stable() {
        let a = fixture_signed();
        let bytes = canonical_blob_bytes(&a);
        let h1 = blob_sha256(&bytes);
        let h2 = blob_sha256(&bytes);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);
    }
}
