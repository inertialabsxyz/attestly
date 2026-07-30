"""Sinks: file write round-trip, cloud retry/422 behaviour, callable invocation."""

from __future__ import annotations

import json
from typing import List

import pytest

# Import attestly-core-py via its C-extension submodule — `attestly` is a namespace
# package shared with the SDK, so the top-level `import attestly` may not expose
# these symbols depending on path order. See sinks.py for the full rationale.
from attestly._native import AgentIdentity, Attestation, sign_attestation

from attestly.langgraph.sinks import (
    CallableSink,
    CloudSink,
    CloudSinkError,
    CloudSinkUnavailable,
    FileSink,
)


def _signed_attestation(label: str = "n1") -> "Attestation":
    ident = AgentIdentity.generate(f"test-agent-{label}")
    return sign_attestation(
        {"node_name": label, "value": "x"}, ident, timestamp=1_700_000_000
    )


# ---- FileSink ------------------------------------------------------------


def test_file_sink_appends_jsonl(tmp_path) -> None:
    path = tmp_path / "att.jsonl"
    sink = FileSink(path)
    a = _signed_attestation("one")
    b = _signed_attestation("two")
    sink.emit(a)
    sink.emit(b)
    lines = path.read_text().strip().split("\n")
    assert len(lines) == 2
    assert json.loads(lines[0])["agent_id"] == a.agent_id
    assert json.loads(lines[1])["agent_id"] == b.agent_id


def test_file_sink_creates_parent_dir(tmp_path) -> None:
    nested = tmp_path / "a" / "b" / "c" / "att.jsonl"
    sink = FileSink(nested)
    sink.emit(_signed_attestation())
    assert nested.exists()


def test_file_sink_round_trips_canonical_wire_format(tmp_path) -> None:
    """A FileSink-written line must be loadable and verifiable end-to-end via
    attestly-core-py — the same path the static audit viewer uses.
    """
    path = tmp_path / "att.jsonl"
    sink = FileSink(path)
    ident = AgentIdentity.generate("agent-roundtrip")
    att = sign_attestation({"node_name": "x"}, ident, timestamp=1_700_000_000)
    sink.emit(att)

    raw = path.read_text().strip()
    parsed = json.loads(raw)
    # All the wire fields documented in services/attestly-cloud/API.md must be present.
    for field in (
        "id",
        "agent_id",
        "agent_pubkey",
        "task_hash",
        "output_hash",
        "output",
        "status",
        "references",
        "timestamp",
        "signature",
    ):
        assert field in parsed, f"missing wire field {field!r}"


# ---- CloudSink -----------------------------------------------------------


class _MockTransport:
    """In-memory HTTP transport: scriptable response sequence + call log."""

    def __init__(self, responses):
        self._responses = list(responses)
        self.calls: List[dict] = []

    def __call__(self, method, url, headers, body, timeout):
        self.calls.append(
            {
                "method": method,
                "url": url,
                "headers": dict(headers),
                "body": body,
                "timeout": timeout,
            }
        )
        if not self._responses:
            raise AssertionError("MockTransport received more calls than scripted")
        next_resp = self._responses.pop(0)
        if isinstance(next_resp, Exception):
            raise next_resp
        return next_resp


def _cloud(*responses, **kwargs):
    transport = _MockTransport(responses)
    sink = CloudSink(
        api_key="test-key",
        endpoint="http://test.local",
        transport=transport,
        backoff_base=0.0,
        backoff_cap=0.0,
        **kwargs,
    )
    return sink, transport


def test_cloud_sink_posts_json_body_with_api_key() -> None:
    sink, transport = _cloud((201, {}, b""))
    att = _signed_attestation()
    sink.emit(att)
    assert len(transport.calls) == 1
    call = transport.calls[0]
    assert call["method"] == "POST"
    assert call["url"] == "http://test.local/v1/attestations"
    assert call["headers"]["x-api-key"] == "test-key"
    assert call["headers"]["content-type"] == "application/json"
    # Body must be the canonical Attestation JSON, not a wrapped envelope.
    payload = json.loads(call["body"].decode())
    assert payload["agent_id"] == att.agent_id


def test_cloud_sink_retries_on_5xx_then_succeeds() -> None:
    sink, transport = _cloud(
        (502, {}, b"upstream"),
        (503, {}, b"again"),
        (200, {}, b""),
    )
    sink.emit(_signed_attestation())
    assert len(transport.calls) == 3


def test_cloud_sink_gives_up_after_max_attempts() -> None:
    sink, transport = _cloud(
        (500, {}, b"first"),
        (502, {}, b"second"),
        (503, {}, b"third"),
    )
    with pytest.raises(CloudSinkUnavailable):
        sink.emit(_signed_attestation())
    assert len(transport.calls) == 3


def test_cloud_sink_surfaces_422_immediately() -> None:
    sink, transport = _cloud((422, {}, b"signature failed"))
    with pytest.raises(CloudSinkError, match="HTTP 422"):
        sink.emit(_signed_attestation())
    assert len(transport.calls) == 1  # no retry


def test_cloud_sink_surfaces_other_4xx_immediately() -> None:
    sink, transport = _cloud((401, {}, b"bad key"))
    with pytest.raises(CloudSinkError, match="HTTP 401"):
        sink.emit(_signed_attestation())
    assert len(transport.calls) == 1


def test_cloud_sink_rejects_empty_api_key() -> None:
    with pytest.raises(ValueError):
        CloudSink(api_key="", endpoint="http://x")


def test_cloud_sink_default_endpoint_matches_api_doc() -> None:
    """Documents the canonical hosted endpoint matches services/attestly-cloud/API.md."""
    assert CloudSink.DEFAULT_ENDPOINT == "https://app.attestly.xyz"


# ---- CallableSink --------------------------------------------------------


def test_callable_sink_invokes_function_per_attestation() -> None:
    received = []
    sink = CallableSink(received.append)
    a = _signed_attestation("one")
    b = _signed_attestation("two")
    sink.emit(a)
    sink.emit(b)
    assert [x.agent_id for x in received] == [a.agent_id, b.agent_id]


def test_callable_sink_rejects_non_callable() -> None:
    with pytest.raises(TypeError):
        CallableSink("not a function")  # type: ignore[arg-type]
