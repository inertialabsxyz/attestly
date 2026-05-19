"""Five-line snippet from the public quickstart at https://awp-cloud.xyz/quickstart.

This file exists so the documentation can ``include`` it instead of
copy-pasting strings into Markdown — keeping the docs and the SDK
in sync. The ``attest`` / ``CloudSink`` symbols land in Step 3; this
file is the placeholder that confirms the import path and env-var
contract the docs commit to.
"""

from __future__ import annotations

import os


def build_my_graph():
    """Stand-in for whatever ``StateGraph`` the user already has.

    The real KYC example will replace this in Step 3 — see
    ``planning/gtm-phase-2-plan.md`` for the sequencing.
    """
    raise NotImplementedError(
        "Step 3 wires the real LangGraph example. "
        "Until then, paste the snippet into your own agent."
    )


def main() -> None:
    # The five lines a quickstart user paste-installs:
    from awp.langgraph import attest, CloudSink  # noqa: F401 (Step 3 deliverable)
    from langgraph.graph import StateGraph  # noqa: F401

    graph = build_my_graph()
    graph = attest(
        graph,
        agent_id="quickstart-agent",
        sink=CloudSink(api_key=os.environ["AWP_API_KEY"]),
    )
    graph.invoke({"hello": "world"})


if __name__ == "__main__":
    main()
