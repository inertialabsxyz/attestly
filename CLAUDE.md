# CLAUDE.md

Guidance for Claude Code agents working in this repository. Keep this file lean — link to authoritative docs rather than duplicating them.

## What this repo is

**Attestly** — a minimal protocol for AI agents to produce cryptographically signed attestations of completed work, with optional on-chain anchoring for coordination and settlement.

Pitch and architecture: [`attestly-landing-page-v2.md`](attestly-landing-page-v2.md).
System architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
Design decisions log: [`docs/DECISIONS.md`](docs/DECISIONS.md).
Market context: [`attestly-market-research.md`](attestly-market-research.md).

## Status

The 8-week prototype (Phases 1–4 of [`planning/attestly-prototype-plan.md`](planning/attestly-prototype-plan.md)) is implemented: attestations + signing, dispatcher/orchestration, Merkle batching, and a parallel-verifier evaluation. GTM build-out is underway — a hosted service and a Python SDK — per [`planning/gtm-phase-1-plan.md`](planning/gtm-phase-1-plan.md) and [`planning/gtm-phase-2-plan.md`](planning/gtm-phase-2-plan.md).

Read the relevant phase of `attestly-prototype-plan.md` (or the GTM plan) before starting work — it defines phase boundaries, exit criteria, and workspace layout. Per-phase dispatch prompts live in [`planning/agent-prompts.md`](planning/agent-prompts.md).

## Layout

- **`crates/attestly-core`** — the reference core: attestation types, canonical serialization, `ed25519-dalek` signing, SHA-256 hashing, `rs_merkle` batching, and SQLite (`rusqlite`) local storage under `./data/`. Framework-agnostic by design; knows nothing about agents or LLMs. Also ships the `attestly-verify` binary (stdin JSON → verify) used for cross-language checks.
- **`crates/attestly-agents`** — Worker/Verifier agents plus the Dispatcher/Batcher orchestration. Currently plain async traits with **no LLM framework wired** — see `docs/DECISIONS.md` for the framework decision (recommendation: Option C, a thin custom layer on Rig; not yet committed).
- **`crates/attestly-python`** — PyO3 bindings (`attestly-core-py`) exposing the Rust signing core to Python.
- **`python/attestly-langgraph`** — pure-Python LangGraph wrapper (`attestly.langgraph.attest`) on top of the bindings. This is the primary developer-facing SDK; **the crypto core stays in Rust and is bound, never reimplemented in Python.**
- **`services/attestly-cloud`** — hosted ingest/search/share-link service. Rust + `axum`, Postgres via `sqlx`, Stripe billing. **Its own Cargo sub-workspace** with an independent `Makefile` (recursed into by the root gate).
- **`crates/attestly-examples`** — runnable examples for the core/agents flows.
- **`tools/`** — `audit-viewer` (static, serverless attestation verifier) and `landing-page`.

## Quality gate

`make check` is the gate — it runs the core Rust workspace lint+test, the Python bindings and LangGraph pytest suites, and recurses into `services/attestly-cloud`. It **must pass before every commit** (see `.claude/commits.md`). `make fix` auto-formats and applies clippy fixes. The first `make check` builds a `.venv` and installs Python tooling, so it is slow on first run.

## Working agreements

These are hard rules for any agent making changes. Read each before your first commit.

- [`.claude/commits.md`](.claude/commits.md) — `make check` before every commit; `type(scope): description` format; one logical change per commit.
- [`.claude/testing.md`](.claude/testing.md) — `make check` is the gate; every feature commit needs a test, every bug fix needs a regression test; clippy warnings cannot be silenced module/crate-wide.
- [`.claude/review-gate.md`](.claude/review-gate.md) — before opening any PR, spawn a review agent against the relevant plan phase and capture its structured report.
- [`.claude/pull-requests.md`](.claude/pull-requests.md) — open as **draft**, target `main`, post the Agent Run Report comment with the review report.
- [`.claude/agent-prompts.md`](.claude/agent-prompts.md) — when work is split across parallel Claude Code agents in worktrees, dispatch prompts follow this template.

## Operating notes

- **Planning docs are the source of truth for requirements.** When in doubt, point to the section of the relevant plan (`planning/attestly-prototype-plan.md` or a `gtm-phase-*-plan.md`) that drives the work, not to inferred conventions. `docs/DECISIONS.md` records why past choices were made.
- **Runtime outputs go in `./data/`** and stay out of git (already covered by `.gitignore`, along with `target/`, `.venv/`, and per-package Python build artifacts).
- **Don't invent scope.** If a plan defers something (on-chain anchoring, identity registration) don't pull it forward without an explicit ask. If a dependency choice isn't covered by the plan, ask before adding it.
