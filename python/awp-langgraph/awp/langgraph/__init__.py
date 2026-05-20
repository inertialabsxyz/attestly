"""LangGraph integration for the Agent Work Protocol (AWP).

The Step 3 surface lands here:

* :func:`attest` — one-line wrap for a LangGraph ``StateGraph`` that emits
  a signed attestation per node execution.
* :class:`FileSink`, :class:`CloudSink`, :class:`CallableSink` — pluggable
  destinations for emitted attestations.
* :mod:`awp.langgraph.telemetry` — anonymous SDK telemetry (Step 5 scaffold)
  with the documented ``AWP_TELEMETRY=0`` opt-out.

All cryptographic primitives — canonical encoding, Ed25519 signing,
identity persistence — are delegated to ``awp-core-py`` (Step 1) so the
SDK never re-implements signing in Python and inherits Step 1's
byte-identity guarantee with the Rust core.
"""

from awp.langgraph import telemetry
from awp.langgraph.sinks import (
    CallableSink,
    CloudSink,
    CloudSinkError,
    CloudSinkUnavailable,
    FileSink,
    Sink,
)
from awp.langgraph.telemetry import telemetry_disable, telemetry_enable
from awp.langgraph.wrapper import attest

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
