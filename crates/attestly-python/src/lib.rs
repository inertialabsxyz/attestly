//! PyO3 bindings for `attestly-core` — sign and verify Attestly attestations from Python.
//!
//! The signing path delegates byte-for-byte to `attestly-core` so that a Python
//! caller and a Rust caller producing the same logical payload always emit
//! the same `Attestation::signing_payload()` bytes and therefore the same
//! ed25519 signature. The cross-language self-test under `tests/` pins this
//! invariant against a checked-in test vector.

use std::path::PathBuf;

use attestly_core::attestation::{Attestation as CoreAttestation, AttestationStatus};
use attestly_core::identity::{AgentIdentity as CoreAgentIdentity, FileIdentityStore};
use attestly_core::signing::{sha256, verify_attestation_signature};
use pyo3::exceptions::{PyIOError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

/// Canonicalise a Python dict into the byte sequence that Attestly signs over.
///
/// The output is `serde_json::to_vec(&Value)` where `Value` is built from
/// the Python dict — keys are sorted alphabetically (serde_json's default
/// `BTreeMap` ordering) so the encoding is independent of Python insertion
/// order. The signing test vector under `tests/fixtures/` is computed
/// against this exact function.
fn py_to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(serde_json::Value::Bool(b.is_true()));
    }
    if let Ok(i) = obj.cast::<PyInt>() {
        let v: i64 = i.extract()?;
        return Ok(serde_json::Value::Number(v.into()));
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        let v: f64 = f.extract()?;
        let num = serde_json::Number::from_f64(v).ok_or_else(|| {
            PyValueError::new_err("payload contains a non-finite float (NaN/Inf)")
        })?;
        return Ok(serde_json::Value::Number(num));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        let v: String = s.extract()?;
        return Ok(serde_json::Value::String(v));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let mut out = Vec::with_capacity(list.len());
        for item in list.iter() {
            out.push(py_to_json_value(&item)?);
        }
        return Ok(serde_json::Value::Array(out));
    }
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        let mut out = Vec::with_capacity(tuple.len());
        for item in tuple.iter() {
            out.push(py_to_json_value(&item)?);
        }
        return Ok(serde_json::Value::Array(out));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k
                .cast::<PyString>()
                .map_err(|_| PyTypeError::new_err("payload dict keys must be strings"))?
                .extract()?;
            map.insert(key, py_to_json_value(&v)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    Err(PyTypeError::new_err(format!(
        "unsupported type in Attestly payload: {}",
        obj.get_type().name()?
    )))
}

/// Python-visible wrapper around `attestly_core::AgentIdentity`.
#[pyclass(
    module = "attestly._native",
    name = "AgentIdentity",
    skip_from_py_object
)]
#[derive(Clone)]
pub struct PyAgentIdentity {
    inner: CoreAgentIdentity,
}

#[pymethods]
impl PyAgentIdentity {
    /// Generate a fresh ephemeral identity from the OS RNG. The keypair is
    /// not persisted — use `load_or_create` if you want the identity to
    /// survive process restarts.
    #[staticmethod]
    fn generate(agent_id: &str) -> Self {
        Self {
            inner: CoreAgentIdentity::generate(agent_id),
        }
    }

    /// Load an identity for `agent_id` from a directory of JSON files, or
    /// generate and save a new one. Matches `attestly_core::AgentIdentity::load_or_create`
    /// against `FileIdentityStore::new(path)`.
    #[staticmethod]
    fn load_or_create(path: &str, agent_id: &str) -> PyResult<Self> {
        let store = FileIdentityStore::new(PathBuf::from(path));
        let inner = CoreAgentIdentity::load_or_create(&store, agent_id)
            .map_err(|e| PyIOError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Construct a deterministic identity from a 32-byte ed25519 secret.
    /// **Test-only.** Production callers must use `generate` or
    /// `load_or_create`; never embed a hard-coded seed in real code.
    #[staticmethod]
    fn from_secret_bytes(agent_id: &str, secret: &Bound<'_, PyBytes>) -> PyResult<Self> {
        let bytes = secret.as_bytes();
        if bytes.len() != 32 {
            return Err(PyValueError::new_err(format!(
                "secret must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(bytes);
        let keypair = attestly_core::signing::AgentKeypair::from_secret_bytes(&seed, None)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            inner: CoreAgentIdentity {
                agent_id: agent_id.to_string(),
                keypair,
            },
        })
    }

    /// Stable agent identifier (matches the `agent_id` field on attestations).
    #[getter]
    fn agent_id(&self) -> String {
        self.inner.agent_id.clone()
    }

    /// 32-byte ed25519 verifying key — the bytes that end up on
    /// `Attestation.agent_pubkey` and that callers pass to
    /// `verify_attestation`.
    #[getter]
    fn public_key(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(py, &self.inner.public_bytes()).into()
    }

    fn __repr__(&self) -> String {
        format!(
            "AgentIdentity(agent_id={:?}, public_key=0x{})",
            self.inner.agent_id,
            hex::encode(self.inner.public_bytes())
        )
    }
}

/// Python-visible wrapper around `attestly_core::Attestation`. Field access goes
/// through `#[getter]` so the underlying Rust struct stays immutable from
/// Python — tampering after signing would invalidate the signature anyway.
#[pyclass(module = "attestly._native", name = "Attestation", skip_from_py_object)]
#[derive(Clone)]
pub struct PyAttestation {
    inner: CoreAttestation,
}

#[pymethods]
impl PyAttestation {
    #[getter]
    fn id(&self) -> String {
        self.inner.id.to_string()
    }

    #[getter]
    fn agent_id(&self) -> String {
        self.inner.agent_id.clone()
    }

    #[getter]
    fn agent_pubkey<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.agent_pubkey)
    }

    #[getter]
    fn task_hash<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.task_hash)
    }

