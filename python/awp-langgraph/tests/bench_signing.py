"""Microbenchmark — per-node signing overhead.

Phase 2 target: <10ms per node on a modern laptop. Signing is delegated to
``awp-core-py``, which does the Ed25519 work in Rust; the realistic Python-
side cost is dict construction + the FFI hop. Measure rather than assume.

Run with::

    pytest python/awp-langgraph/tests/bench_signing.py -v -s

The measured number should be pasted into the package README's
"Performance" section whenever the signing path changes.
"""

from __future__ import annotations

import time

# Import awp-core-py via its C-extension submodule — `awp` is a namespace
# package shared with the SDK. See awp/langgraph/sinks.py for the rationale.
from awp._native import AgentIdentity, sign_attestation, verify_attestation

# Soft ceiling is the spec's <10ms target; the hard ceiling is what the
# test actually fails on, set well above to avoid flaking in shared CI.
SOFT_CEILING_MS = 10.0
HARD_CEILING_MS = 25.0


def _measure_ms(fn, n: int = 500) -> float:
    for _ in range(50):  # warmup
        fn()
    start = time.perf_counter()
    for _ in range(n):
        fn()
    elapsed = time.perf_counter() - start
    return (elapsed / n) * 1000.0


def test_bench_full_sign_under_budget() -> None:
    ident = AgentIdentity.generate("bench-agent")

    def one_sign():
        payload = {
            "node_name": "bench_node",
            "input_hash": "a" * 64,
            "output_hash": "b" * 64,
            "agent_id": "bench-agent",
            "timestamp": 1_700_000_000,
        }
        return sign_attestation(payload, ident, timestamp=1_700_000_000)

    per_call_ms = _measure_ms(one_sign)
    print(f"\n[bench] full sign (build payload + sign_attestation): "
          f"{per_call_ms:.4f} ms/call")
    assert per_call_ms < HARD_CEILING_MS, (
        f"signing overhead {per_call_ms:.2f}ms exceeds {HARD_CEILING_MS}ms hard cap "
        f"(soft target: <{SOFT_CEILING_MS}ms per the Phase 2 spec)"
    )


def test_bench_verify_under_budget() -> None:
    ident = AgentIdentity.generate("bench-agent")
    att = sign_attestation(
        {"node_name": "bench_node", "value": "x"}, ident, timestamp=1_700_000_000
    )
    pubkey = ident.public_key

    per_call_ms = _measure_ms(lambda: verify_attestation(att, pubkey))
    print(f"[bench] verify_attestation: {per_call_ms:.4f} ms/call")
    assert per_call_ms < HARD_CEILING_MS
