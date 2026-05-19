# AWP — Agent Work Protocol

**Signed attestations for AI agent work.**

AWP is a minimal protocol for AI agents to produce cryptographically
signed attestations of completed work, with optional on-chain anchoring
for coordination and settlement.

When agents work on your behalf, you need receipts — not just results.

## Install

```bash
pip install awp-langgraph
```

Then wrap any LangGraph `StateGraph` in one line:

```python
from awp.langgraph import attest
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="my-agent-01")
graph.invoke({"hello": "world"})
```

Every node execution emits a signed attestation. Writes to a local
JSONL file by default; swap in `CloudSink(api_key=...)` to ship
attestations to [AWP Cloud](https://awp-cloud.xyz) for retention,
search, and share-links.

The full SDK reference, anonymous-telemetry opt-out, and self-hosted
path live at [`docs.awp-cloud.xyz`](https://docs.awp-cloud.xyz)
(source under [`docs/site/`](docs/site/)). The 60-second quickstart
flow is at [`awp-cloud.xyz/quickstart`](https://awp-cloud.xyz/quickstart).

## Status

GTM Phase 2. The hosted service ([`services/awp-cloud/`](services/awp-cloud/)),
public pricing, and Stripe billing are live. The LangGraph SDK
([`python/awp-langgraph/`](python/awp-langgraph/)) ships the
telemetry module and package layout today; the full `attest()` API
surface lands as part of Step 3 of the phase — see
[`planning/gtm-phase-2-plan.md`](planning/gtm-phase-2-plan.md) for the
sequencing.

## Core idea

Every task produces a signed attestation linking the agent's identity,
the task, and the output:

```
Attestation {
    agent_id + public_key
    task_hash
    output_hash
    status
    timestamp
    signature
}
```

A second agent independently verifies. Attestations batch into Merkle
trees; roots can anchor on-chain so multiple parties can verify
inclusion without trusting a central server.

## Design principles

- **Blockchain only where necessary.** Computation stays off-chain.
- **No abstraction without example.** Every protocol element maps to a
  concrete use case.
- **Graceful degradation.** Functional when the chain is unavailable.
- **Identity over keys.** Stable identity across key rotations, with
  lineage and operator accountability.

## What AWP is not

Not a token. Not a DAO. Not on-chain AI. Not a reputation score.

## Stack

Rust core, [AutoAgents](https://github.com/liquidos-ai/AutoAgents),
Ed25519, Merkle trees, SQLite. Python SDK on top via PyO3 bindings
([`crates/awp-python/`](crates/awp-python/)). Hosted service in Rust
+ Axum + Postgres ([`services/awp-cloud/`](services/awp-cloud/)).
See [`planning/awp-prototype-plan.md`](planning/awp-prototype-plan.md)
for the full breakdown and weekly milestones; market context lives in
[`awp-market-research.md`](awp-market-research.md).

## Reference implementation — Rust examples

The Rust workspace ships the canonical implementations of every
protocol layer. They are the **reference implementation** the Python
SDK, the audit viewer, and the hosted service all verify against
byte-for-byte. Use them to understand the protocol; use the Python
SDK above to integrate.

All examples live in [`examples/`](examples/) and are runnable with
`cargo run --example <name>`. Each appends to `data/attestations.json`
and `data/executions.json` so you can re-run them and inspect the
growing log.

- **`kyc_receipts`** — the headline demo. Deterministic KYC decision
  rule run end-to-end across three scenarios: a clean **Approve**, a
  clean **Flag**, and a **tampered** receipt where the Worker's signed
  output is mutated after signing. Pair with
  [`docs/USER_JOURNEYS.md`](docs/USER_JOURNEYS.md) (Persona A — Sarah).
- **`dispatcher_flow`** — Dispatcher-coordinated Worker → Verifier with
  per-stage timeouts.
- **`full_pipeline`** — adds the Phase 3 Batcher: Merkle batching and
  inclusion proofs against SQLite.
- **`parallel_verifiers`** — N verifiers concurrent against one Worker;
  surfaces minority dissent.
- **`simple_attestation`** — minimal Worker → Verifier round trip.

## Contributing

The repo follows a structured agent workflow defined in
[`.claude/`](.claude/) — commits, testing, review gate, PRs, and
dispatch prompts for parallel agents. If you're contributing (human or
agent), start there.

---

Built by [Inertia Labs](https://inertialabs.xyz).
