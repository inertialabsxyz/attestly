# AWP — Agent Work Protocol

**Signed attestations for AI agent work.**

AWP is a minimal protocol for AI agents to produce cryptographically
signed attestations of completed work. It is designed to support
optional on-chain anchoring for coordination and settlement — that
anchoring is a roadmap item, not yet implemented (see [Roadmap](#roadmap)).

When agents work on your behalf, you need receipts — not just results.

## Install

The LangGraph SDK is not yet published to a package index — install it
from this repository. It sits on `awp-core-py`, the PyO3 bindings to the
Rust signing core, which you build locally with
[`maturin`](https://www.maturin.rs):

```bash
git clone https://github.com/inertialabsxyz/awp
cd awp

python -m venv .venv && source .venv/bin/activate
pip install --upgrade pip   # editable installs need pip >= 21.3
pip install maturin

# Build the Rust signing core into the venv as `awp-core-py`.
maturin develop --manifest-path crates/awp-python/Cargo.toml --release

# Install the LangGraph SDK in editable mode (--no-deps: awp-core-py is
# already built above; langgraph is installed on the next line).
pip install --no-deps -e python/awp-langgraph
pip install langgraph
```

Then wrap any LangGraph `StateGraph` in one line:

```python
from awp.langgraph import attest
from langgraph.graph import StateGraph

graph = build_my_graph()                       # your existing StateGraph
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
trees, and each batch carries reserved `anchor_tx` / `anchor_chain`
fields: once on-chain anchoring lands, a root can be published so
multiple parties verify inclusion without trusting a central server.
Today the Merkle batching and inclusion proofs are real and local; the
anchoring step is not yet built.

## Design principles

- **Blockchain only where necessary.** Computation stays off-chain;
  anchoring, when added, carries only Merkle roots.
- **No abstraction without example.** Every protocol element maps to a
  concrete use case.
- **Graceful degradation.** Fully functional with no chain at all —
  on-chain anchoring is additive, never a dependency.
- **Identity over keys.** Stable identity across key rotations, with
  lineage and operator accountability.

## What AWP is not

Not a token. Not a DAO. Not on-chain AI. Not a reputation score.

## Roadmap

On-chain anchoring is the headline not-yet-built capability: the Merkle
batch type already reserves `anchor_tx` / `anchor_chain` fields, but no
chain is wired in. It is explicitly out of scope for the current GTM
phase and revisited later. Near-term work tracks the GTM Phase 2 plan
([`planning/gtm-phase-2-plan.md`](planning/gtm-phase-2-plan.md)) — the
SDK wedge and hosted service are in; the LangSmith integration and the
first design-partner close are the remaining items.

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
