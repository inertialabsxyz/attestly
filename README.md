# AWP — Agent Work Protocol

**Signed attestations for AI agent work.**

AWP is a minimal protocol for AI agents to produce cryptographically signed attestations of completed work, with optional on-chain anchoring for coordination and settlement.

When agents work on your behalf, you need receipts — not just results.

## Status

Pre-implementation. The repository currently contains planning and design docs:

- [`awp-landing-page-v2.md`](awp-landing-page-v2.md) — pitch, architecture, FAQ
- [`planning/awp-prototype-plan.md`](planning/awp-prototype-plan.md) — 8-week prototype plan (Worker → Verifier → Dispatcher → Batching)
- [`planning/agent-prompts.md`](planning/agent-prompts.md) — per-phase Claude Code dispatch prompts
- [`awp-market-research.md`](awp-market-research.md) — landscape and positioning

A working Worker–Verifier prototype is the starting point for Phase 1. On-chain anchoring is deferred to Phase 2.

## Core idea

Every task produces a signed attestation linking the agent's identity, the task, and the output:

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

A second agent independently verifies. Attestations batch into Merkle trees; roots can anchor on-chain so multiple parties can verify inclusion without trusting a central server.

## Design principles

- **Blockchain only where necessary.** Computation stays off-chain.
- **No abstraction without example.** Every protocol element maps to a concrete use case.
- **Graceful degradation.** Functional when the chain is unavailable.
- **Identity over keys.** Stable identity across key rotations, with lineage and operator accountability.

## What AWP is not

Not a token. Not a DAO. Not on-chain AI. Not a reputation score.

## Stack

Rust, [AutoAgents](https://github.com/liquidos-ai/AutoAgents), Ed25519, Merkle trees, SQLite. See [`planning/awp-prototype-plan.md`](planning/awp-prototype-plan.md) for the full breakdown and weekly milestones.

## Examples

All examples live in [`examples/`](examples/) and are runnable from the workspace root with `cargo run --example <name>`. Each appends to `data/attestations.json` and `data/executions.json` so you can re-run them and inspect the growing log.

- **`kyc_receipts`** — the headline demo. Replaces the arithmetic task with a deterministic KYC (Know Your Customer) decision rule and runs three scenarios end-to-end: a clean **Approve**, a clean **Flag**, and a **tampered** receipt where the Worker's signed output is mutated after signing. The Verifier independently re-decides each request and the tampered scenario is caught via signature mismatch with a customer-resonant rejection. Pair with [`docs/USER_JOURNEYS.md`](docs/USER_JOURNEYS.md) (Persona A — compliance lead Sarah).
- **`dispatcher_flow`** — Dispatcher-coordinated Worker → Verifier with per-stage timeouts; covers the happy path, a Worker-stage timeout, and a Verifier disagreement.
- **`full_pipeline`** — adds the Phase 3 Batcher: attestations are batched into Merkle trees, persisted to SQLite, and inclusion-proven.
- **`parallel_verifiers`** — Phase 4 stretch task. N verifiers run concurrently against one Worker; disagreement detection surfaces minority dissent.
- **`simple_attestation`** — Phase 1 checkpoint. Minimal Worker → Verifier round trip without the Dispatcher.

## Contributing

The repo follows a structured agent workflow defined in [`.claude/`](.claude/) — commits, testing, review gate, PRs, and dispatch prompts for parallel agents. If you're contributing (human or agent), start there.

---

Built by [Inertia Labs](https://inertialabs.xyz).
