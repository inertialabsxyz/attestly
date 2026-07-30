# Design Decisions Log

Architectural choices made across the Attestly prototype's three implementation
phases (Weeks 1-6), the Phase 4 evaluation findings, and a recommendation
for the framework decision.

This file is the input to the human's framework choice. The recommendation
section names a Decision Option (A/B/C/D from
`planning/attestly-prototype-plan.md`) but the call is the human's, not Claude's.

---

## Design decisions log

### Phase 1 — Attestations & Signing

#### D1.1. Plain async types in place of `AgentBuilder<_, DirectAgent>::new(...).run()`

`crates/attestly-agents/src/{worker,verifier}.rs` implement
`WorkerAgent`/`VerifierAgent` as plain async traits, not via
AutoAgents' `AgentBuilder`. **Why:** the framework's `run()` requires a
live LLM provider — Phase 1 would have been unrunnable without an API
key, and `make check` would have hit a real LLM in CI. The trait
shape mirrors what an AutoAgents agent looks like (struct + tools +
async `run()`) so a future swap is a localised change. See
`PAIN_POINTS.md` Phase 1 Q1 for the full reasoning.

#### D1.2. Attestation generation as agent post-processing, not inside the tool

`Worker::run()` builds the attestation *after* `calculate()` returns,
not from inside the tool. **Why:** tools are reusable across agents
(the Verifier also calls `calculate`), and `output_hash` covers the
agent's claim, not the tool's intermediate state. One signature per
agent action keeps the attestation graph clean. See `PAIN_POINTS.md`
Phase 1 Q2.

#### D1.3. Ephemeral keypairs with deferred persistence

`AgentKeypair::generate()` is called once per Worker/Verifier
construction, with no on-disk identity. **Why:** the prototype plan
defers "Agent identity registration" to Phase 2 of Attestly overall.
Persistent keys would force a key-management story that the prototype
doesn't need yet. The trade-off is that every restart produces a new
agent identity — fine for a prototype, not for production.

#### D1.4. `AttestationStatus::Failed(String)` as a tuple variant

The plan's example showed `Failed(String)` as a tuple variant; an
earlier draft of `crates/attestly-core/src/attestation.rs` used a struct
variant `Failed { reason: String }` for ergonomics. We aligned to the
plan in commit `0275911`. **Why:** matches the spec's wire shape so
external consumers reading `AttestationStatus::Failed("…")` JSON do
not break.

#### D1.5. JSON file (`data/attestations.json`) for Phase 1 storage

Append-only JSON log, one attestation per line. **Why:** simplest
durable storage that supports the plan's "load and verify signatures
independently" exit criterion. SQLite was the eventual target (Phase 3)
but JSON kept Phase 1 dependency-light.

#### D1.6. Two `verify_attestation` surfaces (string and typed)

`crates/attestly-agents/src/tools.rs` exposes `verify_attestation` (taking
`String` JSON, matching the AutoAgents tool convention) and
`verify_attestation_struct` (taking `&Attestation`, used by the
Dispatcher and tests). **Why:** the LLM-driven path needs the JSON
surface; the in-process path benefits from compile-time safety.
Trade-off: small duplication today, foreshadows a real cost as
typed values multiply. See `PAIN_POINTS.md` Phase 4 synthesis #2.

### Phase 2 — Dispatcher & Orchestration

#### D2.1. Coordination types live in `attestly-core`, not `attestly-agents`

`TaskExecution`, `ExecutionStatus`, and the persistence helpers live
in `crates/attestly-core/src/{task,execution}.rs`. **Why:** any consumer
(Dispatcher, dashboard, Phase 3 Batcher) can serialize and reason
about an execution without depending on `attestly-agents`. Keeps the
framework-coupling boundary at `attestly-agents`. See `PAIN_POINTS.md`
Phase 2 Q1.

#### D2.2. Dispatcher takes `&dyn WorkerAgent` and `&dyn VerifierAgent`

Trait-object hand-off, not a framework-driven actor. **Why:** see D1.1
— the framework integration is deferred. With trait objects the
Dispatcher is testable end-to-end without an LLM, and the swap to
`AgentBuilder.run()` becomes a localised change.

