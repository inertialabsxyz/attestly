"""LangGraph integration for the Agent Work Protocol (AWP).

This is the GTM Phase 2 Step 5 scaffold of the package. The full SDK
surface (`attest`, `FileSink`, `CloudSink`, `CallableSink`, dual-agent
mode) lands in Step 3 — see ``planning/gtm-phase-2-plan.md`` for the
sequencing.

What ships in this release:

* :mod:`awp.langgraph.telemetry` — anonymous SDK telemetry, opt-out by
  default, used by the AWP team to identify conversion-ready OSS users.
  The opt-out switch (``AWP_TELEMETRY=0``) and the privacy guarantees
  are documented prominently in the package README.
"""

from awp.langgraph import telemetry
from awp.langgraph.telemetry import telemetry_disable, telemetry_enable

__version__ = "0.1.0a1"

__all__ = ["telemetry", "telemetry_disable", "telemetry_enable", "__version__"]
