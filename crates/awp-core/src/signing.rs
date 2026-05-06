//! ed25519 keypair management and SHA-256 helpers for AWP attestations.
//!
//! Keys are ephemeral in Phase 1 — every agent generates a fresh keypair on
//! startup. Persistence and identity registration are deferred to Phase 2 of
//! AWP overall (see `planning/awp-prototype-plan.md` → "What's Explicitly
//! Deferred").

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

use crate::attestation::Attestation;

/// SHA-256 of `bytes`, returned as a 32-byte array.
///
/// Used for both `task_hash` and `output_hash` on attestations.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Wraps an ed25519 signing key plus its derived verifying key.
#[derive(Clone)]
pub struct AgentKeypair {
    signing: SigningKey,
    verifying: VerifyingKey,
}

impl AgentKeypair {
    /// Generate a fresh ephemeral keypair from the OS RNG.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// The 32-byte verifying-key bytes that go into `Attestation::agent_pubkey`.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.verifying.to_bytes()
    }

    /// Populate `attestation.agent_pubkey` and `attestation.signature` so the
    /// attestation is ready to persist.
    pub fn sign_attestation(&self, attestation: &mut Attestation) {
        attestation.agent_pubkey = self.public_bytes();
        let payload = attestation.signing_payload();
        let sig: Signature = self.signing.sign(&payload);
        attestation.signature = sig.to_bytes();
    }
}

/// Verify the signature on an attestation using its embedded public key.
///
/// Returns `false` for any failure mode: malformed pubkey, malformed
/// signature, or signature mismatch. The `Attestation::output_hash` is
/// **not** re-checked here — that's the `verify_attestation` tool's job
/// (see `awp-agents::tools`).
pub fn verify_attestation_signature(attestation: &Attestation) -> bool {
    let pubkey = match VerifyingKey::from_bytes(&attestation.agent_pubkey) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig = Signature::from_bytes(&attestation.signature);
    let payload = attestation.signing_payload();
    pubkey.verify(&payload, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{Attestation, AttestationStatus};

    fn unsigned(output: &str) -> Attestation {
        Attestation::new(
            "agent-x",
            sha256(b"task"),
            output,
            AttestationStatus::Completed,
            None,
            1_700_000_000,
        )
    }

    #[test]
    fn sha256_is_deterministic_and_known_length() {
        let a = sha256(b"hello");
        let b = sha256(b"hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        // RFC sanity check: sha256("") starts with e3b0c44...
        let empty = sha256(b"");
        assert_eq!(hex::encode(empty)[..7], *"e3b0c44");
    }

    #[test]
    fn signed_attestation_verifies() {
        let kp = AgentKeypair::generate();
        let mut a = unsigned("84");
        kp.sign_attestation(&mut a);
        assert!(verify_attestation_signature(&a));
        assert_eq!(a.agent_pubkey, kp.public_bytes());
    }

    #[test]
    fn tampered_output_fails_verification() {
        let kp = AgentKeypair::generate();
        let mut a = unsigned("84");
        kp.sign_attestation(&mut a);
        a.output = "85".into();
        assert!(!verify_attestation_signature(&a));
    }

    #[test]
    fn tampered_signature_fails_verification() {
        let kp = AgentKeypair::generate();
        let mut a = unsigned("84");
        kp.sign_attestation(&mut a);
        a.signature[0] ^= 0xff;
        assert!(!verify_attestation_signature(&a));
    }

    #[test]
    fn cross_keypair_signature_fails_verification() {
        let kp_a = AgentKeypair::generate();
        let kp_b = AgentKeypair::generate();
        let mut att = unsigned("84");
        kp_a.sign_attestation(&mut att);
        // Swap in a different agent's pubkey but keep the signature
        att.agent_pubkey = kp_b.public_bytes();
        assert!(!verify_attestation_signature(&att));
    }

    #[test]
    fn malformed_pubkey_fails_verification() {
        let kp = AgentKeypair::generate();
        let mut a = unsigned("84");
        kp.sign_attestation(&mut a);
        // Replace pubkey with random bytes that are extremely unlikely to lie
        // on the ed25519 prime-order subgroup; even if they did, the signature
        // would not match.
        a.agent_pubkey = [0xff; 32];
        assert!(!verify_attestation_signature(&a));
    }

    #[test]
    fn round_trip_through_json() {
        let kp = AgentKeypair::generate();
        let mut a = unsigned("84");
        kp.sign_attestation(&mut a);
        let s = serde_json::to_string(&a).unwrap();
        let b: Attestation = serde_json::from_str(&s).unwrap();
        assert!(verify_attestation_signature(&b));
    }
}