#### D2.3. Per-stage timeout via `tokio::time::timeout`

`DispatcherConfig.{worker,verifier}_timeout` default to 30s each.
Failures inside a stage produce `ExecutionStatus::Failed { stage,
reason }` and persist before returning. **Why:** the plan's exit
criterion ("Worker takes >30s") requires bounded latency; a single
end-to-end timeout would hide which stage stuck. Independent timeouts
also let production callers tune them differently.

#### D2.4. Verifier disagreement is a data signal, not a control event

The Dispatcher logs `dispatcher: verifier disagreement …` to stderr
but transitions to `Complete{attestation_valid, answer_correct}` —
not to `Failed`. **Why:** disagreement is the very signal the system
exists to surface. Halting on disagreement would suppress it instead
of recording it. Downstream consumers (Phase 2 of Attestly overall: chain
anchoring, dispute resolution) need the disagreement preserved as
data.

#### D2.5. Pass `&Attestation` (typed) Dispatcher → Verifier; not JSON

Verifier trait takes `worker_attestation: &Attestation`, not a JSON
string. Verifier still calls `verify_attestation_struct` itself —
"the Verifier's job is to be the sceptic." **Why:** type safety,
zero-copy, and the LLM-tool surface is preserved at the *tool*
boundary, not pushed up the call chain. See `PAIN_POINTS.md` Phase 2
Q3.

#### D2.6. Two-file persistence: id-references in `executions.json` joining `attestations.json`

`TaskExecutionRecord` (in executions.json) holds attestation *ids*;
`Attestation` (in attestations.json) holds the full payloads.
`load_executions` rejoins them. **Why:** keeps both files independently
appendable and avoids duplicating attestation payloads in two places.
A clean stepping stone to the Phase 3 SQLite schema. The pain is
that this introduces the second persistence model — see
`PAIN_POINTS.md` Phase 4 synthesis #4.

### Phase 3 — Attestation Batching

#### D3.1. `rs_merkle` over a hand-rolled tree

`crates/attestly-core/src/merkle.rs` wraps `rs_merkle::MerkleTree` rather
than implementing the tree from scratch. **Why:** the plan listed
`rs_merkle` as the "start with" library. Audited, well-tested, and
the per-leaf inclusion proof API matches our needs. The wrapper
crate's only added value is a typed `Position` enum and an
`attestation_leaf_hash` helper that binds a leaf hash to an
attestation's *signed* canonical bytes.

#### D3.2. Count primary, time backstop, shutdown honours `min_batch_size`

`Batcher` flushes at `max_batch_size` reached (synchronous in
`submit`) or `max_batch_age_secs` elapsed (1-second polling tick).
`min_batch_size` only applies on shutdown. **Why:** count-based
flush gives deterministic batch sizes under load; the time backstop
prevents unbounded latency in a quiet hour; `min_batch_size = 1` by
default never drops anything. See `PAIN_POINTS.md` Phase 3 Q1.

#### D3.3. Store per-attestation inclusion proofs alongside the batch

SQLite has its own `proofs` table holding the proof_path as a JSON
BLOB. **Why:** the exit criterion demands "Proof verification works
given only root + proof + attestation" — regenerating proofs would
require the whole leaf set, leaking the rest of the batch as an
implicit input. Trade-off: ~700 bytes per proof, bounded by leaf
count. See `PAIN_POINTS.md` Phase 3 Q2.

#### D3.4. Sealed batches are immutable; late attestations join the next one

The Batcher does not retroactively splice late attestations into
already-sealed batches. **Why:** every issued inclusion proof is only
meaningful relative to the sealed root; mutating a batch invalidates
every proof we already gave out. Late arrivals lend themselves to a
"supplementary batch" pattern if Phase 2 of Attestly overall needs it; the
prototype's append-only model is simpler. See `PAIN_POINTS.md` Phase
3 Q3.

#### D3.5. SQLite sits *alongside* the JSON logs, not replacing them

