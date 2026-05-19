"""Tests for the ``awp.langgraph.telemetry`` opt-out and wire contract.

The two load-bearing claims under test:

1. ``AWP_TELEMETRY=0`` produces **no network calls** (the Step 5 exit
   criterion).
2. The wire payload matches the schema the cloud's
   ``/v1/telemetry/usage`` endpoint accepts — no PII, install UUID,
   sdk_version, count, sink_type, python_version, os.
"""

from __future__ import annotations

import json
import os
import threading
import uuid
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest


@pytest.fixture
def install_id_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Redirect the install-id file at a tmp path so tests never touch ~/.config."""
    target = tmp_path / "install_id"
    monkeypatch.setenv("AWP_INSTALL_ID_PATH", str(target))
    return target


@pytest.fixture
def enabled(monkeypatch: pytest.MonkeyPatch):
    """Drop any stale AWP_TELEMETRY=0 from the inherited environment."""
    monkeypatch.delenv("AWP_TELEMETRY", raising=False)
    from awp.langgraph import telemetry as t

    t._state.enabled = True
    t._state.last_sent_day = None
    t._state.count_today = 0
    t._state.install_id = None
    yield t


@pytest.fixture
def recording_server():
    """Spin up a single-shot HTTP server that records POST bodies."""

    received: list[dict] = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802 (stdlib signature)
            length = int(self.headers.get("content-length", "0"))
            body = self.rfile.read(length)
            received.append(json.loads(body.decode("utf-8")))
            self.send_response(204)
            self.end_headers()

        def log_message(self, *args, **kwargs):  # silence the default access log
            return

    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address
    url = f"http://{host}:{port}/v1/usage"
    try:
        yield url, received
    finally:
        server.shutdown()
        server.server_close()


def test_load_or_create_install_id_is_stable(install_id_path: Path, enabled):
    """A second call must return the same UUID — stable per machine, not per process."""
    first = enabled.load_or_create_install_id()
    second = enabled.load_or_create_install_id()
    assert first == second
    assert install_id_path.exists()
    persisted = uuid.UUID(install_id_path.read_text().strip())
    assert persisted == first


def test_env_opt_out_disables_telemetry(
    install_id_path: Path, monkeypatch: pytest.MonkeyPatch
):
    """``AWP_TELEMETRY=0`` must produce no network call (Step 5 exit criterion)."""
    monkeypatch.setenv("AWP_TELEMETRY", "0")
    from awp.langgraph import telemetry as t

    # The env-var check must short-circuit regardless of in-process state.
    assert t.is_enabled() is False
    sent = t.emit_daily_aggregate(force=True)
    assert sent is False, "no POST should have been attempted under AWP_TELEMETRY=0"


def test_record_attestation_is_noop_when_disabled(
    install_id_path: Path, monkeypatch: pytest.MonkeyPatch
):
    monkeypatch.setenv("AWP_TELEMETRY", "0")
    from awp.langgraph import telemetry as t

    t._state.count_today = 0
    t.record_attestation("FileSink")
    assert t._state.count_today == 0, "counter must not tick when disabled"


def test_programmatic_disable_blocks_emit(install_id_path: Path, enabled):
    enabled.telemetry_disable()
    assert enabled.is_enabled() is False
    sent = enabled.emit_daily_aggregate(force=True)
    assert sent is False


def test_emit_posts_expected_payload(
    install_id_path: Path,
    enabled,
    recording_server,
    monkeypatch: pytest.MonkeyPatch,
):
    """Happy path: aggregate POSTs the documented schema."""
    url, received = recording_server
    monkeypatch.setenv("AWP_TELEMETRY_URL", url)

    enabled.record_attestation("FileSink")
    enabled.record_attestation("FileSink")
    enabled.record_attestation("FileSink")
    ok = enabled.emit_daily_aggregate(force=True)

    assert ok is True
    assert len(received) == 1
    body = received[0]
    # Schema check — these are the exact field names the cloud accepts.
    assert set(body.keys()) == {
        "install_id",
        "sdk_version",
        "attestations_emitted_today",
        "sink_type",
        "python_version",
        "os",
    }
    uuid.UUID(body["install_id"])  # parseable
    assert body["sdk_version"] == enabled.SDK_VERSION
    assert body["attestations_emitted_today"] == 3
    assert body["sink_type"] == "FileSink"
    assert body["python_version"]
    assert body["os"]
    # No PII — none of these may appear in a telemetry payload.
    for forbidden in ("email", "agent_id", "customer_id", "api_key", "user_id"):
        assert forbidden not in body


def test_emit_dedupes_within_a_day(
    install_id_path: Path,
    enabled,
    recording_server,
    monkeypatch: pytest.MonkeyPatch,
):
    """Two un-forced emits within the same UTC day produce one POST, not two."""
    url, received = recording_server
    monkeypatch.setenv("AWP_TELEMETRY_URL", url)

    assert enabled.emit_daily_aggregate(force=True) is True
    assert enabled.emit_daily_aggregate(force=False) is False
    assert len(received) == 1


def test_emit_swallows_network_errors(
    install_id_path: Path,
    enabled,
    monkeypatch: pytest.MonkeyPatch,
):
    """A dead endpoint must not raise — telemetry is best-effort."""
    monkeypatch.setenv(
        "AWP_TELEMETRY_URL",
        "http://127.0.0.1:1/v1/usage",  # port 1 is reserved
    )
    # No raise; just False.
    assert enabled.emit_daily_aggregate(force=True) is False


def test_default_install_id_path_respects_xdg(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    monkeypatch.delenv("AWP_INSTALL_ID_PATH", raising=False)
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    from awp.langgraph import telemetry as t

    assert t._default_install_id_path() == tmp_path / "awp" / "install_id"


def test_default_install_id_path_falls_back_to_home(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
):
    monkeypatch.delenv("AWP_INSTALL_ID_PATH", raising=False)
    monkeypatch.delenv("XDG_CONFIG_HOME", raising=False)
    monkeypatch.setenv("HOME", str(tmp_path))
    from awp.langgraph import telemetry as t

    assert t._default_install_id_path() == tmp_path / ".config" / "awp" / "install_id"


def test_env_var_overrides_in_process_enable(
    install_id_path: Path,
    monkeypatch: pytest.MonkeyPatch,
):
    """``AWP_TELEMETRY=0`` must beat a subsequent ``telemetry_enable()`` call."""
    monkeypatch.setenv("AWP_TELEMETRY", "0")
    from awp.langgraph import telemetry as t

    t.telemetry_enable()
    assert t.is_enabled() is False


def test_record_attestation_resets_count_on_new_day(install_id_path: Path, enabled):
    """The counter is per-day, not per-process."""
    from datetime import date

    enabled.record_attestation("FileSink")
    enabled.record_attestation("FileSink")
    assert enabled._state.count_today == 2
    # Simulate a day rollover.
    enabled._state.current_day = date(1970, 1, 1)
    enabled.record_attestation("FileSink")
    assert enabled._state.count_today == 1