    #[getter]
    fn output_hash<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.output_hash)
    }

    #[getter]
    fn output(&self) -> String {
        self.inner.output.clone()
    }

    /// Status as a string: `"Completed"`, `"Failed"`, or `"Verified"`.
    /// The v0.1 Python API only emits `"Completed"`; the other variants are
    /// exposed for symmetry with verified attestations loaded from disk in
    /// future revisions.
    #[getter]
    fn status(&self) -> &'static str {
        match self.inner.status {
            AttestationStatus::Completed => "Completed",
            AttestationStatus::Failed(_) => "Failed",
            AttestationStatus::Verified { .. } => "Verified",
        }
    }

    #[getter]
    fn references(&self) -> Option<String> {
        self.inner.references.map(|u| u.to_string())
    }

    #[getter]
    fn timestamp(&self) -> i64 {
        self.inner.timestamp
    }

    #[getter]
    fn signature<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.signature)
    }

    /// Raw canonical bytes that the signature covers. Useful for verifying
    /// the byte-identical encoding contract from Python tests.
    fn signing_payload<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.signing_payload())
    }

    /// JSON-encoded full attestation, matching the wire format
    /// `attestly_core::append_attestation` writes to disk.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "Attestation(id={}, agent_id={:?}, status={})",
            self.inner.id,
            self.inner.agent_id,
            self.status()
        )
    }
}

/// Internal helper exposed to tests: canonicalise a Python dict into the
/// same JSON bytes that `sign_attestation` would feed into `task_hash` and
/// `output`. Lets the cross-language test pin byte equality without
/// reaching through a signed attestation.
#[pyfunction]
fn canonical_payload_bytes<'py>(
    py: Python<'py>,
    payload: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyBytes>> {
    let value = py_to_json_value(payload)?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|e| PyValueError::new_err(format!("canonical encode failed: {e}")))?;
    Ok(PyBytes::new(py, &bytes))
}

/// Sign `payload` (a JSON-compatible dict) under `identity`'s ed25519 key
/// and return a fully-populated [`PyAttestation`].
///
/// The dict is canonicalised inside Rust via `serde_json::Value` — keys
/// are sorted alphabetically, so insertion order in Python is irrelevant.
/// The resulting attestation's `task_hash` and `output_hash` are both
/// `sha256(canonical_bytes(payload))`; the `output` field carries the
/// canonical JSON string itself.
///
/// `timestamp` is supplied by the caller when set, otherwise it defaults
/// to the current Unix timestamp. `id` is supplied as a UUID string when
/// the caller needs a deterministic signature (test vectors); it defaults
/// to a fresh v4 UUID otherwise.
#[pyfunction]
#[pyo3(signature = (payload, identity, timestamp = None, id = None))]
fn sign_attestation(
    payload: &Bound<'_, PyAny>,
    identity: &PyAgentIdentity,
    timestamp: Option<i64>,
    id: Option<&str>,
) -> PyResult<PyAttestation> {
    let value = py_to_json_value(payload)?;
    let canonical = serde_json::to_vec(&value)
        .map_err(|e| PyValueError::new_err(format!("canonical encode failed: {e}")))?;
    let task_hash = sha256(&canonical);
    let output = String::from_utf8(canonical)
        .map_err(|e| PyValueError::new_err(format!("canonical bytes are not UTF-8: {e}")))?;

    let ts = timestamp.unwrap_or_else(chrono_now_secs);

    let mut att = CoreAttestation::new(
        identity.inner.agent_id.clone(),
        task_hash,
        output,
        AttestationStatus::Completed,
        None,
        ts,
    );
    if let Some(uuid_str) = id {
        att.id = uuid::Uuid::parse_str(uuid_str)
            .map_err(|e| PyValueError::new_err(format!("invalid uuid: {e}")))?;
    }
    identity.inner.keypair.sign_attestation(&mut att);
    Ok(PyAttestation { inner: att })
}

/// Verify an attestation against `pubkey`. Returns `True` if the embedded
/// ed25519 signature is valid *and* `pubkey` matches `attestation.agent_pubkey`
/// — guarding against the cross-pubkey confusion described in
/// `attestly_core::signing::tests::cross_keypair_signature_fails_verification`.
#[pyfunction]
fn verify_attestation(attestation: &PyAttestation, pubkey: &Bound<'_, PyBytes>) -> PyResult<bool> {
    let bytes = pubkey.as_bytes();
    if bytes.len() != 32 {
        return Err(PyValueError::new_err(format!(
            "pubkey must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut expected = [0u8; 32];
    expected.copy_from_slice(bytes);
    if attestation.inner.agent_pubkey != expected {
        return Ok(false);
    }
    Ok(verify_attestation_signature(&attestation.inner))
}

/// Compatibility shim — `chrono` is already a workspace dependency, but we
/// only need the `now().timestamp()` value, so factor it out so the test
/// vector can stub a fixed timestamp without dragging `chrono` into the
/// public API.
fn chrono_now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// PyO3 module entry point. Exposed to Python as `attestly._native` and
/// re-exported by `python/attestly/__init__.py`.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyAgentIdentity>()?;
    m.add_class::<PyAttestation>()?;
    m.add_function(wrap_pyfunction!(sign_attestation, m)?)?;
    m.add_function(wrap_pyfunction!(verify_attestation, m)?)?;
    m.add_function(wrap_pyfunction!(canonical_payload_bytes, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
