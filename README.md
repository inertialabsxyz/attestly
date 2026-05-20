# AWP — Agent Work Protocol

**Signed attestations for AI agent work.**

AWP is a minimal protocol for AI agents to produce cryptographically
signed attestations of completed work, with optional on-chain anchoring
for coordination and settlement.

When agents work on your behalf, you need receipts — not just results.

## Install

The LangGraph SDK is published to **TestPyPI** while GTM Phase 2 is in
progress (promotion to `pypi.org` is gated on the first design-partner
close):

```bash
pip install --index-url https://test.pypi.org/simple/ awp-langgraph
```

Then wrap any LangGraph `StateGraph` in one line:

```python
from awp.langgraph import attest
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="my-agent-01")
graph.compile().invoke({"hello": "world"})
```

Every node execution emits a signed attestation. Writes to a local
JSONL file by default; swap in `CloudSink(api_key=...)` to ship
attestations to the hosted service for retention, search, and
share-links.

SDK reference, sink configuration, dual-agent mode, and the
anonymous-telemetry opt-out are documented in
[`python/awp-langgraph/README.md`](python/awp-langgraph/README.md).
The documentation site source (quickstart, concepts, self-hosted path,
compliance pointers) lives under [`docs/site/`](docs/site/).

To see the whole protocol working end-to-end, follow the presenter
runbook in [`docs/DEMO.md`](docs/DEMO.md).

## Status

GTM Phase 2 — SDK wedge and paid conversion. Shipped to `main`:

- **PyO3 bindings** ([`crates/awp-python/`](crates/awp-python/)) —
  the Rust signing core exposed to Python as `awp-core-py`, with a
  byte-identical-signature guarantee across Rust and Python.
- **`awp-cloud`** ([`services/awp-cloud/`](services/awp-cloud/)) —
  hosted ingest, search, share-links, account dashboard, Stripe
  billing, and a retention sweeper. Rust + Axum + Postgres.
- **LangGraph SDK** ([`python/awp-langgraph/`](python/awp-langgraph/)) —
  the one-line `attest()` wrapper, `FileSink` / `CloudSink` /
  `CallableSink`, dual-agent mode, and anonymous telemetry.
- **Documentation site** ([`docs/site/`](docs/site/)) and public
  pricing on the landing page.

Remaining for the phase: the LangSmith metadata integration and the
first design-partner close. See
[`planning/gtm-phase-2-plan.md`](planning/gtm-phase-2-plan.md) for the
full sequencing and
[`planning/gtm-phase-2-agent-prompts.md`](planning/gtm-phase-2-agent-prompts.md)
for the per-step dispatch prompts.

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

Rust core — Ed25519 signing, SHA-256 hashing, `rs_merkle` for batching,
SQLite for persistence. The Python SDK sits on top via PyO3 bindings
([`crates/awp-python/`](crates/awp-python/)). The hosted service is Rust
+ Axum + Postgres ([`services/awp-cloud/`](services/awp-cloud/)).

See [`planning/awp-prototype-plan.md`](planning/awp-prototype-plan.md)
for the full breakdown and weekly milestones; market context lives in
[`awp-market-research.md`](awp-market-research.md).

## Audit viewer

[`tools/audit-viewer/`](tools/audit-viewer/) is a single-file static
HTML viewer — no build step, no server. Drop an `attestations.jsonl`
(from the LangGraph SDK) or `attestations.json` + `executions.json`
(from the Rust examples) onto it and every signature is re-verified in
the browser. Editing a signed receipt flips its row red. It is the
buyer-facing artefact for the audit story.

## Reference implementation — Rust examples

The Rust workspace ships the canonical implementations of every
protocol layer. They are the **reference implementation** the Python
SDK, the audit viewer, and the hosted service all verify against
byte-for-byte. Use them to understand the protocol; use the Python
SDK above to integrate.

Examples live in
[`crates/awp-examples/examples/`](crates/awp-examples/examples/) and run
with `cargo run --example <name>`. Each appends to
`data/attestations.json` and `data/executions.json` so you can re-run
them and inspect the growing log.

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
