//! SQLite persistence for Attestly attestations, batches, and inclusion proofs.
//!
//! Phase 3 introduces SQLite as the source of truth for Merkle batching,
//! while the Phase 1/2 JSON logs remain authoritative for the Worker /
//! Verifier / Dispatcher flow (see `attestation.rs`, `execution.rs`).
//! The two persistence layers are deliberately not unified yet — the
//! plan only commits to "All data persisted to SQLite" for *batched*
//! data; collapsing the JSON logs into SQLite is a Phase 4+ decision.
//!
//! ## Schema
//!
//! Three tables, normalised on `id`:
//!
//! - `batches` — one row per Merkle batch
//! - `attestations` — every attestation; `batch_id` is `NULL` until the
//!   attestation has been batched
//! - `proofs` — one row per attestation in a batch, holding the
//!   inclusion-proof path as a serde-JSON BLOB. Stored, not regenerated,
//!   to preserve the exit-criterion that "verification works given only
//!   root + proof + attestation" — the proof is durable, not derived.
//!
//! All multi-row writes use `BEGIN IMMEDIATE` transactions so a partial
//! batch never lands.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::attestation::{Attestation, AttestationStatus};
use crate::merkle::{verify_inclusion, Batch, InclusionProof};
use crate::signing::{sha256, verify_attestation_signature};

/// Errors raised by the SQLite storage layer.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("attestation {0} not found")]
    AttestationNotFound(Uuid),
    #[error("batch {0} not found")]
    BatchNotFound(Uuid),
    #[error("proof for attestation {0} not found")]
    ProofNotFound(Uuid),
    #[error("attestation {0} is not yet part of a batch")]
    AttestationNotBatched(Uuid),
    #[error("malformed status payload for attestation {0}: {1}")]
    MalformedStatus(Uuid, String),
}

