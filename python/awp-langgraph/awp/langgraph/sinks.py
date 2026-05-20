"""Pluggable sinks for emitting signed attestations.

Three built-in implementations are provided:

- :class:`FileSink` — append a JSON line per attestation. The default.
- :class:`CloudSink` — ``POST /v1/attestations`` to a hosted ``awp-cloud``
  per ``services/awp-cloud/API.md``. Retries on 5xx and connection errors;
  raises loudly on HTTP 422 (signature failed server-side verification,
  which always means the SDK is broken — not the network).
- :class:`CallableSink` — invoke an arbitrary callable. Escape hatch.

Custom sinks just need to implement :class:`Sink` (the ``Sink.emit`` method
takes one ``awp.Attestation`` and returns ``None``).

The cloud-side wire contract is locked at ``services/awp-cloud/API.md`` and
is the same payload shape ``Attestation.to_json()`` produces, so this module
does no encoding work of its own — the canonical signing payload is owned
entirely by ``awp-core-py``.
"""

from __future__ import annotations

import os
import random
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable, Optional, Protocol, runtime_checkable

# Import the Attestation type from awp-core-py's C-extension submodule.
# We import from `awp._native` rather than the top-level `awp` package
# because `awp` is a PEP 420 namespace package shared with this SDK — when
# both packages are on the path, `import awp` may execute either package's
# `__init__.py`, and only `awp-core-py`'s exposes `Attestation` at the top
# level. The `_native` submodule is unambiguous regardless of import order.
try:
    from awp._native import Attestation  # type: ignore
except ImportError as exc:  # pragma: no cover - the package depends on awp-core-py
    raise ImportError(
        "awp-langgraph requires awp-core-py to be installed. "
        "Install it with `pip install awp-core-py` (or `maturin develop` "
        "from `crates/awp-python/` for local builds)."
    ) from exc


@runtime_checkable
class Sink(Protocol):
    """Anywhere a signed attestation can land."""

    def emit(self, attestation: Attestation) -> None: ...


# ---------------------------------------------------------------------------
# FileSink
# ---------------------------------------------------------------------------


class FileSink:
    """Append attestations as JSONL to ``path``.

    Each line is ``Attestation.to_json()`` from ``awp-core-py`` — byte-
    identical to what the Rust ``append_attestation`` function writes, so
    the same JSONL file can be loaded indifferently by either implementation
    and verified against the static audit viewer.

    Thread-safe: a per-instance lock serialises concurrent ``emit`` calls so
    parallel LangGraph branches never interleave half-written lines.
    """

    sink_type = "FileSink"

    def __init__(self, path: os.PathLike) -> None:
        self.path = Path(path)
        self._lock = threading.Lock()
        parent = self.path.parent
        if str(parent) and not parent.exists():
            parent.mkdir(parents=True, exist_ok=True)

    def emit(self, attestation: Attestation) -> None:
        line = attestation.to_json()
        with self._lock:
            with self.path.open("a", encoding="utf-8") as f:
                f.write(line)
                f.write("\n")


# ---------------------------------------------------------------------------
# CloudSink
# ---------------------------------------------------------------------------


class CloudSinkError(RuntimeError):
    """The cloud refused our attestation in a way that points at an SDK bug
    (chiefly: HTTP 422 — signature failed server-side verification, or 4xx
    indicating bad auth / malformed request). Network and 5xx transient
    failures surface as :class:`CloudSinkUnavailable` instead.
    """


class CloudSinkUnavailable(RuntimeError):
    """The cloud is unreachable after all retries. Distinct from
    :class:`CloudSinkError` — this one means the network is sad, not the
    payload.
    """


# Default transport. Tests inject their own.
def _default_transport(
    method: str,
    url: str,
    headers: dict,
    body: bytes,
    timeout: float,
) -> "tuple[int, dict, bytes]":
    req = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as e:
        # HTTPError is a Response object; reading body lets the caller decide
        # whether to retry or surface.
        return e.code, dict(e.headers or {}), e.read() if hasattr(e, "read") else b""


