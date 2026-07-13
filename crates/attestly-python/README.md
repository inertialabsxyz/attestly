# attestly-core-py

Python bindings for the [Attestly](https://github.com/inertialabsxyz/attestly)
core — sign and verify cryptographic attestations of agent work from
Python, with byte-identical signatures to the Rust reference
implementation.

## Status

GTM Phase 2, Step 1 of the Attestly project. Currently published to **TestPyPI
only**; promotion to the real `pypi.org` is gated on the Phase 2 design-
partner close. See `planning/gtm-phase-2-plan.md` for sequencing.

## Install

```bash
pip install --index-url https://test.pypi.org/simple/ attestly-core-py
```

Local development build (Rust toolchain + Python 3.9+ required):

```bash
pip install maturin pytest
cd crates/attestly-python
maturin develop --release
```

`maturin develop` compiles the Rust extension and installs it into the
active venv. The Python import name is `attestly`; the distribution name on
PyPI is `attestly-core-py`.

## Usage

```python
import attestly

# Persistent identity — load from disk or generate + save.
identity = attestly.AgentIdentity.load_or_create("./data/identities", "agent-1")

# Or, ephemeral identity (generated, not persisted):
identity = attestly.AgentIdentity.generate("agent-1")

# Sign a payload. The dict is canonicalised inside Rust (alpha-sorted
# keys) before signing, so insertion order is irrelevant.
attestation = attestly.sign_attestation(
    {"decision": "approve", "amount_cents": 50_000},
    identity,
)

# Verify the embedded signature against the pubkey.
assert attestly.verify_attestation(attestation, identity.public_key)

# Access fields.
print(attestation.id)             # uuid string
print(attestation.agent_id)       # "agent-1"
print(attestation.output)         # canonical JSON of the payload
print(attestation.signature.hex())
print(attestation.timestamp)
```

## Byte-identical signatures across Rust and Python

The load-bearing guarantee: an attestation signed in Python verifies in
Rust without re-serialising, and vice versa. The Python ↔ Rust ↔ Python
self-test at `tests/cross_language.py` pins this against a checked-in
fixed-seed vector. If either side's canonical encoding ever drifts, both
test suites fire on the next CI run.

Worked example:

```python
# Python side
import attestly, json

identity = attestly.AgentIdentity.from_secret_bytes("agent-x", b"\x07" * 32)
att = attestly.sign_attestation({"k": "v"}, identity, timestamp=1700000000)

# Write the attestation to disk in the same JSON-lines wire format that
# `attestly_core::append_attestation` uses.
with open("/tmp/att.json", "w") as f:
    f.write(att.to_json() + "\n")
```

```bash
# Rust side: verify the file using the bundled `attestly-verify` binary.
cargo run -p attestly-core --bin attestly-verify --release < /tmp/att.json
# → ok
```

## FFI boundary

| Python type | Rust type |
| --- | --- |
| `dict` (str-keyed, JSON-like values) | `serde_json::Value::Object` |
| `list` / `tuple` | `serde_json::Value::Array` |
| `str` | `serde_json::Value::String` |
| `int` (within `i64`) | `serde_json::Value::Number` |
| `float` (finite) | `serde_json::Value::Number` |
| `bool` | `serde_json::Value::Bool` |
| `None` | `serde_json::Value::Null` |
| `bytes` (pubkey, signature) | `&[u8]` |

Non-finite floats (`NaN`, `±Inf`) raise `ValueError`; non-string dict
keys raise `TypeError`; integers wider than `i64` raise `OverflowError`.

`AgentIdentity` and `Attestation` are Python classes backed by Rust
structs; field access goes through getters so the underlying Rust value
is immutable from Python. Tampering after signing would invalidate the
signature anyway.

## Canonical encoding rules

1. Dict keys are sorted alphabetically (`serde_json`'s default
   `BTreeMap` ordering). Insertion order in Python is irrelevant.
2. `task_hash` and `output_hash` are both
   `SHA-256(canonical_bytes(payload))`.
3. `output` is the canonical JSON string itself — UTF-8 round-trips
   through `Attestation::output`.
4. `signing_payload` (the bytes the signature covers) is the
   `serde_json::to_vec` of the `Attestation` struct **excluding the
   signature**, in the declaration order documented in
   `crates/attestly-core/src/attestation.rs`.

The Rust reference is the source of truth — if Python and Rust disagree,
Python is wrong.

## Limitations (v0.1)

- **No async API.** All calls are synchronous. Async wrappers may land
  in a later release once the LangGraph SDK lands and surfaces a real
  need.
- **No streaming attestations.** The full payload + signature must fit
  in memory.
- **Single-process only.** No coordination primitives. Use the LangGraph
  SDK (Step 3) or `attestly-cloud` (Step 2) for multi-process attestation
  coordination.
- **Status is `Completed` only when signing from Python.** `Failed` and
  `Verified` statuses are exposed as read-only on attestations loaded
  from disk but are not produced by `sign_attestation`.
- **`AgentIdentity.from_secret_bytes` is test-only.** Never embed a
  fixed seed in production — use `generate` or `load_or_create`.

## License

Apache-2.0 OR MIT, same as the rest of the Attestly repository.