/// Owns a SQLite [`Connection`] and applies the Attestly schema on first use.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open (or create) a SQLite database at `path`, applying the Attestly
    /// schema with `CREATE TABLE IF NOT EXISTS`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        Self::apply_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database — used by tests and short-lived tools.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::apply_schema(&conn)?;
        Ok(Self { conn })
    }

    fn apply_schema(conn: &Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS batches (
                id                  TEXT PRIMARY KEY,
                merkle_root         BLOB NOT NULL,
                attestation_count   INTEGER NOT NULL,
                created_at          INTEGER NOT NULL,
                anchor_tx           TEXT,
                anchor_chain        TEXT
            );

            CREATE TABLE IF NOT EXISTS attestations (
                id              TEXT PRIMARY KEY,
                agent_id        TEXT NOT NULL,
                agent_pubkey    BLOB NOT NULL,
                task_hash       BLOB NOT NULL,
                output_hash     BLOB NOT NULL,
                output          TEXT NOT NULL,
                status          TEXT NOT NULL,
                "references"    TEXT,
                timestamp       INTEGER NOT NULL,
                signature       BLOB NOT NULL,
                batch_id        TEXT REFERENCES batches(id)
            );

            CREATE INDEX IF NOT EXISTS idx_attestations_batch_id
                ON attestations(batch_id);

            CREATE TABLE IF NOT EXISTS proofs (
                attestation_id  TEXT PRIMARY KEY REFERENCES attestations(id),
                batch_id        TEXT NOT NULL REFERENCES batches(id),
                proof_path      BLOB NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    /// Insert an attestation with `batch_id = NULL`. Idempotent on `id`:
    /// re-inserting the same attestation is a no-op (`INSERT OR IGNORE`).
    pub fn insert_attestation(&mut self, att: &Attestation) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        insert_attestation_in_tx(&tx, att)?;
        tx.commit()?;
        Ok(())
    }

    /// Insert a batch and all its inclusion proofs in a single transaction.
    ///
    /// Each attestation in `attestations` is upserted (so the Batcher can
    /// submit attestations that were never previously inserted) and then
    /// linked to the new batch via `batch_id`. The matching
    /// [`InclusionProof`] for each attestation lands in `proofs`. Length
    /// of `attestations` and `proofs` must agree and match
    /// `batch.attestation_count` — those invariants are enforced.
    pub fn insert_batch_with_proofs(
        &mut self,
        batch: &Batch,
        attestations: &[Attestation],
        proofs: &[InclusionProof],
    ) -> Result<(), StorageError> {
        debug_assert_eq!(
            attestations.len(),
            proofs.len(),
            "attestation and proof slices must align"
        );
        debug_assert_eq!(
            attestations.len() as u32,
            batch.attestation_count,
            "batch.attestation_count must match payload"
        );

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO batches (id, merkle_root, attestation_count, created_at, anchor_tx, anchor_chain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                batch.id.to_string(),
                batch.merkle_root.as_slice(),
                batch.attestation_count,
                batch.created_at,
                batch.anchor_tx,
                batch.anchor_chain,
            ],
        )?;
        for (att, proof) in attestations.iter().zip(proofs.iter()) {
            insert_attestation_in_tx(&tx, att)?;
            tx.execute(
                "UPDATE attestations SET batch_id = ?1 WHERE id = ?2",
                params![batch.id.to_string(), att.id.to_string()],
            )?;
            let proof_blob = serde_json::to_vec(&proof.proof_path)?;
            tx.execute(
                "INSERT INTO proofs (attestation_id, batch_id, proof_path)
                 VALUES (?1, ?2, ?3)",
                params![
                    proof.attestation_id.to_string(),
                    batch.id.to_string(),
                    proof_blob
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Fetch a single attestation by id. Returns
    /// `Err(StorageError::AttestationNotFound)` if no row exists.
    pub fn get_attestation(&self, id: Uuid) -> Result<Attestation, StorageError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, agent_id, agent_pubkey, task_hash, output_hash, output, status,
                        \"references\", timestamp, signature
                 FROM attestations WHERE id = ?1",
                params![id.to_string()],
                row_to_attestation,
            )
            .optional()?;
        row.ok_or(StorageError::AttestationNotFound(id))
    }

    /// Fetch a batch by id.
    pub fn get_batch(&self, id: Uuid) -> Result<Batch, StorageError> {
        let attestation_ids: Vec<Uuid> = self
            .conn
            .prepare("SELECT id FROM attestations WHERE batch_id = ?1 ORDER BY rowid")?
            .query_map(params![id.to_string()], |r| {
                let s: String = r.get(0)?;
                Uuid::parse_str(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let row = self
            .conn
            .query_row(
                "SELECT id, merkle_root, attestation_count, created_at, anchor_tx, anchor_chain
                 FROM batches WHERE id = ?1",
                params![id.to_string()],
                |r| {
                    let id_str: String = r.get(0)?;
                    let root_blob: Vec<u8> = r.get(1)?;
                    let count: u32 = r.get(2)?;
                    let created_at: i64 = r.get(3)?;
                    let anchor_tx: Option<String> = r.get(4)?;
                    let anchor_chain: Option<String> = r.get(5)?;
                    Ok((
                        id_str,
                        root_blob,
                        count,
                        created_at,
                        anchor_tx,
                        anchor_chain,
                    ))
                },
            )
            .optional()?;

        let (id_str, root_blob, count, created_at, anchor_tx, anchor_chain) =
            row.ok_or(StorageError::BatchNotFound(id))?;
        let merkle_root: [u8; 32] = root_blob.as_slice().try_into().map_err(|_| {
            StorageError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Blob,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "merkle_root must be 32 bytes",
                )),
            ))
        })?;
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            StorageError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))
        })?;
        Ok(Batch {
            id,
            merkle_root,
            attestation_ids,
            attestation_count: count,
            created_at,
            anchor_tx,
            anchor_chain,
        })
    }

    /// Fetch the inclusion proof for a single attestation, joined to the
    /// batch row so the caller has the leaf hash and root in hand.
    pub fn get_proof(&self, attestation_id: Uuid) -> Result<InclusionProof, StorageError> {
        let row = self
            .conn
            .query_row(
                "SELECT batch_id, proof_path FROM proofs WHERE attestation_id = ?1",
                params![attestation_id.to_string()],
                |r| {
                    let batch_id: String = r.get(0)?;
                    let proof_blob: Vec<u8> = r.get(1)?;
                    Ok((batch_id, proof_blob))
                },
            )
            .optional()?;
        let (batch_id_str, proof_blob) = row.ok_or(StorageError::ProofNotFound(attestation_id))?;
        let batch_id = Uuid::parse_str(&batch_id_str).map_err(|e| {
            StorageError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))
        })?;
        let proof_path = serde_json::from_slice(&proof_blob)?;
        let attestation = self.get_attestation(attestation_id)?;
        let attestation_hash = crate::merkle::attestation_leaf_hash(&attestation);
        Ok(InclusionProof {
            attestation_id,
            attestation_hash,
            batch_id,
            proof_path,
        })
    }

    /// Cryptographically verify that `attestation_id` is a member of its
    /// recorded batch — using only the stored Merkle root and proof, plus
    /// the attestation itself reconstructed from the database. Returns
    /// `false` for any mismatch.
    pub fn verify_attestation_inclusion(&self, attestation_id: Uuid) -> Result<bool, StorageError> {
        let attestation = self.get_attestation(attestation_id)?;
        // Re-derive the leaf hash from the attestation we just loaded; if
        // somebody tampered with the row, the hash won't match the proof's.
        let recomputed_leaf = crate::merkle::attestation_leaf_hash(&attestation);
        // Also independently re-check the signature. A forged row could have
        // a self-consistent leaf but an invalid signature; report the
        // tampering as a verification failure rather than silently passing.
        if !verify_attestation_signature(&attestation) {
            return Ok(false);
        }
        // And re-check the output_hash field matches the stored output
        // (Phase 1 invariant — the verifier tool's "hash_valid" check).
        if sha256(attestation.output.as_bytes()) != attestation.output_hash {
            return Ok(false);
        }

        let batch_id_opt: Option<String> = self.conn.query_row(
            "SELECT batch_id FROM attestations WHERE id = ?1",
            params![attestation_id.to_string()],
            |r| r.get(0),
        )?;
        let Some(batch_id_str) = batch_id_opt else {
            return Err(StorageError::AttestationNotBatched(attestation_id));
        };
        let batch_id = Uuid::parse_str(&batch_id_str).map_err(|e| {
            StorageError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))
        })?;

        let proof = self.get_proof(attestation_id)?;
        let batch = self.get_batch(batch_id)?;

        if proof.attestation_hash != recomputed_leaf {
            return Ok(false);
        }
        Ok(verify_inclusion(
            &batch.merkle_root,
            &recomputed_leaf,
            &proof,
        ))
    }
}