class CloudSink:
    """POST attestations to a hosted ``awp-cloud`` endpoint.

    Wire contract: ``services/awp-cloud/API.md`` § ``POST /v1/attestations``.
    Body is the JSON-encoded attestation; auth via ``x-api-key``.

    Retries on 5xx and connection errors with exponential backoff (3
    attempts max). HTTP 422 (signature invalid) raises
    :class:`CloudSinkError` immediately — that points at an SDK bug rather
    than a transient problem, and silently retrying would mask it. Other
    4xx also surface immediately (bad auth, malformed request: not things
    that get better with retries).
    """

    DEFAULT_ENDPOINT = "https://app.awp-cloud.xyz"
    _POST_PATH = "/v1/attestations"
    sink_type = "CloudSink"

    def __init__(
        self,
        api_key: str,
        endpoint: str = DEFAULT_ENDPOINT,
        *,
        timeout: float = 10.0,
        max_attempts: int = 3,
        backoff_base: float = 0.25,
        backoff_cap: float = 4.0,
        transport: Optional[Callable[..., "tuple[int, dict, bytes]"]] = None,
    ) -> None:
        if not api_key:
            raise ValueError("CloudSink requires a non-empty api_key")
        self.api_key = api_key
        self.endpoint = endpoint.rstrip("/")
        self.timeout = timeout
        self.max_attempts = max_attempts
        self.backoff_base = backoff_base
        self.backoff_cap = backoff_cap
        self._transport = transport or _default_transport

    def emit(self, attestation: Attestation) -> None:
        url = f"{self.endpoint}{self._POST_PATH}"
        body = attestation.to_json().encode("utf-8")
        headers = {
            "content-type": "application/json",
            "x-api-key": self.api_key,
            "user-agent": "awp-langgraph",
        }

        last_error: Any = None
        for attempt in range(1, self.max_attempts + 1):
            try:
                status, _resp_headers, resp_body = self._transport(
                    "POST", url, headers, body, self.timeout
                )
            except (urllib.error.URLError, OSError) as e:
                last_error = e
                self._sleep_backoff(attempt)
                continue

            # 422 means the server's signature re-verification failed. That
            # is always an SDK bug — retrying would only mask it.
            if status == 422:
                raise CloudSinkError(
                    f"cloud rejected attestation with HTTP 422 "
                    f"(signature verification failed server-side): "
                    f"{_safe_decode(resp_body)}"
                )

            if 200 <= status < 300:
                return

            if 400 <= status < 500:
                raise CloudSinkError(
                    f"cloud rejected attestation with HTTP {status}: "
                    f"{_safe_decode(resp_body)}"
                )

            # 5xx — transient. Back off and try again.
            last_error = CloudSinkUnavailable(
                f"cloud returned HTTP {status}: {_safe_decode(resp_body)}"
            )
            self._sleep_backoff(attempt)

        raise CloudSinkUnavailable(
            f"giving up after {self.max_attempts} attempts: {last_error}"
        ) from (last_error if isinstance(last_error, BaseException) else None)

    def _sleep_backoff(self, attempt: int) -> None:
        # Exponential backoff with a small jitter so concurrent SDK users
        # don't reconverge on the same retry tick.
        delay = min(self.backoff_cap, self.backoff_base * (2 ** (attempt - 1)))
        delay += random.uniform(0, self.backoff_base)
        time.sleep(delay)


def _safe_decode(b: bytes) -> str:
    try:
        return b.decode("utf-8", errors="replace")[:500]
    except Exception:
        return repr(b[:200])


# ---------------------------------------------------------------------------
# CallableSink
# ---------------------------------------------------------------------------


class CallableSink:
    """Escape hatch: invoke an arbitrary callable per attestation."""

    sink_type = "CallableSink"

    def __init__(self, fn: Callable[[Attestation], Any]) -> None:
        if not callable(fn):
            raise TypeError("CallableSink expects a callable")
        self.fn = fn

    def emit(self, attestation: Attestation) -> None:
        self.fn(attestation)
