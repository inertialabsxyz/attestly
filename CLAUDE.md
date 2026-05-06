# CLAUDE.md

Guidance for Claude Code agents working in this repository. Keep this file lean — link to authoritative docs rather than duplicating them.

## What this repo is

**AWP (Agent Work Protocol)** — a minimal protocol for AI agents to produce cryptographically signed attestations of completed work, with optional on-chain anchoring for coordination and settlement.

Pitch and architecture: [`awp-landing-page-v2.md`](awp-landing-page-v2.md).
Market context: [`awp-market-research.md`](awp-market-research.md).

## Status

Pre-implementation. The repo currently contains planning docs and agent-workflow conventions — no source code yet. The 8-week prototype plan lives in [`planning/awp-prototype-plan.md`](planning/awp-prototype-plan.md); read it before starting any implementation work, since it defines phase boundaries, exit criteria, and the intended Cargo workspace layout (`crates/awp-core`, `crates/awp-agents`). Per-phase dispatch prompts live in [`planning/agent-prompts.md`](planning/agent-prompts.md).

## Stack (per the prototype plan)

- **Language:** Rust
- **Framework:** [AutoAgents](https://github.com/liquidos-ai/AutoAgents) (multi-agent, actor-based)
- **Crypto:** `ed25519-dalek` for signing, SHA-256 for hashing, `rs_merkle` for batching
- **Storage:** SQLite (`rusqlite`) under `./data/`
- **Chain integration:** deferred to Phase 2

If a dependency choice isn't covered by `planning/awp-prototype-plan.md`, ask before adding it.

## Working agreements

These are hard rules for any agent making changes. Read each before your first commit.

- [`.claude/commits.md`](.claude/commits.md) — `make check` before every commit; `type(scope): description` format; one logical change per commit.
- [`.claude/testing.md`](.claude/testing.md) — `make check` is the gate; every feature commit needs a test, every bug fix needs a regression test; clippy warnings cannot be silenced module/crate-wide.
- [`.claude/review-gate.md`](.claude/review-gate.md) — before opening any PR, spawn a review agent against the relevant phase of `awp-prototype-plan.md` and capture its structured report.
- [`.claude/pull-requests.md`](.claude/pull-requests.md) — open as **draft**, target `main`, post the Agent Run Report comment with the review report.
- [`.claude/agent-prompts.md`](.claude/agent-prompts.md) — when work is split across parallel Claude Code agents in worktrees, dispatch prompts follow this template.

## Operating notes

- **Planning docs are the source of truth for requirements.** When in doubt, point to the section of `planning/awp-prototype-plan.md` that drives the work, not to inferred conventions.
- **The `make check` gate doesn't exist yet** — there's no `Makefile` until the first crate lands. The first implementation PR should add it (running `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`) so subsequent agents have a real gate to honour.
- **Runtime outputs go in `./data/`** and stay out of git. Add `.gitignore` entries (`data/`, `attestations.json`, `target/`) the first time they're produced.
- **Don't invent scope.** If the prototype plan defers something to Phase 2 (on-chain anchoring, identity registration, HTTP API), don't pull it forward without an explicit ask.