Phase 3 added `data/attestly.db` but the Phase 1/2 JSON logs continue to
be written. **Why:** the plan's "Repo Structure" showed both; nothing
in the Phase 3 brief authorised replacing the JSON logs, and the
Phase 1/2 examples (`simple_attestation`, `dispatcher_flow`) still
need to work. The cost is the three-storage-models tax flagged in
`PAIN_POINTS.md` Phase 4 synthesis #4. A future cleanup should pick
one.

### Phase 4 — Evaluation (this phase)

#### D4.1. Stretch task: parallel verifiers (Option B), not MCP (Option A)

We picked Option B because Phase 2's PAIN_POINTS Q2 had explicitly
flagged parallel verifiers as the natural test of whether
AutoAgents' actor model adds value over plain trait objects. Option
A would have exercised our existing tool surface (a known-clean
path) but said little about the multi-agent coordination question
that drives the framework choice. Per the brief: "Pick the one most
likely to surface a framework limitation, not the one most likely to
succeed."

#### D4.2. `ParallelDispatcher` ships next to single-verifier `Dispatcher`, not replacing it

`crates/attestly-agents/src/parallel.rs` defines a separate
`ParallelExecution` result type (with `verifier_attestations: Vec<…>`)
rather than generalising `TaskExecution`. **Why:** the Phase 4 brief
says explicitly "this is an evaluation phase, not a refactor", and
generalising the coordination type would touch the Phase 1/2/3
storage and reader code. The shim cost is `as_task_execution` (which
projects the parallel run to a single-verifier `TaskExecution` for
the executions log) and a duplicated config struct. Cleanly
generalising the coordination type is a Phase 2-of-Attestly task.

#### D4.3. Disagreement reports `Complete{false, false}`, not majority verdict

When verifiers split, `ParallelExecution.status` reports
`attestation_valid=false, answer_correct=false`, with the per-verifier
verdicts preserved in `verifier_attestations`. **Why:** disagreement
is itself a verification failure even if a majority would have said
"valid". Downstream consumers wanting majority logic can compute it
from the per-verifier vec; we did not bake a majority policy into the
core type. The opposite default (silently report majority) would
hide disagreements, undermining the whole multi-verifier point.

#### D4.4. Strict-failure policy on `try_join_all`, not best-effort

Any single verifier error or timeout fails the whole stage. **Why:**
matches the Phase 2 single-verifier Dispatcher (any verifier error =
`Failed`) and is appropriate for high-trust environments where a
verifier failing is itself suspicious. Switching to best-effort
(`futures::future::join_all`, recording per-verifier failures
inline) is a one-line change but a real product decision; we left
it for the human. See `PAIN_POINTS.md` Phase 4 entry.

---

## Framework comparison

The plan's "Evaluation Criteria" table needs concrete observations,
not adjectives. The score per row uses **Lived** for AutoAgents
(directly observed in 6 weeks of implementation) and **Reviewed** for
swarms-rs and Custom-on-Rig (read their public docs/examples per the
Phase 4 brief; *we did not actually port any code*).

Sources reviewed:

- AutoAgents — directly used; see `crates/attestly-agents/src/worker.rs:1-21`
  for the framework-integration scan results
- swarms-rs — <https://github.com/The-Swarm-Corporation/swarms-rs> (README, examples list)
- Rig — <https://github.com/0xPlaygrounds/rig> (README, docs.rig.rs reference)

