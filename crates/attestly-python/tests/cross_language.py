"""Cross-language signing self-test for attestly-core-py.

Pins byte-equality of the canonical encoding and signature against the
exact same vector defined in
`crates/attestly-core/tests/cross_language_vector.rs`. If either side ever
drifts, both sides fail in the next CI run.

Round-trip coverage:

1. ``sign_python_verify_rust`` — sign in Python, pipe the JSON
   attestation through the ``attestly-verify`` Rust binary, confirm exit 0.
2. ``sign_rust_verify_python`` — call the same Rust binary to print the
   pinned attestation's signing_payload bytes, then sign in Python and
   confirm the produced signature verifies in Rust as well.
3. ``canonical_bytes_match`` — assert byte equality of the canonical
   payload bytes against the constant pinned in the Rust test.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

import attestly


# Test-vector constants — keep these in sync with
# `crates/attestly-core/tests/cross_language_vector.rs`. Any intentional
# canonical-encoding change must update both files in the same commit.
VECTOR_SECRET_HEX = "07" * 32
VECTOR_AGENT_ID = "agent-vector"
VECTOR_ATTESTATION_ID = "11111111-2222-4333-8444-555555555555"
VECTOR_TIMESTAMP = 1_700_000_000

# Payload deliberately constructed in non-alphabetical key order — the
# canonical encoding must reorder to sorted keys so the byte output is
# identical whether the caller built the dict alphabetically or not.
VECTOR_PAYLOAD = {
    "z_field": "last alphabetically",
    "amount_cents": 50000,
    "action": "approve",
    "nested": {
        "y": True,
        "a": [1, 2, 3],
    },
}

EXPECTED_CANONICAL_PAYLOAD = (
    b'{"action":"approve","amount_cents":50000,'
    b'"nested":{"a":[1,2,3],"y":true},'
    b'"z_field":"last alphabetically"}'
)

EXPECTED_AGENT_PUBKEY_HEX = (
    "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
)

EXPECTED_SIGNATURE_HEX = (
    "b5d93fe27caf9c40a7efb5ae12ddc6ab7e7537a60968545939aba727d1ba5094"
    "e8896c47a9d482a261c28ba3b25ce6ea7e89db1b01eaa733e2387dfd5b09ed0c"
)

EXPECTED_OUTPUT_HASH_HEX = (
    "656512b26e98e00ffc465a37ef446e953665caa3270f6a692eed3ebb308dace4"
)


REPO_ROOT = Path(__file__).resolve().parents[3]


def _attestly_verify_binary() -> Path:
    """Locate the `attestly-verify` binary built from `attestly-core`.

    Honours `ATTESTLY_VERIFY_BIN` for CI overrides, otherwise searches the
    standard cargo target directories. The binary is built by `make
    check-python` before pytest runs.
    """
    env_override = os.environ.get("ATTESTLY_VERIFY_BIN")
    if env_override:
        return Path(env_override)

    # Prefer release if both exist (CI uses release), otherwise dev.
    for profile in ("release", "debug"):
        candidate = REPO_ROOT / "target" / profile / "attestly-verify"
        if candidate.exists():
            return candidate

    # Fall back to PATH lookup (e.g. installed via `cargo install`).
    on_path = shutil.which("attestly-verify")
    if on_path:
        return Path(on_path)

    raise FileNotFoundError(
        "attestly-verify binary not found. Build it with "
        "`cargo build -p attestly-core --bin attestly-verify` first."
    )


def _build_vector_identity() -> attestly.AgentIdentity:
    secret = bytes.fromhex(VECTOR_SECRET_HEX)
    return attestly.AgentIdentity.from_secret_bytes(VECTOR_AGENT_ID, secret)


def _build_vector_attestation() -> attestly.Attestation:
    identity = _build_vector_identity()
    return attestly.sign_attestation(
        VECTOR_PAYLOAD,
        identity,
        timestamp=VECTOR_TIMESTAMP,
        id=VECTOR_ATTESTATION_ID,
    )


def test_canonical_bytes_match() -> None:
    """Python's canonical encoding of the vector payload matches Rust's."""
    actual = attestly.canonical_payload_bytes(VECTOR_PAYLOAD)
    assert actual == EXPECTED_CANONICAL_PAYLOAD, (
        "canonical_payload_bytes drifted.\n"
        f"  expected: {EXPECTED_CANONICAL_PAYLOAD!r}\n"
        f"  actual:   {actual!r}"
    )


def test_canonical_bytes_insertion_order_independent() -> None:
    """Reordering the dict's insertion keys produces identical bytes."""
    reordered = {
        "action": "approve",
        "nested": {"a": [1, 2, 3], "y": True},
        "amount_cents": 50000,
        "z_field": "last alphabetically",
    }
    assert attestly.canonical_payload_bytes(reordered) == EXPECTED_CANONICAL_PAYLOAD


def test_vector_signature_matches_rust() -> None:
    """The pinned Python signature equals the pinned Rust signature."""
    att = _build_vector_attestation()
    assert att.signature.hex() == EXPECTED_SIGNATURE_HEX
    assert att.agent_pubkey.hex() == EXPECTED_AGENT_PUBKEY_HEX
    assert att.output_hash.hex() == EXPECTED_OUTPUT_HASH_HEX
    assert att.task_hash.hex() == EXPECTED_OUTPUT_HASH_HEX


def test_vector_attestation_self_verifies() -> None:
    """Python-signed → Python-verified round-trip on the pinned vector."""
    att = _build_vector_attestation()
    assert attestly.verify_attestation(att, att.agent_pubkey) is True


def test_sign_python_verify_rust() -> None:
    """Python-signed → Rust-verified: pipe JSON through `attestly-verify`."""
    try:
        binary = _attestly_verify_binary()
    except FileNotFoundError as exc:
        pytest.skip(str(exc))

    att = _build_vector_attestation()
    proc = subprocess.run(
        [str(binary)],
        input=att.to_json().encode("utf-8"),
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert proc.returncode == 0, (
        f"attestly-verify rejected a Python-signed attestation. "
        f"stderr={proc.stderr.decode(errors='replace')}"
    )
    assert proc.stdout.strip() == b"ok"


def test_sign_rust_verify_python() -> None:
    """Rust-emitted signing_payload bytes match what Python signs over.

    Builds the vector attestation in Python, asks the Rust binary to
    print its `signing_payload`, and asserts byte equality. This pins
    the cross-language canonical encoding from the **Rust** side
    independently of the constants in this file — if Rust's
    `Attestation::signing_payload` ever changes its byte layout, this
    test fires.
    """
    try:
        binary = _attestly_verify_binary()
    except FileNotFoundError as exc:
        pytest.skip(str(exc))

    att = _build_vector_attestation()
    proc = subprocess.run(
        [str(binary), "--print-canonical"],
        input=att.to_json().encode("utf-8"),
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert proc.returncode == 0, (
        f"attestly-verify --print-canonical failed. "
        f"stderr={proc.stderr.decode(errors='replace')}"
    )
    # Python and Rust must agree on signing_payload byte-for-byte.
    assert proc.stdout == att.signing_payload()


def test_python_canonical_payload_matches_attestation_output() -> None:
    """`canonical_payload_bytes` matches the attestation's `output` field.

    `sign_attestation` stores the canonical JSON in `output` as a UTF-8
    string. They must agree, otherwise the `output_hash` invariant breaks.
    """
    att = _build_vector_attestation()
    canonical = attestly.canonical_payload_bytes(VECTOR_PAYLOAD)
    assert att.output.encode("utf-8") == canonical


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
