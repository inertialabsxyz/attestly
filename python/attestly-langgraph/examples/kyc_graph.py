"""GTM Phase 2 — LangGraph version of the KYC Receipts demo.

Mirrors the three-scenario flow of ``examples/kyc_receipts.rs`` from GTM
Phase 1, but using a LangGraph ``StateGraph`` wrapped with
``attestly.langgraph.attest`` so every node execution emits a signed
attestation.

Scenarios printed in order:

1. **Approve** — small domestic card transaction. Worker approves.
2. **Flag**    — large amount. Worker flags with reason "amount exceeds
   threshold".
3. **Tampered** — Same flow run under dual-agent mode with a screening
   node whose verifier replay is forced to disagree (simulating a
   non-deterministic LLM). The Verifier attestation records
   ``answer_correct: false`` and the disagreement is surfaced as a
   warning log line — the graph still runs to completion.

Run with::

    python python/attestly-langgraph/examples/kyc_graph.py

Writes signed attestations to ``data/attestations.jsonl`` (loadable by the
static audit viewer at ``tools/audit-viewer/index.html``).

Optional flags::

    --sink cloud      Switch to CloudSink. Needs ATTESTLY_API_KEY set and an
                      attestly-cloud instance reachable at ATTESTLY_CLOUD_ENDPOINT
                      (default http://localhost:8080).
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import TypedDict

# Make the in-tree SDK importable when running from a fresh clone.
_REPO_ROOT = Path(__file__).resolve().parents[3]
_SDK_DIR = _REPO_ROOT / "python" / "attestly-langgraph"
if _SDK_DIR.exists():
    sys.path.insert(0, str(_SDK_DIR))

try:
    from attestly.langgraph import CloudSink, FileSink, attest  # noqa: E402
except ImportError as exc:  # pragma: no cover - friendly error
    print(
        f"error: could not import the attestly-langgraph SDK ({exc}).\n"
        "       Install attestly-core-py (`maturin develop` in crates/attestly-python/) "
        "and langgraph first.",
        file=sys.stderr,
    )
    raise SystemExit(1)

try:
    from langgraph.graph import END, START, StateGraph  # noqa: E402
except ImportError:  # pragma: no cover
    print("error: this example needs `langgraph` — pip install langgraph", file=sys.stderr)
    raise SystemExit(1)


# ---------------------------------------------------------------------------
# KYC state + nodes
# ---------------------------------------------------------------------------


class KycState(TypedDict, total=False):
    customer_id: str
    amount_cents: int
    country: str
    rail: str
    decision: str
    reason: str


_FLAG_AMOUNT_CENTS = 1_000_000  # $10,000
_ALLOW_LIST = {"US", "CA", "GB", "DE", "FR", "ES", "IT", "NL"}


def screen_node(state: KycState) -> dict:
    """Mirror ``KycWorker::decide`` from ``crates/attestly-agents/src/kyc_agents.rs``."""
    amount = state.get("amount_cents", 0)
    country = state.get("country", "")
    if amount > _FLAG_AMOUNT_CENTS:
        return {"decision": "FLAG", "reason": "amount exceeds threshold"}
    if country not in _ALLOW_LIST:
        return {"decision": "FLAG", "reason": f"country {country} not in allow-list"}
    return {"decision": "APPROVE", "reason": ""}


def render_node(state: KycState) -> dict:
    """Render the decision as a customer-facing line."""
    decision = state.get("decision", "?")
    reason = state.get("reason", "")
    customer = state.get("customer_id", "?")
    if decision == "APPROVE":
        return {"reason": f"Customer #{customer}: approved."}
    if decision == "FLAG":
        return {"reason": f"Customer #{customer}: flagged — {reason}."}
    return {"reason": f"Customer #{customer}: unknown decision."}


_DISAGREE = {"trigger": False}


def disagreeing_screen_node(state: KycState) -> dict:
    """Same as ``screen_node`` but every other invocation returns the
    opposite decision. Simulates a non-deterministic LLM under dual-agent
    mode for the tampered scenario.
    """
    base = screen_node(state)
    if _DISAGREE["trigger"]:
        _DISAGREE["trigger"] = False  # flip on the verifier replay, then reset
        flipped = "FLAG" if base["decision"] == "APPROVE" else "APPROVE"
        return {"decision": flipped, "reason": "verifier reached opposite conclusion"}
    _DISAGREE["trigger"] = True
    return base


# ---------------------------------------------------------------------------
# Graph builder
# ---------------------------------------------------------------------------


def build_graph(*, with_disagreement: bool = False):
    g = StateGraph(KycState)
    g.add_node("screen", disagreeing_screen_node if with_disagreement else screen_node)
    g.add_node("render", render_node)
    g.add_edge(START, "screen")
    g.add_edge("screen", "render")
    g.add_edge("render", END)
    return g


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def _resolve_sink(arg: str):
    if arg == "cloud":
        api_key = os.environ.get("ATTESTLY_API_KEY")
        if not api_key:
            print("error: --sink cloud requires ATTESTLY_API_KEY to be set", file=sys.stderr)
            raise SystemExit(2)
        endpoint = os.environ.get("ATTESTLY_CLOUD_ENDPOINT", "http://localhost:8080")
        return CloudSink(api_key=api_key, endpoint=endpoint)
    return FileSink(_REPO_ROOT / "data" / "attestations.jsonl")


def main() -> int:
    parser = argparse.ArgumentParser(description="Attestly LangGraph KYC demo")
    parser.add_argument(
        "--sink",
        choices=["file", "cloud"],
        default="file",
        help="Where to write attestations (default: file → data/attestations.jsonl)",
    )
    args = parser.parse_args()

    sink = _resolve_sink(args.sink)

    # Scenario 1: Approve --------------------------------------------------
    g1 = build_graph()
    attest(g1, agent_id="agent-kyc-01", sink=sink)
    print("\n--- Scenario 1: Approve ---")
    s1 = g1.compile().invoke(
        {"customer_id": "4711", "amount_cents": 8_999, "country": "US", "rail": "card"}
    )
    print(s1["reason"])

    # Scenario 2: Flag -----------------------------------------------------
    g2 = build_graph()
    attest(g2, agent_id="agent-kyc-01", sink=sink)
    print("\n--- Scenario 2: Flag (large amount) ---")
    s2 = g2.compile().invoke(
        {
            "customer_id": "8842",
            "amount_cents": 5_000_000,
            "country": "US",
            "rail": "card",
        }
    )
    print(s2["reason"])

    # Scenario 3: Tampered (dual-agent disagreement) ----------------------
    _DISAGREE["trigger"] = False  # reset
    g3 = build_graph(with_disagreement=True)
    attest(
        g3,
        agent_id="agent-kyc-01",
        verifier_agent_id="agent-kyc-02",
        sink=sink,
    )
    print("\n--- Scenario 3: Tampered (verifier disagrees) ---")
    s3 = g3.compile().invoke(
        {
            "customer_id": "9999",
            "amount_cents": 2_500_000,
            "country": "US",
            "rail": "card",
        }
    )
    print(s3["reason"])
    print(
        "(check the log: the Verifier emitted answer_correct=false for the "
        "screen node — disagreement is surfaced as a warning, not an "
        "exception, so the graph still completes.)"
    )

    if args.sink == "file":
        path = _REPO_ROOT / "data" / "attestations.jsonl"
        print(f"\nAttestations written to: {path}")
        print(
            "Open tools/audit-viewer/index.html and drag in this file to "
            "verify every signature in the browser."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