| Criterion | Weight | AutoAgents (Lived) | swarms-rs (Reviewed) | Custom-on-Rig (Reviewed) |
|-----------|--------|---------------------|------------------------|---------------------------|
| Attestation integration | High | **2/5** — `AgentBuilder.run()` requires an LLM provider; we never managed to inject attestation generation into the framework's output flow without breaking `make check` hermeticity. Worker/Verifier ship as plain async types because of this (PAIN_POINTS Phase 1 Q1). | **3/5** — Agent builder pattern (`agent_builder().build()`) and "Standardized methods for inter-agent communication" are abstract in docs; concrete attestation hooks not documented. Comparable shape to AutoAgents, so the same gap likely applies. | **4/5** — Rig is single-agent-focused with `client.agent(model_id).preamble().build().prompt()`. Attestation generation lives in *our* code wrapping Rig's prompt response — no framework surface to hook into, nothing to fight. The `attestly-core` crate already has the right shape for this. |
| Orchestration flexibility | High | **3/5** — Has a `design_patterns` example folder including `parallel` and `reflection`, which is the closest documented pattern to worker-verifier — but the patterns are LLM-driven by default, not pure-async. The pub/sub `Environment` system is documented but we never used it; for our shape (sequential dispatch, optional fan-out) trait objects were sufficient. | **3/5** — `ConcurrentWorkflow::builder().agents(vec![…]).build()` directly matches our parallel-verifier shape. Sequential workflows mentioned but less detailed. Less mature than AutoAgents but the API surface fits our needs more directly. | **4/5** — No multi-agent primitives; users build their own coordination. For our shapes (sequential Worker→Verifier, parallel N-verifier, time-triggered Batcher) `tokio::join!`, `try_join_all`, and `mpsc::channel` are the obvious tools — exactly what we ended up using on top of AutoAgents anyway. The framework wouldn't fight us. |
| Error handling clarity | Medium | **3/5** — `AgentBuilder.build()` returns `Result`, errors flow through. We didn't drive enough framework code to encounter recovery semantics (the plan deferred "production error handling"). Stage-level error handling in our Dispatcher is tokio `Result`/`Elapsed`, not framework-mediated. | **2/5** — Documentation mentions "memory management, tool integration, and autonomous execution" but error/recovery semantics not visible in the README content reviewed. | **4/5** — Error handling is just Rust + tokio. `Result<T, E>` everywhere, `tokio::time::timeout` for cancellation. Nothing opaque. |
| Documentation quality | Medium | **3/5** — Quickstart works; design-pattern examples exist. Specific multi-agent worker-verifier example does **not** exist (PAIN_POINTS Phase 1 Q1). Forced us to extrapolate from `basic` + `pipeline` examples. | **2/5** — README provides quickstart and architectural diagrams but advanced patterns (tool composition, error recovery, scaling) lack detail. 154 GitHub stars; nascent. | **4/5** — Production-grade docs at docs.rig.rs, 7.2k GitHub stars, used by St Jude / Coral Protocol. README warns of "breaking changes" but the documentation surface is mature. |
| Community / maintenance | Low | **3/5** — Active. v0.3.7 is recent. Liquid Labs ownership. Small but present community. | **2/5** — 154 stars and 21 open issues; active development with "room for stabilization". Smaller community than the others. | **4/5** — 7.2k stars, multiple production users named in README. |
| **Weighted total** | — | **2.7/5** | **2.7/5** | **4.0/5** |

**Weighted total** computed as `(Attestation×3 + Orchestration×3 +
Error×2 + Docs×2 + Community×1) / 11` to translate the row weights
("High/Medium/Low") into multipliers.

### What the table does and doesn't say

It doesn't say swarms-rs is bad — its `ConcurrentWorkflow` is the
closest direct match for our parallel-verifier shape. It just says we
have no first-hand evidence to bet on it; the docs gaps that bite
(error recovery, attestation hooks) would only show up at the same
point we hit them in AutoAgents — week 1 or 2 of porting.

It does say Rig wins the *evidence* test: every concrete win column
(attestation hooks, error handling, docs maturity, community) goes
to it, with the cost being "no multi-agent primitives." Phase 4's
parallel-verifier implementation showed that for *our* coordination
shape, multi-agent primitives are not load-bearing — `try_join_all`
plus `mpsc::channel` carried us through Phase 3 too.

---

## Recommendation for the human

**Recommended Decision: Option C — Thin custom layer on Rig.**

### Why

1. **Phase 4's stretch task showed our coordination is simpler than a
   framework.** Six weeks across three phases produced one Worker→Verifier
   sequence, one Worker→N-Verifiers fan-out, and one time-or-count
   Batcher. Every coordination shape we shipped was 10-50 lines of
   `tokio` + `futures` glue (`Dispatcher::run`, `ParallelDispatcher::run`,
   `Batcher::run_batcher`). None of them benefited from an actor model,
   pub/sub bus, or supervisor — and Phase 1's framework-integration scan
   confirmed no AutoAgents primitive was driving any of them anyway.

