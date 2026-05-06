//! `Attestation` — the unit of signed work in AWP.
//!
//! An attestation is a record that an agent (Worker or Verifier) produced a
//! particular output for a particular task, signed with the agent's
//! ed25519 key. The struct mirrors the "Data Structures" section of
//! `planning/awp-prototype-plan.md` exactly; trace/zk fields from
//! Appendix A are deliberately deferred per that doc's Recommendation.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::signing::verify_attestation_signature;

/// All possible terminal states for an attestation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttestationStatus {
    Completed,
    Failed {
        reason: String,
    },
    Verified {
        /// Cryptographic check: signature + hashes verified.
        attestation_valid: bool,
        /// Semantic check: the answer is actually correct.
        answer_correct: bool,
    },
}

/// A signed attestation for a single agent task execution.
///
/// `signature` is populated by [`crate::signing::AgentKeypair::sign_attestation`]
/// and covers everything else in the struct via [`Attestation::signing_payload`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    pub id: Uuid,
    pub agent_id: String,
    #[serde(with = "hex_array_32")]
    pub agent_pubkey: [u8; 32],

    #[serde(with = "hex_array_32")]
    pub task_hash: [u8; 32],
    #[serde(with = "hex_array_32")]
    pub output_hash: [u8; 32],
    pub output: String,

    pub status: AttestationStatus,

    /// For verifiers: the id of the attestation being verified.
    pub references: Option<Uuid>,

    pub timestamp: i64,

    #[serde(with = "hex_array_64")]
    pub signature: [u8; 64],
}

impl Attestation {
    /// Construct an unsigned attestation. The caller must populate
    /// `agent_pubkey` and `signature` via
    /// [`crate::signing::AgentKeypair::sign_attestation`] before persisting.
    pub fn new(
        agent_id: impl Into<String>,
        task_hash: [u8; 32],
        output: impl Into<String>,
        status: AttestationStatus,
        references: Option<Uuid>,
        timestamp: i64,
    ) -> Self {
        let output: String = output.into();
        let output_hash = crate::signing::sha256(output.as_bytes());
        Self {
            id: Uuid::new_v4(),
            agent_id: agent_id.into(),
            agent_pubkey: [0u8; 32],
            task_hash,
            output_hash,
            output,
            status,
            references,
            timestamp,
            signature: [0u8; 64],
        }
    }

    /// Canonical bytes covered by the agent's signature. Includes every field
    /// except `signature` itself, serialized as a deterministic JSON view so
    /// the bytes are stable across runs and Rust toolchains.
    pub fn signing_payload(&self) -> Vec<u8> {
        let view = SigningPayload {
            id: &self.id,
            agent_id: &self.agent_id,
            agent_pubkey: &self.agent_pubkey,
            task_hash: &self.task_hash,
            output_hash: &self.output_hash,
            output: &self.output,
            status: &self.status,
            references: &self.references,
            timestamp: self.timestamp,
        };
        // serde_json preserves struct field order, giving us a deterministic
        // canonical encoding without an extra dependency.
        serde_json::to_vec(&view).expect("SigningPayload serialization is infallible")
    }
}

#[derive(Serialize)]
struct SigningPayload<'a> {
    id: &'a Uuid,
    agent_id: &'a str,
    #[serde(serialize_with = "ser_hex_32_ref")]
    agent_pubkey: &'a [u8; 32],
    #[serde(serialize_with = "ser_hex_32_ref")]
    task_hash: &'a [u8; 32],
    #[serde(serialize_with = "ser_hex_32_ref")]
    output_hash: &'a [u8; 32],
    output: &'a str,
    status: &'a AttestationStatus,
    references: &'a Option<Uuid>,
    timestamp: i64,
}

fn ser_hex_32_ref<S>(bytes: &&[u8; 32], ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    ser.serialize_str(&hex::encode(*bytes))
}

/// Errors raised by attestation persistence.
#[derive(Debug, Error)]
pub enum AttestationError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Append an attestation to the JSON-lines log at `path`. Creates the file if
/// missing. Each line is a single JSON-encoded [`Attestation`] — concatenation
/// is the canonical multi-attestation format for AWP's append-only log.
pub fn append_attestation(path: &Path, a: &Attestation) -> Result<(), AttestationError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(a)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Load every attestation from `path`, verifying each signature on the way
/// in. Invalid records are skipped with a warning printed to stderr; the
/// returned vec contains only cryptographically-valid attestations.
pub fn load_attestations(path: &Path) -> Result<Vec<Attestation>, AttestationError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let att: Attestation = match serde_json::from_str(&line) {
            Ok(a) => a,
            Err(e) => {
                eprintln!(
                    "warn: skipping attestation at {}:{} — parse error: {e}",
                    path.display(),
                    lineno + 1
                );
                continue;
            }
        };
        if !verify_attestation_signature(&att) {
            eprintln!(
                "warn: skipping attestation at {}:{} — invalid signature (id={})",
                path.display(),
                lineno + 1,
                att.id
            );
            continue;
        }
        out.push(att);
    }
    Ok(out)
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