fn insert_attestation_in_tx(tx: &Transaction<'_>, att: &Attestation) -> Result<(), StorageError> {
    let status_json = serde_json::to_string(&att.status)?;
    tx.execute(
        "INSERT OR IGNORE INTO attestations
            (id, agent_id, agent_pubkey, task_hash, output_hash, output,
             status, \"references\", timestamp, signature, batch_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        params![
            att.id.to_string(),
            att.agent_id,
            att.agent_pubkey.as_slice(),
            att.task_hash.as_slice(),
            att.output_hash.as_slice(),
            att.output,
            status_json,
            att.references.map(|u| u.to_string()),
            att.timestamp,
            att.signature.as_slice(),
        ],
    )?;
    Ok(())
}

fn row_to_attestation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attestation> {
    let id_str: String = row.get(0)?;
    let id = Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let agent_id: String = row.get(1)?;
    let agent_pubkey_blob: Vec<u8> = row.get(2)?;
    let task_hash_blob: Vec<u8> = row.get(3)?;
    let output_hash_blob: Vec<u8> = row.get(4)?;
    let output: String = row.get(5)?;
    let status_str: String = row.get(6)?;
    let references_str: Option<String> = row.get(7)?;
    let timestamp: i64 = row.get(8)?;
    let signature_blob: Vec<u8> = row.get(9)?;

    let to_32 = |blob: Vec<u8>, col: usize| -> rusqlite::Result<[u8; 32]> {
        blob.as_slice().try_into().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                col,
                rusqlite::types::Type::Blob,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "expected 32 bytes",
                )),
            )
        })
    };
    let to_64 = |blob: Vec<u8>, col: usize| -> rusqlite::Result<[u8; 64]> {
        blob.as_slice().try_into().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                col,
                rusqlite::types::Type::Blob,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "expected 64 bytes",
                )),
            )
        })
    };

    let status: AttestationStatus = serde_json::from_str(&status_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let references = match references_str {
        Some(s) => Some(Uuid::parse_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };

    Ok(Attestation {
        id,
        agent_id,
        agent_pubkey: to_32(agent_pubkey_blob, 2)?,
        task_hash: to_32(task_hash_blob, 3)?,
        output_hash: to_32(output_hash_blob, 4)?,
        output,
        status,
        references,
        timestamp,
        signature: to_64(signature_blob, 9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::AttestationStatus;
    use crate::merkle::{attestation_leaf_hash, build_tree, inclusion_proof};
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

    fn make_batch(attestations: &[Attestation], clock: i64) -> (Batch, Vec<InclusionProof>) {
        let leaves: Vec<[u8; 32]> = attestations.iter().map(attestation_leaf_hash).collect();
        let tree = build_tree(&leaves).unwrap();
        let batch_id = Uuid::new_v4();
        let proofs: Vec<InclusionProof> = attestations
            .iter()
            .enumerate()
            .map(|(i, a)| inclusion_proof(&tree, i, a.id, batch_id).unwrap())
            .collect();
        let batch = Batch {
            id: batch_id,
            merkle_root: tree.root().unwrap(),
            attestation_ids: attestations.iter().map(|a| a.id).collect(),
            attestation_count: attestations.len() as u32,
            created_at: clock,
            anchor_tx: None,
            anchor_chain: None,
        };
        (batch, proofs)
    }

    #[test]
    fn insert_and_get_attestation_round_trip() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        s.insert_attestation(&a).unwrap();
        let back = s.get_attestation(a.id).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn missing_attestation_yields_not_found() {
        let s = Storage::open_in_memory().unwrap();
        let id = Uuid::new_v4();
        assert!(matches!(
            s.get_attestation(id),
            Err(StorageError::AttestationNotFound(_))
        ));
    }

    #[test]
    fn insert_attestation_is_idempotent() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        s.insert_attestation(&a).unwrap();
        // Second insert is a no-op (INSERT OR IGNORE).
        s.insert_attestation(&a).unwrap();
        assert_eq!(s.get_attestation(a.id).unwrap(), a);
    }

    #[test]
    fn batch_with_proofs_round_trip() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let atts: Vec<Attestation> = (0..4).map(|i| signed(&kp, &format!("o{i}"))).collect();
        let (batch, proofs) = make_batch(&atts, 1_700_000_000);

        s.insert_batch_with_proofs(&batch, &atts, &proofs).unwrap();

        let back = s.get_batch(batch.id).unwrap();
        assert_eq!(back.merkle_root, batch.merkle_root);
        assert_eq!(back.attestation_count, 4);
        // Attestation ids are loaded in insertion order via rowid.
        assert_eq!(back.attestation_ids.len(), 4);
        for a in &atts {
            assert!(back.attestation_ids.contains(&a.id));
        }
        for (i, p) in proofs.iter().enumerate() {
            let stored = s.get_proof(atts[i].id).unwrap();
            assert_eq!(stored.attestation_id, atts[i].id);
            assert_eq!(stored.proof_path, p.proof_path);
        }
    }

    #[test]
    fn verify_inclusion_succeeds_for_each_batched_attestation() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let atts: Vec<Attestation> = (0..6).map(|i| signed(&kp, &format!("o{i}"))).collect();
        let (batch, proofs) = make_batch(&atts, 1_700_000_000);
        s.insert_batch_with_proofs(&batch, &atts, &proofs).unwrap();
        for a in &atts {
            assert!(s.verify_attestation_inclusion(a.id).unwrap());
        }
    }

    #[test]
    fn verify_inclusion_fails_for_unbatched_attestation() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        s.insert_attestation(&a).unwrap();
        assert!(matches!(
            s.verify_attestation_inclusion(a.id),
            Err(StorageError::AttestationNotBatched(_))
        ));
    }

    #[test]
    fn tampering_with_stored_attestation_makes_inclusion_fail() {
        // Direct DB tampering: simulate an attacker who alters the
        // attestations.output column after the batch is sealed. The
        // recomputed leaf no longer matches the stored proof, so
        // verify_attestation_inclusion must return false.
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let atts: Vec<Attestation> = (0..3).map(|i| signed(&kp, &format!("o{i}"))).collect();
        let (batch, proofs) = make_batch(&atts, 1_700_000_000);
        s.insert_batch_with_proofs(&batch, &atts, &proofs).unwrap();

        let target = atts[1].id;
        s.conn
            .execute(
                "UPDATE attestations SET output = 'evil' WHERE id = ?1",
                params![target.to_string()],
            )
            .unwrap();

        assert!(!s
            .verify_attestation_inclusion(target)
            .expect("verify call itself should not error"));
    }

    #[test]
    fn missing_batch_yields_not_found() {
        let s = Storage::open_in_memory().unwrap();
        assert!(matches!(
            s.get_batch(Uuid::new_v4()),
            Err(StorageError::BatchNotFound(_))
        ));
    }

    #[test]
    fn missing_proof_yields_not_found() {
        let s = Storage::open_in_memory().unwrap();
        assert!(matches!(
            s.get_proof(Uuid::new_v4()),
            Err(StorageError::ProofNotFound(_))
        ));
    }

    #[test]
    fn verified_status_round_trips_through_sqlite() {
        let mut s = Storage::open_in_memory().unwrap();
        let kp = AgentKeypair::generate();
        let mut a = Attestation::new(
            "verifier",
            sha256(b"task"),
            "ok",
            AttestationStatus::Verified {
                attestation_valid: true,
                answer_correct: true,
            },
            Some(Uuid::new_v4()),
            1_700_000_000,
        );
        kp.sign_attestation(&mut a);
        s.insert_attestation(&a).unwrap();
        assert_eq!(s.get_attestation(a.id).unwrap(), a);
    }

    #[test]
    fn opens_database_at_filesystem_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("attestly.db");
        let mut s = Storage::open(&path).unwrap();
        let kp = AgentKeypair::generate();
        let a = signed(&kp, "84");
        s.insert_attestation(&a).unwrap();
        drop(s);
        let s2 = Storage::open(&path).unwrap();
        assert_eq!(s2.get_attestation(a.id).unwrap(), a);
    }
}
