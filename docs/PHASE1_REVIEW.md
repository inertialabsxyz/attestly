# Phase 1 Review — Reading of the Postmortem and Decisions

A reading of the prototype's Phase 4 deliverables (`docs/DECISIONS.md`, `docs/PAIN_POINTS.md`, and the "Phase 1 Postmortem" section appended to `planning/attestly-prototype-plan.md`), with a recommended next move.

This is not new analysis — every claim here cites a section of the three source files. Treat this file as a synthesised executive summary, not a separate decision record.

## Numbers (from the Postmortem)

- **Scope:** 100% of planned exit criteria across all 4 phases. No net-new beyond plan, no cuts.
- **Code:** ~5,500 LOC across `attestly-core` + `attestly-agents` + examples. 24 implementation commits.
- **Tests:** 79 passing (46 in `attestly-core`, 33 in `attestly-agents`) + 9 integration tests.
- **Friction logged:** 12 documented pain points across the three implementation phases.

## The big finding

**AutoAgents — the framework named "primary" in the plan — has zero lines of code in the shipped implementation.** Worker, Verifier, Dispatcher, ParallelDispatcher, and Batcher are all plain async traits + tokio. The framework's `AgentBuilder.run()` requires a live LLM provider, which would either break `make check` hermeticity (real LLM calls in CI) or make example binaries unrunnable without an API key — so the agents implemented around it.

This is the single largest gap between what the plan named and what got shipped. Every coordination decision after Phase 1 was made *around* this gap, not *with* the framework's help.

## The five recurring pain points

From `docs/PAIN_POINTS.md` "Phase 4 Synthesis", ordered by impact on framework choice:

1. **No documented worker-verifier pattern in AutoAgents.** The core friction. Six weeks in, the framework never enters the call path.
2. **Dual `verify_attestation` surfaces** — string-JSON for tools, typed `&Attestation` for in-process. Currently ~5 lines of glue, but foreshadows real cost when LLM-driven tool calls land: every typed value needs a serialised twin.
3. **`TaskExecution` is hard-coded to one verifier.** Phase 4's parallel-verifiers stretch task immediately ran into it; required a `ParallelExecution` type plus an `as_task_execution` projection shim to keep the Phase 2 reader working.
4. **Three storage models for one prototype.** `attestations.json` (Phase 1) + `executions.json` (Phase 2) + `attestly.db` SQLite (Phase 3) all live simultaneously, with overlapping semantics. Fine for a prototype, untenable for production.
5. **Liveness backstop wakeups.** The Batcher polls once/second forever for time-based triggers. Invisible at one Batcher, a real cost at production scale.

## Framework recommendation

**Option C — Thin custom layer on Rig**, with **medium confidence**.

| Framework | Source | Weighted score |
|-----------|--------|----------------|
| AutoAgents | Lived (6 weeks) | 2.7 / 5 |
| swarms-rs | Reviewed (docs only) | 2.7 / 5 |
| **Rig** | Reviewed (docs only) | **4.0 / 5** |

The argument:

- **Coordination is simpler than a framework.** Every shape shipped (Worker→Verifier, Worker→N-Verifiers, time-or-count Batcher) is 10–50 lines of `tokio` + `futures` glue. No actor model, pub/sub, or supervisor was load-bearing.
- **The friction was framework-shaped, not coordination-shaped.** The dual-surface tool problem and the hermeticity problem both go away if we own the agent loop and call Rig only for completions.
- **Rig has the strongest evidence base.** 7.2k stars, mature docs, named production users.
- **`attestly-core` is already framework-agnostic.** A swap touches `attestly-agents` only — the plan's "Minimal framework coupling in attestly-core" risk-mitigation row was honoured.

## The strongest counter-argument (from DECISIONS.md itself)

**AutoAgents lock-in is currently zero.** "Stay with AutoAgents" and "switch to Rig" are practically indistinguishable *until* someone wires real LLMs into Worker/Verifier. If the next milestone is on-chain anchoring or identity registration — work that doesn't touch the agent loop — the decision can be deferred with the same evidence still applying.

The day to commit to Rig is the day someone wants the Worker to actually call an LLM.

## Three product decisions waiting for the human

Flagged in `docs/DECISIONS.md` and `docs/PAIN_POINTS.md` as explicitly the human's call, not the agents':

1. **Framework choice** (defer or commit). Per above.
2. **Strict vs. best-effort parallel-verifier policy.** Currently strict (any verifier failure = stage failure). One-line change to best-effort using `join_all` instead of `try_join_all`. Phase 4 left it for the human.
3. **Storage consolidation.** Three models is fine for prototype, untenable for production. Pick one before Phase 2-of-Attestly work compounds the divergence.

## Suggested next move

The Postmortem is honest that the framework decision can be deferred — the swap is contained either way. The pain points that *don't* get easier with delay are #3 (`TaskExecution`'s hard-coded single-verifier shape) and #4 (three storage models). Both compound the longer they sit.

A defensible *protocol-driven* sequencing:

1. **Generalise the coordination type** before the second multi-verifier shape lands. The Phase 4 postmortem already calls this out: "Phase 2 of Attestly overall should generalise the coordination type before the second pattern lands, not after."
2. **Consolidate storage** to a single canonical model (probably SQLite, since it's already the source of truth for batches). Keep JSON exporters for compatibility but drop them as the source of truth.
3. **Then start on-chain anchoring** with a clean coordination type and a single storage model.
4. **Defer the framework decision** until LLM integration is the next-task. The 4-hour Rig spike suggested in DECISIONS.md is a cheap way to convert "medium confidence" to "high" when that day arrives.

> **Update — superseded by GTM-driven ordering.** A subsequent user-journey analysis ([`USER_JOURNEYS.md`](USER_JOURNEYS.md)) re-prioritises the next steps around buyer personas from [`../attestly-market-research.md`](../attestly-market-research.md). The decisive insight: persistent agent identity (gap #3 in [`PROCESS_OVERVIEW.md`](PROCESS_OVERVIEW.md)) is load-bearing for Persona B's pilot stage, not just a deferred item; and the audit viewer is load-bearing for Persona A's pilot. The GTM-driven ordering — viewer → vertical demo → identity → compliance mapping — supersedes the protocol-driven ordering above for the next six months. See `USER_JOURNEYS.md` for the full reasoning.

This is one reading. The alternatives — "commit to Rig now and do the spike", or "start anchoring immediately and accept the storage / coordination compounding cost" — are both defensible.

## Source files

- [`docs/PROCESS_OVERVIEW.md`](PROCESS_OVERVIEW.md) — 30-second walkthrough of the full pipeline
- [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) — what is currently implemented (modules, flows, swim lanes)
- [`docs/USER_JOURNEYS.md`](USER_JOURNEYS.md) — buyer-persona journeys + GTM-driven next-step ordering
- [`docs/DECISIONS.md`](DECISIONS.md) — design decisions log, framework comparison, recommendation
- [`docs/PAIN_POINTS.md`](PAIN_POINTS.md) — running friction log, Phase 4 synthesis at the top
- [`planning/attestly-prototype-plan.md`](../planning/attestly-prototype-plan.md) — original plan plus appended "Phase 1 Postmortem" section
