"""LangGraph integration for Attestly.

The Step 3 surface lands here:

* :func:`attest` — one-line wrap for a LangGraph ``StateGraph`` that emits
  a signed attestation per node execution.
* :class:`FileSink`, :class:`CloudSink`, :class:`CallableSink` — pluggable
  destinations for emitted attestations.
* :mod:`attestly.langgraph.telemetry` — anonymous SDK telemetry (Step 5 scaffold)
  with the documented ``ATTESTLY_TELEMETRY=0`` opt-out.

All cryptographic primitives — canonical encoding, Ed25519 signing,
identity persistence — are delegated to ``attestly-core-py`` (Step 1) so the
SDK never re-implements signing in Python and inherits Step 1's
byte-identity guarantee with the Rust core.
"""

from attestly.langgraph import telemetry
from attestly.langgraph.sinks import (
    CallableSink,
    CloudSink,
    CloudSinkError,
    CloudSinkUnavailable,
    FileSink,
    Sink,
)
from attestly.langgraph.telemetry import telemetry_disable, telemetry_enable
from attestly.langgraph.wrapper import attest

__version__ = "0.1.0a1"

__all__ = [
    "attest",
    "Sink",
    "FileSink",
    "CloudSink",
    "CloudSinkError",
    "CloudSinkUnavailable",
    "CallableSink",
    "telemetry",
    "telemetry_disable",
    "telemetry_enable",
    "__version__",
]
