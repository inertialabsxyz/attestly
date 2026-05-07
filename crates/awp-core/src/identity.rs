//! Persistent agent identity — store and reload an agent's signing key
//! across process restarts.
//!
//! ## Why this exists
//!
//! Phase 1 of AWP (the prototype plan in `planning/awp-prototype-plan.md`)
//! deferred persistent identity: every agent generates a fresh ephemeral
//! keypair on startup. That is fine for arithmetic demos but breaks any
//! settlement system that wants to recognise an agent across runs — the
//! `agent_pubkey` on every attestation rotates whenever the process is
//! restarted. GTM Phase 1 → Step 3 closes that gap so the KYC receipts
//! demo can produce two consecutive runs whose attestations share the same
//! `agent_pubkey` for matching agent IDs. See `planning/gtm-phase-1-plan.md`
//! → "Step 3 — Persistent Agent Identity" for the spec.
//!
//! ## On-disk format
//!
//! [`FileIdentityStore`] writes one JSON file per agent at
//! `<dir>/<agent_id>.json`:
//!
//! ```json
//! {
//!   "agent_id": "agent-kyc-01",
//!   "secret_key": "<64 hex chars — 32 bytes of ed25519 secret>",
//!   "public_key": "<64 hex chars — 32 bytes of ed25519 verifying key>"
//! }
//! ```
//!
//! Both keys are hex-encoded raw 32-byte ed25519 material. The `public_key`
//! field is redundant (it can be rederived from the secret) but is included
//! so an audit reader can see the identity without instantiating cryptography
//! and so [`AgentKeypair::from_secret_bytes`] can sanity-check the file
//! against tampering.
//!
//! ## File permissions
//!
//! On Unix, each saved file has mode `0600` (owner read/write only). On
//! Windows there is no equivalent enforcement; the first save on Windows
//! emits a warning to stderr and the module-level documentation flags the
//! gap. If the file is loaded on Unix with permissions broader than `0600`,
//! we log a warning to stderr but still load — flagging the misconfiguration
//! while keeping the demo runnable.
//!
//! ## Atomicity
//!
//! Saves write to `<dir>/<agent_id>.json.tmp.<pid>.<rand>` first and then
//! `rename` into place, so a crash mid-write cannot leave a partial JSON
//! file that would later fail to parse.
//!
//! ## Security note
//!
//! **The file-based store is appropriate for development and demo use only.**
//! It writes raw private keys to disk in hex, protected only by filesystem
//! permissions. Production deployments should use OS keychain, an HSM, or a
//! KMS-backed [`IdentityStore`]. The trait is the integration point:
//!
//! ```ignore
//! // Production example skeleton:
//! // struct KmsIdentityStore { client: KmsClient }
//! // impl IdentityStore for KmsIdentityStore {
//! //     fn load(&self, agent_id: &str) -> Result<Option<AgentIdentity>> { ... }
//! //     fn save(&self, identity: &AgentIdentity) -> Result<()>           { ... }
//! // }
//! ```
//!
//! `KmsIdentityStore::load` would fetch the agent's wrapped key by alias and
//! `KmsIdentityStore::save` would either generate the key inside the KMS or
//! upload it under a CMK — never round-tripping the raw secret through this
//! process's heap the way [`FileIdentityStore`] does.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
#[cfg(windows)]
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::signing::{AgentKeypair, KeypairError};

/// An agent's stable identity — a string ID plus the keypair it signs with.
#[derive(Clone, Debug)]
pub struct AgentIdentity {
    pub agent_id: String,
    pub keypair: AgentKeypair,
}

impl AgentIdentity {
    /// Generate a fresh ephemeral identity. Used by `Worker::new` /
    /// `Verifier::new` to preserve their pre-Step-3 backwards-compatible
    /// behaviour: no persistence, new keypair every call.
    pub fn generate(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            keypair: AgentKeypair::generate(),
        }
    }

    /// Return the existing identity for `agent_id` from `store` if present,
    /// otherwise generate a fresh one, save it through the store, and
    /// return that.
    pub fn load_or_create(
        store: &dyn IdentityStore,
        agent_id: &str,
    ) -> Result<Self, IdentityError> {
        if let Some(existing) = store.load(agent_id)? {
            return Ok(existing);
        }
        let fresh = Self::generate(agent_id);
        store.save(&fresh)?;
        Ok(fresh)
    }

    /// 32-byte verifying key (the bytes that end up on
    /// `Attestation::agent_pubkey`). Convenience accessor so callers don't
    /// need to reach through `keypair`.
    pub fn public_bytes(&self) -> [u8; 32] {
        self.keypair.public_bytes()
    }
}

