"""End-to-end: wrap a real LangGraph StateGraph; verify attestations.

Covers the Step 3 exit criteria: one attestation per node, every emitted
attestation verifies via attestly-core-py (the same path the static audit
viewer uses), idempotent wrap, behaviour-preserving outputs, exception
propagation, post-attest add_node coverage, and dual-agent mode including
the disagreement verdict.
"""

from __future__ import annotations

import json
from typing import TypedDict

import pytest

# Import attestly-core-py via its C-extension submodule — see sinks.py rationale.
from attestly._native import AgentIdentity, verify_attestation  # noqa: E402

pytest.importorskip("langgraph")

from langgraph.graph import END, START, StateGraph  # noqa: E402

from attestly.langgraph import CallableSink, FileSink, attest  # noqa: E402


class _S(TypedDict, total=False):
    x: int
    plus_one: int
    times_two: int
    final: int


def _add_one(state: _S) -> dict:
    return {"plus_one": state["x"] + 1}


def _times_two(state: _S) -> dict:
    return {"times_two": state["plus_one"] * 2}


def _finalise(state: _S) -> dict:
    return {"final": state["times_two"]}


def _build_three_node_graph():
    g = StateGraph(_S)
    g.add_node("add_one", _add_one)
    g.add_node("times_two", _times_two)
    g.add_node("finalise", _finalise)
    g.add_edge(START, "add_one")
    g.add_edge("add_one", "times_two")
    g.add_edge("times_two", "finalise")
    g.add_edge("finalise", END)
    return g


def test_three_node_graph_emits_one_attestation_per_node(tmp_path) -> None:
    sink_path = tmp_path / "att.jsonl"
    g = _build_three_node_graph()
    attest(
        g,
        agent_id="worker-1",
        sink=FileSink(sink_path),
        identities_dir=tmp_path / "ids",
    )
    result = g.compile().invoke({"x": 5})
    # Behaviour preserved: same final value as the un-wrapped graph.
    assert result["final"] == (5 + 1) * 2

    lines = [
        json.loads(line)
        for line in sink_path.read_text().splitlines()
        if line.strip()
    ]
    assert len(lines) == 3
    assert {att["agent_id"] for att in lines} == {"worker-1"}


