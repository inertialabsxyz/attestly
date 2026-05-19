"""Anonymous SDK telemetry for ``awp-langgraph``.

The contract is documented in the package README and at
``planning/gtm-phase-2-plan.md`` (GTM Phase 2 Step 5).

Quick contract:

* Daily aggregate POST to ``https://telemetry.awp-cloud.xyz/v1/usage``.
* Payload contains **no** payload data, **no** agent ids, **no** customer
  ids, and **no** user identifiers beyond a single install UUID generated
  locally on first import.
* Opt out via the ``AWP_TELEMETRY=0`` environment variable or by calling
  :func:`telemetry_disable` from Python.
* The install UUID lives at ``~/.config/awp/install_id`` (override with
  the ``AWP_INSTALL_ID_PATH`` environment variable, e.g. for tests).

Why opt-out by default? Conversion intelligence is what funds OSS
development; the opt-out path is one environment variable, zero PII is
collected, and the per-day aggregate is the smallest signal that lets
the GTM team know which OSS users have grown into a real workload.

If community signal turns negative inside the first 30 days of launch,
the project owners may flip the default to opt-IN (this is the explicit
escape valve called out in the Step 5 prompt — it is a human decision,
not an SDK decision).
"""

from __future__ import annotations

import json
import os
import platform
import sys
import threading
import time
import uuid
from dataclasses import dataclass, field
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Optional
from urllib import error as urllib_error
from urllib import request as urllib_request

DEFAULT_TELEMETRY_URL = "https://telemetry.awp-cloud.xyz/v1/usage"
TELEMETRY_ENV_VAR = "AWP_TELEMETRY"
TELEMETRY_URL_ENV_VAR = "AWP_TELEMETRY_URL"
INSTALL_ID_PATH_ENV_VAR = "AWP_INSTALL_ID_PATH"
SDK_VERSION = "0.1.0a1"

# A short, polite timeout. Telemetry must never delay the user's program;
# if the network is wedged we drop the report.
_REQUEST_TIMEOUT_SECONDS = 3.0


def _default_install_id_path() -> Path:
    """Return the default location of the install-id file.

    ``$XDG_CONFIG_HOME/awp/install_id`` if ``XDG_CONFIG_HOME`` is set,
    otherwise ``~/.config/awp/install_id``. Override with the
    ``AWP_INSTALL_ID_PATH`` environment variable.
    """
    override = os.environ.get(INSTALL_ID_PATH_ENV_VAR)
    if override:
        return Path(override)
    xdg = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg) if xdg else Path.home() / ".config"
    return base / "awp" / "install_id"


def is_enabled() -> bool:
    """True iff telemetry is enabled in the current process.

    Opt-out is honoured both via the ``AWP_TELEMETRY=0`` environment
    variable (the documented path) and via :func:`telemetry_disable`
    (the programmatic path). The env var wins — if it's set to ``0``
    (or any of the equivalent off values), no in-process flip can
    re-enable telemetry.
    """
    raw = os.environ.get(TELEMETRY_ENV_VAR)
    if raw is not None and raw.strip().lower() in {"0", "false", "no", "off"}:
        return False
    return _state.enabled


def telemetry_disable() -> None:
    """Disable telemetry for the rest of this process.

    Equivalent to ``AWP_TELEMETRY=0`` but settable from Python. Useful in
    notebooks or apps that want to opt out without touching the
    environment.
    """
    _state.enabled = False


def telemetry_enable() -> None:
    """Re-enable telemetry in this process.

    Has no effect if ``AWP_TELEMETRY=0`` (or equivalent) is set in the
    environment — the env var is authoritative when present.
    """
    _state.enabled = True


def load_or_create_install_id(path: Optional[Path] = None) -> uuid.UUID:
    """Return the stable per-machine install UUID, creating it on first use."""
    target = path or _default_install_id_path()
    try:
        existing = target.read_text(encoding="utf-8").strip()
        return uuid.UUID(existing)
    except (FileNotFoundError, ValueError, PermissionError):
        # Fall through to (re)create. We deliberately swallow PermissionError
        # so a read-only home directory never crashes the user's program;
        # we'll generate an ephemeral id below.
        pass

    new_id = uuid.uuid4()
    try:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(str(new_id) + "\n", encoding="utf-8")
    except (PermissionError, OSError):
        # Read-only home, sandboxed runtime, etc. The id stays in memory
        # for the lifetime of this process; the next process will get a
        # fresh one. That's fine — telemetry is best-effort, not exact.
        pass
    return new_id