/// Errors surfaced by [`IdentityStore`] implementations and
/// [`AgentIdentity::load_or_create`].
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("identity file is malformed: {0}")]
    Malformed(#[from] serde_json::Error),

    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("identity key bytes are the wrong length: expected {expected}, got {got}")]
    KeyLength { expected: usize, got: usize },

    #[error(transparent)]
    Keypair(#[from] KeypairError),
}

/// Pluggable persistence for [`AgentIdentity`]. The file-based implementation
/// in this crate is for development and demo use; production callers should
/// implement this against an HSM, KMS, or OS keychain — see the module-level
/// rustdoc for an example skeleton.
pub trait IdentityStore {
    /// Return the identity for `agent_id` if one is stored. `Ok(None)` means
    /// "no record yet" — distinct from an `Err` reporting a tampered or
    /// unreadable record.
    fn load(&self, agent_id: &str) -> Result<Option<AgentIdentity>, IdentityError>;

    /// Persist the identity. Implementations must be atomic with respect to
    /// concurrent readers (no partial writes) and must protect the secret
    /// material per the implementation's contract.
    fn save(&self, identity: &AgentIdentity) -> Result<(), IdentityError>;
}

/// On-disk wire shape used by [`FileIdentityStore`]. Public only so other
/// crates that want to interoperate with the same file format (e.g. a future
/// CLI) don't have to reverse-engineer the JSON layout.
#[derive(Debug, Serialize, Deserialize)]
struct IdentityFile {
    agent_id: String,
    /// Hex-encoded 32-byte ed25519 secret.
    secret_key: String,
    /// Hex-encoded 32-byte ed25519 verifying key. Redundant with `secret_key`
    /// (the public is rederivable) but kept on disk for human-readable
    /// auditability and to detect tampering.
    public_key: String,
}

/// File-backed [`IdentityStore`]. See the module-level documentation for the
/// on-disk format, file permissions, and security caveats.
pub struct FileIdentityStore {
    dir: PathBuf,
}

/// Tracks whether we have already warned about the missing-permissions story
/// on Windows. Guarded so the demo doesn't spew the same line on every save.
static WINDOWS_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

impl FileIdentityStore {
    /// Construct a store rooted at `dir`. The directory is *not* created
    /// eagerly — it is created on the first `save` so a read-only program
    /// (one that only ever calls `load`) doesn't accidentally side-effect
    /// the filesystem.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    fn path_for(&self, agent_id: &str) -> PathBuf {
        self.dir.join(format!("{agent_id}.json"))
    }
}

impl IdentityStore for FileIdentityStore {
    fn load(&self, agent_id: &str) -> Result<Option<AgentIdentity>, IdentityError> {
        let path = self.path_for(agent_id);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(IdentityError::Io(e)),
        };

        #[cfg(unix)]
        warn_if_permissions_too_loose(&path);

        let file: IdentityFile = serde_json::from_slice(&bytes)?;
        let secret = decode_key_bytes(&file.secret_key)?;
        let public = decode_key_bytes(&file.public_key)?;
        let keypair = AgentKeypair::from_secret_bytes(&secret, Some(&public))?;
        Ok(Some(AgentIdentity {
            agent_id: file.agent_id,
            keypair,
        }))
    }

    fn save(&self, identity: &AgentIdentity) -> Result<(), IdentityError> {
        fs::create_dir_all(&self.dir)?;

        let path = self.path_for(&identity.agent_id);
        let file = IdentityFile {
            agent_id: identity.agent_id.clone(),
            secret_key: hex::encode(identity.keypair.secret_bytes()),
            public_key: hex::encode(identity.keypair.public_bytes()),
        };
        let json = serde_json::to_vec_pretty(&file)?;

        let tmp = tmp_path_for(&path);
        write_atomic(&tmp, &path, &json)?;

        #[cfg(unix)]
        set_owner_only(&path)?;

        #[cfg(windows)]
        warn_about_windows_permissions_once();

        Ok(())
    }
}

fn tmp_path_for(final_path: &Path) -> PathBuf {
    let pid = std::process::id();
    let nonce: u64 = rand::random();
    let mut tmp = final_path.to_path_buf();
    let mut name = tmp
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default()
        .into_string()
        .unwrap_or_default();
    name.push_str(&format!(".tmp.{pid}.{nonce:016x}"));
    tmp.set_file_name(name);
    tmp
}

fn write_atomic(tmp: &Path, dest: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    fs::write(tmp, bytes)?;
    if let Err(e) = fs::rename(tmp, dest) {
        let _ = fs::remove_file(tmp);
        return Err(IdentityError::Io(e));
    }
    Ok(())
}