2. **The friction we hit was framework-shaped, not coordination-shaped.**
   The single biggest pain point (PAIN_POINTS synthesis #1) was that
   `AgentBuilder.run()` needs a live LLM provider, breaking
   `make check` hermeticity. We worked around this by *not using the
   framework*. The dual `verify_attestation` / `verify_attestation_struct`
   surfaces (synthesis #2) exist because the framework's tool convention
   is JSON strings, while our typed in-process path wants `&Attestation`.
   Both pains go away if we own the agent loop and just call Rig for
   completions.

3. **Rig has the strongest evidence base.** 7.2k stars, named
   production users, mature docs at docs.rig.rs, 20+ provider clients
   under one trait. The "breaking changes" warning is real but no
   worse than AutoAgents' v0.3.x velocity.

4. **`attestly-core` is already framework-agnostic.** The plan's
   risk-mitigation row "Minimal framework coupling in attestly-core" was
   followed — `attestly-core` knows nothing about agents, tools, or LLMs.
   A swap to Rig touches `attestly-agents` only.

### What "thin custom layer on Rig" looks like

```
crates/attestly-agents/
  src/
    rig_client.rs       # one place that talks to Rig (single LLM provider hop)
    worker.rs           # WorkerAgent trait impl using rig_client
    verifier.rs         # VerifierAgent trait impl using rig_client
    dispatcher.rs       # unchanged (already trait-object based)
    parallel.rs         # unchanged
    batcher.rs          # unchanged
    tools.rs            # verify_attestation_struct stays; the JSON tool
                        # surface is now optional (only needed if we ever
                        # expose tools over MCP)
```

The plan estimated "1-2 weeks" for a custom layer; based on six
weeks' worth of evidence that our coordination needs are minimal,
that estimate is plausible. The bulk of the work is one `rig_client`
module that the existing trait impls call into.

### The strongest counter-argument

**Sunk cost is real, and AutoAgents lock-in is *currently zero*.**
The risk-mitigation row in the plan ("Minimal framework coupling in
attestly-core") was honored: swapping frameworks is a contained change.
But the *opposite* is also true — staying with AutoAgents is a
contained decision. The framework currently has zero lines of code
in our implementation, so there is no integration debt to escape.
*Continuing to defer the framework integration* (Option A in
practice, by leaving the trait-object Worker/Verifier in place) is
indistinguishable from Option C until the day we want LLM-driven
agents — at which point Rig and AutoAgents have roughly the same
cost-of-integration.

So: **if** the next milestone (Phase 2 of Attestly overall) is on-chain
anchoring or identity registration — work that doesn't touch the
agent loop — Option A (status quo) and Option C (Rig) are
indistinguishable, and the human can defer this decision again. **If**
the next milestone is wiring real LLMs into Worker/Verifier, that's
the moment to commit to Rig per Option C.

### Confidence and uncertainty

- **High confidence:** AutoAgents' multi-agent story is not load-bearing
  for our needs. We have direct evidence (Phase 4 stretch task) that
  trait objects + tokio handle every coordination shape we've shipped.
- **High confidence:** swarms-rs is too immature to bet on right now.
- **Medium confidence:** Rig is the right answer. Based on docs review
  only — we did not port any code. The framework's "Here be dragons"
  warning is a real risk; a 4-hour spike to wire Rig into one Worker
  would convert the medium confidence into high.
- **Low confidence in scope:** the human may decide that the next
  milestone (chain anchoring, identity, or HTTP API) outweighs
  framework changes for now — in which case Option A wins by inertia
  and the same data still applies six months from now.

The decision is the human's. This file lays out the evidence; the
weighing of "switch now" vs. "defer until LLM integration is needed"
is a product call, not a technical one.

---

## Phase log

The plan's Weekly Log Template was not used per phase — each phase's
PR (`#2`/`#3`/`#4`) and the corresponding `PAIN_POINTS.md` section
served as the record. The template is preserved in
`planning/attestly-prototype-plan.md` for any future extension of the
prototype that wants weekly granularity.
