//! ed25519 keypair management and SHA-256 helpers for Attestly attestations.
//!
//! Keys can either be generated ephemerally via [`AgentKeypair::generate`] or
//! constructed from raw 32-byte secrets via [`AgentKeypair::from_secret_bytes`]
//! — the latter is what [`crate::identity::FileIdentityStore`] uses to load
//! a persisted identity from disk. Production deployments should prefer an
//! HSM- or KMS-backed [`crate::identity::IdentityStore`] over the file-based
//! store; see `crate::identity` for guidance.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::attestation::Attestation;

/// SHA-256 of `bytes`, returned as a 32-byte array.
///
/// Used for both `task_hash` and `output_hash` on attestations.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Errors surfaced when reconstructing or validating an [`AgentKeypair`].
///
/// Keypair generation itself is infallible (the OS RNG is treated as a
/// trusted source); these only fire on the `from_secret_bytes` path.
#[derive(Debug, Error)]
pub enum KeypairError {
    /// The verifying key derived from the secret does not match the expected
    /// public key supplied by the caller. Indicates a tampered or mismatched
    /// identity file.
    #[error("public key derived from secret does not match expected public key")]
    PublicMismatch,
}

/// Wraps an ed25519 signing key plus its derived verifying key.
#[derive(Clone)]
pub struct AgentKeypair {
    signing: SigningKey,
    verifying: VerifyingKey,
}

// Manual Debug — never include the secret signing key in any debug output.
// Required so callers can `#[derive(Debug)]` types that embed an
// AgentKeypair (and so error variants that wrap the keypair can derive
// Debug for `unwrap_err` use in tests).
impl std::fmt::Debug for AgentKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentKeypair")
            .field("public", &hex::encode(self.verifying.to_bytes()))
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl AgentKeypair {
    /// Generate a fresh ephemeral keypair from the OS RNG.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// Reconstruct a keypair from the 32-byte ed25519 secret. The verifying
    /// key is rederived; if `expected_public` is provided we cross-check it
    /// against the rederived value and return [`KeypairError::PublicMismatch`]
    /// on disagreement (defence in depth against tampered identity files).
    pub fn from_secret_bytes(
        secret: &[u8; 32],
        expected_public: Option<&[u8; 32]>,
    ) -> Result<Self, KeypairError> {
        let signing = SigningKey::from_bytes(secret);
        let verifying = signing.verifying_key();
        if let Some(expected) = expected_public {
            if &verifying.to_bytes() != expected {
                return Err(KeypairError::PublicMismatch);
            }
        }
        Ok(Self { signing, verifying })
    }

    /// The 32-byte ed25519 secret. Treat with care — this is the secret
    /// material that any consumer must persist behind the same protections as
    /// any other private key.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing.to_bytes()
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
/// (see `attestly-agents::tools`).
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
    fn keypair_round_trips_through_secret_bytes() {
        let kp = AgentKeypair::generate();
        let secret = kp.secret_bytes();
        let public = kp.public_bytes();

        let restored = AgentKeypair::from_secret_bytes(&secret, Some(&public)).unwrap();
        assert_eq!(restored.public_bytes(), public);

        // Signatures from the restored keypair must verify under the original
        // public key.
        let mut a = unsigned("84");
        restored.sign_attestation(&mut a);
        assert!(verify_attestation_signature(&a));
        assert_eq!(a.agent_pubkey, public);
    }

    #[test]
    fn from_secret_bytes_rejects_mismatched_public() {
        let kp_a = AgentKeypair::generate();
        let kp_b = AgentKeypair::generate();
        let err = AgentKeypair::from_secret_bytes(&kp_a.secret_bytes(), Some(&kp_b.public_bytes()))
            .unwrap_err();
        assert!(matches!(err, KeypairError::PublicMismatch));
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