fn decode_key_bytes(hex_str: &str) -> Result<[u8; 32], IdentityError> {
    let raw = hex::decode(hex_str)?;
    if raw.len() != 32 {
        return Err(IdentityError::KeyLength {
            expected: 32,
            got: raw.len(),
        });
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(unix)]
fn warn_if_permissions_too_loose(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            eprintln!(
                "warning: identity file {} has permissions {:o}; expected 0600. \
                 Other users on this host may be able to read the secret key.",
                path.display(),
                mode
            );
        }
    }
}

#[cfg(windows)]
fn warn_about_windows_permissions_once() {
    if WINDOWS_WARNING_EMITTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        eprintln!(
            "warning: FileIdentityStore on Windows does not enforce file ACLs. \
             Identity files will be created with default ACLs. For production use, \
             implement an IdentityStore backed by the Windows credential manager, \
             DPAPI, or a KMS."
        );
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn _suppress_unused_atomic_on_unix() {
    let _ = &WINDOWS_WARNING_EMITTED;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_then_load_round_trips_keypair() {
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let original = AgentIdentity::generate("agent-x");
        let original_public = original.public_bytes();
        let original_secret = original.keypair.secret_bytes();

        store.save(&original).unwrap();

        let loaded = store.load("agent-x").unwrap().expect("identity exists");
        assert_eq!(loaded.agent_id, "agent-x");
        assert_eq!(loaded.public_bytes(), original_public);
        assert_eq!(loaded.keypair.secret_bytes(), original_secret);
    }

    #[test]
    fn load_returns_none_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());
        let result = store.load("nobody-home").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_or_create_creates_then_reloads_same_identity() {
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let first = AgentIdentity::load_or_create(&store, "agent-y").unwrap();
        let public = first.public_bytes();

        let second = AgentIdentity::load_or_create(&store, "agent-y").unwrap();
        assert_eq!(second.agent_id, "agent-y");
        assert_eq!(second.public_bytes(), public);

        // And a different agent_id should produce a distinct keypair.
        let other = AgentIdentity::load_or_create(&store, "agent-z").unwrap();
        assert_ne!(other.public_bytes(), public);
    }

    #[test]
    fn load_returns_err_on_malformed_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("agent-bad.json");
        fs::write(&path, b"not json").unwrap();

        let store = FileIdentityStore::new(dir.path());
        let err = store.load("agent-bad").unwrap_err();
        assert!(matches!(err, IdentityError::Malformed(_)));
    }

    #[test]
    fn load_returns_err_on_tampered_public_key() {
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let identity = AgentIdentity::generate("agent-tampered");
        store.save(&identity).unwrap();

        // Replace the public_key field with a different agent's public key
        // and confirm load detects the mismatch.
        let other_public = AgentIdentity::generate("decoy").public_bytes();
        let path = dir.path().join("agent-tampered.json");
        let raw = fs::read_to_string(&path).unwrap();
        let mut file: IdentityFile = serde_json::from_str(&raw).unwrap();
        file.public_key = hex::encode(other_public);
        fs::write(&path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();

        let err = store.load("agent-tampered").unwrap_err();
        assert!(matches!(err, IdentityError::Keypair(_)));
    }

    #[test]
    fn save_creates_directory_if_missing() {
        let parent = TempDir::new().unwrap();
        let nested = parent.path().join("does/not/exist/yet");
        let store = FileIdentityStore::new(&nested);

        let identity = AgentIdentity::generate("agent-deep");
        store.save(&identity).unwrap();
        assert!(nested.join("agent-deep.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_mode_0600_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let identity = AgentIdentity::generate("agent-perm");
        store.save(&identity).unwrap();

        let path = dir.path().join("agent-perm.json");
        let meta = fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected mode 0600, got {mode:o}");
    }

    #[test]
    fn signature_round_trips_after_load() {
        use crate::attestation::{Attestation, AttestationStatus};
        use crate::signing::{sha256, verify_attestation_signature};

        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let saved = AgentIdentity::generate("agent-sig");
        store.save(&saved).unwrap();

        let loaded = store.load("agent-sig").unwrap().unwrap();
        let mut att = Attestation::new(
            loaded.agent_id.clone(),
            sha256(b"task-bytes"),
            "84",
            AttestationStatus::Completed,
            None,
            1_700_000_000,
        );
        loaded.keypair.sign_attestation(&mut att);

        assert!(verify_attestation_signature(&att));
        assert_eq!(att.agent_pubkey, saved.public_bytes());
    }

    #[test]
    fn load_or_create_regenerates_after_file_deletion() {
        let dir = TempDir::new().unwrap();
        let store = FileIdentityStore::new(dir.path());

        let first = AgentIdentity::load_or_create(&store, "agent-rotating").unwrap();
        fs::remove_file(dir.path().join("agent-rotating.json")).unwrap();
        let second = AgentIdentity::load_or_create(&store, "agent-rotating").unwrap();
        assert_ne!(first.public_bytes(), second.public_bytes());
    }
}
