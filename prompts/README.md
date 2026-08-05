# Agent Prompts — Attestly Production Checklist (Persona B pilot)

This folder decomposes [`docs/PRODUCTION_CHECKLIST.md`](../docs/PRODUCTION_CHECKLIST.md)
— everything that must be true before `attestly-cloud` can host a **paid
Persona B pilot** — into self-contained dispatch prompts for Claude Code agents
running in **isolated git worktrees**. Agents do not share state during
execution: each prompt is self-contained, hand-off context is explicit, and
parallel agents have hard file boundaries so they merge cleanly.

These prompts are authored per [`.claude/agent-prompts.md`](../.claude/agent-prompts.md)
and compose with the gate docs — every agent honours them, no prompt duplicates
their contents:

- [`.claude/commits.md`](../.claude/commits.md) — `make check` before every commit; `type(scope): description`
- [`.claude/testing.md`](../.claude/testing.md) — `make check` is the hard gate; every feature/fix needs a test
- [`.claude/review-gate.md`](../.claude/review-gate.md) — spawn a review agent before opening a PR
- [`.claude/pull-requests.md`](../.claude/pull-requests.md) — open a draft PR + post the Agent Run Report comment

The source of truth for **requirements** is `docs/PRODUCTION_CHECKLIST.md`. The
checklist's "Explicitly out of scope" section (on-chain anchoring, LLM
framework, S3 backend, SOC2, multi-region, Node SDK) is **deferred by design** —
**no agent here pulls any of it forward.**

---

## Sequencing Overview

```
Step 1  (serial, first)         Step 4  (serial, last)
┌──────────────────────────┐    ┌──────────────────────────────┐
│ P1: cloud deploy-         │    │ P4: deploy + Stripe + backup │
│     readiness code fixes  │    │     ops runbook (docs only)  │
│  • base_url single-source │    │  • fly deploy / DNS / TLS    │
│  • admin-key fail-fast    │    │  • Postgres + secrets        │
│  • healthz doc/impl match │    │  • Stripe test-mode cycle    │
└────────────┬─────────────┘    │  • automated backups + doc   │
             │                  └──────────────▲───────────────┘
   merged to main                              │
             │                        all code merged to main
   ┌─────────┴───────────────────────┐         │
   ▼                                 ▼         │
┌──────────────────────┐   ┌────────────────────────────┐
│ Step 2  (parallel)   │   │ Step 3  (parallel)         │
│ P2: prod hardening    │   │ P3: identity regression    │
│  (cloud/ Rust)        │   │     test (python/bindings) │
│  • rate limiting      │   │  • cross-process pubkey    │
│  • auth-audit test    │   │    stability test          │
│  • tamper-evidence     │   │  disjoint tree from P2     │
│    integration test    │   └────────────────────────────┘
└──────────────────────┘
```

**Rules**

- **Do not start Step 2 until Step 1 is merged to `main`.** Step 1 owns the
  shared `attestly-cloud` config surface (`src/state.rs`, `src/lib.rs`,
  `src/main.rs`, `src/handlers/share_links.rs`, `src/handlers/health.rs`) and
  pre-places the single-source base-URL accessor Step 2 will not touch.
- **Steps 2 and 3 run in parallel, in separate worktrees.** They touch
  **disjoint trees**: Step 2 is entirely under `services/attestly-cloud/`,
  Step 3 is entirely under `python/attestly-langgraph/` and
  `crates/attestly-python/`. They share no files and merge without coordinating.
- **Do not start Step 4 until Steps 1–3 are merged to `main`.** Step 4 is an
  ops runbook that deploys and exercises the merged code against a live host;
  it must reflect final code (base-URL behaviour, admin-key fail-fast, rate
  limits) and adds **no production Rust**.
- Each step's prompt lives in its own file:
  - [`step-1-cloud-deploy-readiness.md`](step-1-cloud-deploy-readiness.md)
  - [`step-2-production-hardening.md`](step-2-production-hardening.md)
  - [`step-3-identity-regression-test.md`](step-3-identity-regression-test.md)
  - [`step-4-deploy-and-billing-runbook.md`](step-4-deploy-and-billing-runbook.md)

## Coverage map (checklist item → step)

| PRODUCTION_CHECKLIST.md item | Step |
|---|---|
| §1 `ATTESTLY_CLOUD_BASE_URL` single-source (share links resolve to real host) | 1 |
| §1 `ATTESTLY_ADMIN_KEY` must be a strong secret (fail-fast on placeholder) | 1 |
| §1 `/healthz` behaviour matches its contract | 1 |
| §1 `flyctl deploy`, Postgres, blob volume, DNS/TLS, live smoke test | 4 |
| §2 Stripe secrets, webhook registration, test-mode Checkout→overage cycle, dashboard | 4 |
| §3 identity-survives-restart regression test | 3 |
| §4 auth: every `/v1/*` route requires a valid API key (audit test) | 2 |
| §4 rate limiting on ingest + share-link creation | 2 |
| §4 tamper-evidence end-to-end (422 `signature_invalid` + red banner) — integration test | 2 |
| §4 tamper-evidence confirmed **on the live host** | 4 |
| §4 structured logging + `5xx`/healthcheck alert | 4 |
| §4 automated Postgres backups + documented restore | 4 |

> Two checklist items are split across a code step and the ops step: §4
> tamper-evidence is proven by an **integration test in Step 2** and
> **confirmed against the live host in Step 4**; structured logging already
> exists in code (`TraceLayer` + `EnvFilter`), so Step 4 only wires the
> **alert** and documents it.