mod hex_array_64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(de: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(de)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        v.try_into()
            .map_err(|_| serde::de::Error::custom("expected 64 bytes hex"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{sha256, AgentKeypair};
    use tempfile::tempdir;

    fn make_completed_attestation(keypair: &AgentKeypair, output: &str) -> Attestation {
        let task_hash = sha256(b"compute 140 * 0.6");
        let mut a = Attestation::new(
            "test-agent",
            task_hash,
            output,
            AttestationStatus::Completed,
            None,
            1_700_000_000,
        );
        keypair.sign_attestation(&mut a);
        a
    }

    #[test]
    fn output_hash_matches_sha256_of_output_bytes() {
        let kp = AgentKeypair::generate();
        let a = make_completed_attestation(&kp, "84");
        assert_eq!(a.output_hash, sha256(b"84"));
    }

    #[test]
    fn signing_payload_excludes_signature() {
        let kp = AgentKeypair::generate();
        let mut a = make_completed_attestation(&kp, "84");
        let payload_before = a.signing_payload();
        a.signature = [9u8; 64];
        let payload_after = a.signing_payload();
        assert_eq!(payload_before, payload_after);
    }

    #[test]
    fn signing_payload_is_deterministic() {
        let kp = AgentKeypair::generate();
        let a = make_completed_attestation(&kp, "84");
        let p1 = a.signing_payload();
        let p2 = a.signing_payload();
        assert_eq!(p1, p2);
    }

    #[test]
    fn signing_payload_changes_when_output_changes() {
        let kp = AgentKeypair::generate();
        let a = make_completed_attestation(&kp, "84");
        let mut b = a.clone();
        b.output = "85".into();
        b.output_hash = sha256(b"85");
        assert_ne!(a.signing_payload(), b.signing_payload());
    }

    #[test]
    fn round_trip_serde_preserves_signature() {
        let kp = AgentKeypair::generate();
        let a = make_completed_attestation(&kp, "84");
        let s = serde_json::to_string(&a).unwrap();
        let b: Attestation = serde_json::from_str(&s).unwrap();
        assert_eq!(a, b);
        assert!(verify_attestation_signature(&b));
    }

    #[test]
    fn append_then_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("attestations.json");
        let kp = AgentKeypair::generate();
        let a1 = make_completed_attestation(&kp, "84");
        let a2 = make_completed_attestation(&kp, "126");

        append_attestation(&path, &a1).unwrap();
        append_attestation(&path, &a2).unwrap();

        let loaded = load_attestations(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], a1);
        assert_eq!(loaded[1], a2);
    }

    #[test]
    fn load_skips_tampered_records() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("attestations.json");
        let kp = AgentKeypair::generate();
        let a1 = make_completed_attestation(&kp, "84");
        let mut a2 = make_completed_attestation(&kp, "126");

        append_attestation(&path, &a1).unwrap();
        // Tamper: mutate output after signing — signature no longer covers payload
        a2.output = "evil".into();
        append_attestation(&path, &a2).unwrap();

        let loaded = load_attestations(&path).unwrap();
        assert_eq!(loaded.len(), 1, "tampered record must be dropped");
        assert_eq!(loaded[0], a1);
    }

    #[test]
    fn load_returns_empty_for_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        let loaded = load_attestations(&path).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn verified_status_round_trips() {
        let kp = AgentKeypair::generate();
        let task_hash = sha256(b"task");
        let mut a = Attestation::new(
            "verifier",
            task_hash,
            "84",
            AttestationStatus::Verified {
                attestation_valid: true,
                answer_correct: true,
            },
            Some(Uuid::new_v4()),
            1_700_000_000,
        );
        kp.sign_attestation(&mut a);
        let s = serde_json::to_string(&a).unwrap();
        let b: Attestation = serde_json::from_str(&s).unwrap();
        assert_eq!(a, b);
    }
}