def test_every_emitted_attestation_verifies(tmp_path) -> None:
    """Every emitted attestation must verify via attestly-core-py — the exact
    verification path the static audit viewer reproduces in the browser.
    """
    received: list = []
    g = _build_three_node_graph()
    attest(
        g,
        agent_id="worker-1",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.compile().invoke({"x": 5})

    assert len(received) == 3
    for att in received:
        assert verify_attestation(att, att.agent_pubkey) is True


def test_wrap_is_idempotent(tmp_path) -> None:
    """Calling attest() twice on the same graph must not double-emit."""
    received = []
    g = _build_three_node_graph()
    sink = CallableSink(received.append)
    attest(g, agent_id="w", sink=sink, identities_dir=tmp_path / "ids")
    attest(g, agent_id="w", sink=sink, identities_dir=tmp_path / "ids")
    g.compile().invoke({"x": 1})
    assert len(received) == 3, f"expected 3 attestations, got {len(received)}"


def test_behaviour_preserving_outputs_unchanged(tmp_path) -> None:
    """Wrapped graph must produce identical state to the un-wrapped graph."""
    unwrapped = _build_three_node_graph().compile().invoke({"x": 7})

    wrapped_graph = _build_three_node_graph()
    attest(
        wrapped_graph,
        agent_id="w",
        sink=CallableSink(lambda a: None),
        identities_dir=tmp_path / "ids",
    )
    wrapped = wrapped_graph.compile().invoke({"x": 7})

    assert wrapped == unwrapped


def test_node_exception_propagates_and_no_attestation_emitted(tmp_path) -> None:
    def boom(_state):
        raise RuntimeError("kaboom")

    g = StateGraph(_S)
    g.add_node("boom", boom)
    g.add_edge(START, "boom")
    g.add_edge("boom", END)

    received = []
    attest(
        g,
        agent_id="w",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    with pytest.raises(Exception):
        g.compile().invoke({"x": 1})
    assert received == []  # no receipt for incomplete work


def test_post_attest_add_node_also_wraps(tmp_path) -> None:
    """add_node calls *after* attest() must also be wrapped."""
    received = []
    g = StateGraph(_S)
    attest(
        g,
        agent_id="w",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.add_node("add_one", _add_one)
    g.add_node("times_two", _times_two)
    g.add_edge(START, "add_one")
    g.add_edge("add_one", "times_two")
    g.add_edge("times_two", END)
    g.compile().invoke({"x": 1})
    assert len(received) == 2


def test_dual_agent_emits_worker_and_verifier_attestations(tmp_path) -> None:
    received: list = []
    g = _build_three_node_graph()
    attest(
        g,
        agent_id="worker-1",
        verifier_agent_id="verifier-1",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.compile().invoke({"x": 5})

    # 3 nodes * 2 attestations each
    assert len(received) == 6

    agents = [att.agent_id for att in received]
    assert agents.count("worker-1") == 3
    assert agents.count("verifier-1") == 3

    # Every emitted attestation still verifies cryptographically.
    for att in received:
        assert verify_attestation(att, att.agent_pubkey) is True


def test_dual_agent_disagreement_records_answer_correct_false(tmp_path) -> None:
    """A non-deterministic node that returns a different output on the
    verifier replay must produce a Verifier attestation whose signed
    payload records answer_correct=false.
    """
    counter = {"calls": 0}

    def flaky(state: _S) -> dict:
        counter["calls"] += 1
        # First call and second (verifier replay) call return different values.
        return {"plus_one": state["x"] + counter["calls"]}

    g = StateGraph(_S)
    g.add_node("flaky", flaky)
    g.add_edge(START, "flaky")
    g.add_edge("flaky", END)

    received: list = []
    attest(
        g,
        agent_id="worker-1",
        verifier_agent_id="verifier-1",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.compile().invoke({"x": 10})

    verifier_atts = [a for a in received if a.agent_id == "verifier-1"]
    assert len(verifier_atts) == 1
    # The verdict is embedded in the signed payload, surfaced via `output`
    # (the canonical JSON string of the payload dict).
    payload = json.loads(verifier_atts[0].output)
    assert payload["answer_correct"] is False
    assert payload["attestation_valid"] is True
    # And it references the Worker attestation it checked.
    worker_ids = {a.id for a in received if a.agent_id == "worker-1"}
    assert payload["references"] in worker_ids


def test_dual_agent_agreement_records_answer_correct_true(tmp_path) -> None:
    """A deterministic node produces matching Worker/Verifier outputs and
    the Verifier records answer_correct=true.
    """
    g = StateGraph(_S)
    g.add_node("add_one", _add_one)
    g.add_edge(START, "add_one")
    g.add_edge("add_one", END)

    received: list = []
    attest(
        g,
        agent_id="worker-1",
        verifier_agent_id="verifier-1",
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.compile().invoke({"x": 3})

    verifier_atts = [a for a in received if a.agent_id == "verifier-1"]
    assert len(verifier_atts) == 1
    payload = json.loads(verifier_atts[0].output)
    assert payload["answer_correct"] is True


def test_explicit_identity_overrides_file_store(tmp_path) -> None:
    explicit = AgentIdentity.generate("override-agent")
    received: list = []
    g = _build_three_node_graph()
    attest(
        g,
        agent_id="override-agent",
        identity=explicit,
        sink=CallableSink(received.append),
        identities_dir=tmp_path / "ids",
    )
    g.compile().invoke({"x": 1})
    assert {a.agent_pubkey for a in received} == {explicit.public_key}
    # No file was written for that agent in the store directory.
    assert not (tmp_path / "ids" / "override-agent.json").exists()