@dataclass
class _State:
    """Process-local telemetry state.

    The counter is incremented per attestation by the SDK's emission path
    (Step 3 will wire this up). Tests use :func:`record_attestation` directly.
    """

    enabled: bool = True
    install_id: Optional[uuid.UUID] = None
    sink_type: str = "FileSink"
    count_today: int = 0
    current_day: date = field(default_factory=lambda: datetime.now(timezone.utc).date())
    last_sent_day: Optional[date] = None
    lock: threading.Lock = field(default_factory=threading.Lock)


_state = _State()


def record_attestation(sink_type: Optional[str] = None) -> None:
    """Increment the in-memory daily counter.

    Called by the SDK each time an attestation is emitted. The full SDK
    (Step 3) will wire this into the :class:`Sink.emit` hook; tests and
    integrators may call it directly. If telemetry is disabled, this
    function is a no-op and never touches the network.
    """
    if not is_enabled():
        return
    today = datetime.now(timezone.utc).date()
    with _state.lock:
        if today != _state.current_day:
            _state.current_day = today
            _state.count_today = 0
        _state.count_today += 1
        if sink_type:
            _state.sink_type = sink_type


def _payload(install_id: uuid.UUID, count: int, sink_type: str) -> dict:
    """Build the exact wire payload the cloud's `/v1/telemetry/usage` accepts."""
    return {
        "install_id": str(install_id),
        "sdk_version": SDK_VERSION,
        "attestations_emitted_today": int(count),
        "sink_type": sink_type,
        "python_version": platform.python_version(),
        "os": sys.platform,
    }


def _telemetry_url() -> str:
    return os.environ.get(TELEMETRY_URL_ENV_VAR, DEFAULT_TELEMETRY_URL)


def _post(url: str, payload: dict, timeout: float = _REQUEST_TIMEOUT_SECONDS) -> bool:
    """POST the payload as JSON. Returns True on 2xx, False on any failure."""
    body = json.dumps(payload).encode("utf-8")
    req = urllib_request.Request(
        url,
        data=body,
        method="POST",
        headers={
            "content-type": "application/json",
            "user-agent": f"awp-langgraph/{SDK_VERSION}",
        },
    )
    try:
        with urllib_request.urlopen(req, timeout=timeout) as resp:  # noqa: S310
            return 200 <= resp.status < 300
    except (urllib_error.URLError, TimeoutError, OSError):
        return False


def emit_daily_aggregate(force: bool = False) -> bool:
    """POST today's aggregate. Returns True if a request was sent and accepted.

    No-op if telemetry is disabled, or if today's aggregate has already
    been sent (re-runs within the same UTC day overwrite the server-side
    row, but we still throttle to one POST per day per process to keep
    the network footprint trivial). Pass ``force=True`` to bypass the
    per-day dedupe — tests and the once-on-import scheduler use this.
    """
    if not is_enabled():
        return False
    today = datetime.now(timezone.utc).date()
    with _state.lock:
        if not force and _state.last_sent_day == today:
            return False
        install_id = _state.install_id or load_or_create_install_id()
        _state.install_id = install_id
        payload = _payload(install_id, _state.count_today, _state.sink_type)
        _state.last_sent_day = today

    return _post(_telemetry_url(), payload)


def _start_background_scheduler() -> None:
    """Fire-and-forget background thread that emits the aggregate daily.

    The thread sleeps in 1-hour chunks and emits when the UTC day rolls
    over. It is a daemon so it never blocks Python exit; the first
    aggregate is sent on first import for the install-id discovery and
    the conversion signal.
    """
    if not is_enabled():
        return

    def _loop() -> None:
        # Send once on first import — this is how the GTM team sees a new
        # install at all. The count is 0 at this moment for fresh
        # imports, but the (install_id, day) row is what matters.
        try:
            emit_daily_aggregate(force=True)
        except Exception:
            pass
        while True:
            time.sleep(3600)
            if not is_enabled():
                return
            try:
                emit_daily_aggregate()
            except Exception:
                # Telemetry is best-effort. Never crash the host program.
                pass

    t = threading.Thread(target=_loop, name="awp-telemetry", daemon=True)
    t.start()


def _maybe_autostart() -> None:
    """Start the background scheduler unless we're under pytest or opted-out.

    We deliberately skip the scheduler under pytest so the test suite
    never opens a real network socket. Tests that exercise the wire
    payload call :func:`emit_daily_aggregate` directly with a redirected
    ``AWP_TELEMETRY_URL``.
    """
    if "pytest" in sys.modules:
        return
    if not is_enabled():
        return
    _start_background_scheduler()


# Side effect on import: kick the scheduler. The opt-out check above
# guarantees this is a zero-cost no-op when telemetry is disabled.
_maybe_autostart()
