"""Runnable version of the five-line snippet from the public quickstart
at https://awp-cloud.xyz/quickstart.

The docs walk through the same five lines (`quickstart.md`, step 3). This
file makes them runnable end-to-end so the quickstart can be smoke-tested:
``build_my_graph`` is a real one-node ``StateGraph`` standing in for
whatever graph the user already has.

Run it::

    AWP_API_KEY=<your key> python python/awp-langgraph/examples/quickstart_snippet.py

With ``AWP_API_KEY`` set it ships attestations to AWP Cloud via
``CloudSink``. Without it, the snippet falls back to ``FileSink`` so the
example still runs offline — the five-line cloud path is what the docs
show; the fallback just keeps this file runnable without a key.

For the full multi-scenario KYC walkthrough see ``examples/kyc_graph.py``.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import TypedDict

_REPO_ROOT = Path(__file__).resolve().parents[3]
_SDK_DIR = _REPO_ROOT / "python" / "awp-langgraph"
if _SDK_DIR.exists():
    sys.path.insert(0, str(_SDK_DIR))


class State(TypedDict, total=False):
    hello: str
    greeting: str


def build_my_graph():
    """A minimal one-node StateGraph — stand-in for the user's own graph."""
    from langgraph.graph import END, START, StateGraph

    def greet(state: State) -> dict:
        return {"greeting": f"hello, {state.get('hello', 'world')}"}

    g = StateGraph(State)
    g.add_node("greet", greet)
    g.add_edge(START, "greet")
    g.add_edge("greet", END)
    return g


def main() -> None:
    # The five lines a quickstart user paste-installs:
    from awp.langgraph import CloudSink, FileSink, attest

    graph = build_my_graph()
    api_key = os.environ.get("AWP_API_KEY")
    sink = (
        CloudSink(api_key=api_key)
        if api_key
        else FileSink(_REPO_ROOT / "data" / "attestations.jsonl")
    )
    attest(graph, agent_id="quickstart-agent", sink=sink)
    result = graph.compile().invoke({"hello": "world"})

    print(result["greeting"])
    if api_key:
        print("Signed attestation shipped to AWP Cloud — check your dashboard.")
    else:
        print(
            "AWP_API_KEY unset — signed attestation written to "
            f"{_REPO_ROOT / 'data' / 'attestations.jsonl'} via FileSink."
        )


if __name__ == "__main__":
    main()
