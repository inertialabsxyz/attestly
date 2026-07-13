"""Python-side API smoke tests for attestly-core-py.

Verifies the surface documented in
`planning/gtm-phase-2-plan.md` → "Step 1 — PyO3 Bindings":

- ``AgentIdentity.generate(agent_id)`` produces an identity with a
  32-byte public key.
- ``AgentIdentity.load_or_create(path, agent_id)`` persists across calls.
- ``sign_attestation(dict, identity)`` produces an attestation whose
  fields are accessible as Python attributes.
- ``verify_attestation(attestation, pubkey)`` returns ``True`` for a
  well-formed attestation and ``False`` when tampered.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import attestly


def test_generate_yields_32_byte_pubkey() -> None:
    identity = attestly.AgentIdentity.generate("agent-gen")
    assert identity.agent_id == "agent-gen"
    assert isinstance(identity.public_key, bytes)
    assert len(identity.public_key) == 32


def test_load_or_create_persists_across_calls(tmp_path: Path) -> None:
    first = attestly.AgentIdentity.load_or_create(str(tmp_path), "agent-persist")
    second = attestly.AgentIdentity.load_or_create(str(tmp_path), "agent-persist")
    assert first.public_key == second.public_key

    # Different agent_id ⇒ different key under the same store.
    other = attestly.AgentIdentity.load_or_create(str(tmp_path), "agent-other")
    assert other.public_key != first.public_key


def test_load_or_create_writes_file_with_expected_shape(tmp_path: Path) -> None:
    identity = attestly.AgentIdentity.load_or_create(str(tmp_path), "agent-shape")
    on_disk = tmp_path / "agent-shape.json"
    assert on_disk.exists()
    parsed = json.loads(on_disk.read_text())
    assert parsed["agent_id"] == "agent-shape"
    assert len(bytes.fromhex(parsed["public_key"])) == 32
    assert len(bytes.fromhex(parsed["secret_key"])) == 32


def test_sign_then_verify_round_trip() -> None:
    identity = attestly.AgentIdentity.generate("agent-roundtrip")
    att = attestly.sign_attestation({"task": "hello"}, identity)
    assert attestly.verify_attestation(att, identity.public_key) is True


def test_attestation_field_accessors() -> None:
    identity = attestly.AgentIdentity.generate("agent-fields")
    att = attestly.sign_attestation({"task": "hello"}, identity, timestamp=1700000000)

    assert att.agent_id == "agent-fields"
    assert att.agent_pubkey == identity.public_key
    assert isinstance(att.task_hash, bytes) and len(att.task_hash) == 32
    assert isinstance(att.output_hash, bytes) and len(att.output_hash) == 32
    assert att.output == '{"task":"hello"}'  # canonical JSON
    assert att.status == "Completed"
    assert att.references is None
    assert att.timestamp == 1700000000
    assert isinstance(att.signature, bytes) and len(att.signature) == 64


def test_verify_rejects_wrong_pubkey() -> None:
    identity = attestly.AgentIdentity.generate("agent-a")
    other = attestly.AgentIdentity.generate("agent-b")
    att = attestly.sign_attestation({"task": "hello"}, identity)
    assert attestly.verify_attestation(att, other.public_key) is False


def test_verify_rejects_short_pubkey() -> None:
    identity = attestly.AgentIdentity.generate("agent-short")
    att = attestly.sign_attestation({"task": "hello"}, identity)
    with pytest.raises(ValueError, match="pubkey must be 32 bytes"):
        attestly.verify_attestation(att, b"\x00" * 8)


def test_payload_must_have_string_keys() -> None:
    identity = attestly.AgentIdentity.generate("agent-typed")
    with pytest.raises(TypeError, match="dict keys must be strings"):
        attestly.sign_attestation({1: "v"}, identity)


def test_payload_rejects_unsupported_types() -> None:
    identity = attestly.AgentIdentity.generate("agent-typed")
    with pytest.raises(TypeError, match="unsupported type"):
        attestly.sign_attestation({"k": object()}, identity)


def test_payload_supports_nested_structures() -> None:
    identity = attestly.AgentIdentity.generate("agent-nested")
    att = attestly.sign_attestation(
        {"list": [1, 2, [3, 4]], "obj": {"k": "v", "n": None}},
        identity,
    )
    assert attestly.verify_attestation(att, identity.public_key) is True
    # serde_json's BTreeMap sorts keys ⇒ `list` comes before `obj`.
    assert att.output == '{"list":[1,2,[3,4]],"obj":{"k":"v","n":null}}'


def test_to_json_round_trips() -> None:
    identity = attestly.AgentIdentity.generate("agent-json")
    att = attestly.sign_attestation({"task": "hello"}, identity, timestamp=42)
    blob = att.to_json()
    parsed = json.loads(blob)
    assert parsed["agent_id"] == "agent-json"
    assert parsed["timestamp"] == 42
    assert parsed["status"] == "Completed"


def test_version_string_is_exposed() -> None:
    assert isinstance(attestly.__version__, str)
    assert attestly.__version__.count(".") >= 1
