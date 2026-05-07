# Pain Points

A running log of friction encountered while building the AWP prototype. Each
phase appends its "Key Questions to Answer" with proposed answers — not
unilateral decisions. The human chooses which answer ships; this file is
input to that choice and to Phase 4's framework comparison.

---

## Phase 1 — Attestations & Signing

### Q1. How do you cleanly inject attestation generation into AutoAgents output flow?

**Proposed answer (deferred to human):** *Don't, yet.* AutoAgents v0.3.7 has
no documented worker-verifier pattern, and `AgentBuilder<_, DirectAgent>::new(...).run()`
requires a live LLM provider, which would either (a) make the Phase 1 example
unrunnable without an API key, or (b) make `make check` non-hermetic by hitting
a real LLM in CI. Phase 1 ships Worker and Verifier as plain async types
(`crates/awp-agents/src/{worker,verifier}.rs`) whose surface mirrors what an
AutoAgents agent looks like — struct, tools, async `run()` — so the swap to
`AgentBuilder.run()` in Phase 2+ is a localised change.

The cleanest *eventual* injection point appears to be the agent's `From<ReActAgentOutput>`
conversion: when `output.done` is true, build the `Attestation` from the
final reasoning trace plus the tool-call results captured by the runtime.
That keeps attestation generation out of the LLM prompt path while still
binding the signed record to the agent's actual final output.

**Recommended decision for the human:** keep the current adapter shape
through Phase 2 (Dispatcher) and revisit at Phase 4 with concrete data on
how AutoAgents represents tool-call traces. If the framework doesn't expose
the call trace cleanly, that's a strong signal for Decision Option C
(thin custom layer on Rig).

---

### Q2. Should attestation be created inside the agent tool, or as post-processing?

**Proposed answer (deferred to human):** *Post-processing.* Phase 1 builds the
attestation in the Worker's `run()` after the tool returns
(`crates/awp-agents/src/worker.rs:107-131`), not inside `calculate()`. Three
reasons:

1. **Tools should be reusable across agents** — `calculate` is also called by
   the Verifier for its independent re-solve; threading attestation logic into
   the tool would couple the tool to Worker semantics.
2. **The signing payload covers the agent's claim, not the tool's intermediate
   state** — `output_hash` covers the *formatted final output string*, which
   is a Worker-level concern, not a calculator-level one.
3. **One signature per agent action keeps the attestation graph clean** — if
   tools self-attested, a single Worker run with three tool calls would
   produce four attestations and the Verifier would have to reason about
   which one is "the" answer.

**Recommended decision for the human:** ratify post-processing. Revisit only
if Phase 4's stretch task (parallel Verifiers / MCP) surfaces a case where
tool-internal attestation would simplify the wire protocol.

---

### Q3. How to handle partial failures (agent runs but attestation signing fails)?

**Proposed answer (deferred to human):** *Signing must not fail in the happy
path.* `AgentKeypair::sign_attestation()` is infallible by construction —
ed25519 signing is deterministic and the keypair was generated successfully
at agent startup. The only realistic failure mode is *persistence* failing
after the attestation is built and signed (e.g. disk full when writing
`attestations.json`).

Phase 1 keeps the attestation in memory — `Worker::run()` returns the signed
`Attestation` struct, and the example/test caller is responsible for
persisting it. If `append_attestation` fails, the caller sees the error and
can retry; the in-memory attestation is unaffected. There is no half-state
where signing partially succeeded.

For tool-level partial failures (the calculator rejects bad input),
`AttestationStatus::Failed(reason)` is emitted and *that* is signed — the
agent always produces a record of what happened, never silently swallows the
error.

**Open edge case:** if `Worker::run()` panics between executing the tool and
signing the attestation, no record is produced. Production-quality handling
would catch panics and emit `Failed("agent panicked")`, but the prototype
plan's "Production error handling (prototype quality acceptable)" deferral
covers this. Flag it for Phase 4 if it bites.

**Recommended decision for the human:** keep the current "in-memory-first,
persist-second" split. Revisit when Phase 2's Dispatcher introduces
network/RPC edges, where transient failures are real.

---

### Q4. How should the Verifier behave if attestation is cryptographically invalid but the answer is correct?

**Proposed answer (deferred to human):** *Report both bits independently.*
This is the central design feature of `AttestationStatus::Verified`:

```rust
Verified {
    attestation_valid: bool,  // crypto check
    answer_correct: bool,     // semantic check
}
```

Phase 1's Verifier (`crates/awp-agents/src/verifier.rs:73-123`) computes
each bit independently — `attestation_valid` from `verify_attestation`,
`answer_correct` from comparing its own `calculate` re-solve to the
worker's claimed output. A valid signature on a wrong answer produces
`Verified { attestation_valid: true, answer_correct: false }`; an invalid
signature on a correct answer produces `Verified { attestation_valid: false,
answer_correct: true }` if the Verifier's independent solve happens to match.

What the Dispatcher (Phase 2) does with this is a separate question — it
might log/halt on `attestation_valid: false`, since that's a stronger signal
of byzantine behaviour than `answer_correct: false`. The Phase 2 prompt's
"Verifier disagreement" task addresses the second case but not the first;
worth reading carefully there.

**Edge case found during implementation** (verifier.rs:91-100): if the
Worker emitted `Failed(reason)` and the Verifier's independent solve
also fails, we currently report `answer_correct: false` because there is no
correct answer to compare against. An alternative is `answer_correct: true`
(meaning "we agree the task is unsolvable"). The current behaviour is
conservative — a downstream consumer can still tell unsolvable from
miscalculated by reading `worker.status` separately.

**Recommended decision for the human:** ratify "report both bits
independently" as the contract. The Failed/Failed edge case can be deferred
to Phase 2 when the Dispatcher decides what action to take on each
combination.

---

## Phase 2 — Dispatcher & Orchestration

### Q1. Where does coordination state live — in Dispatcher, or external store?

**Proposed answer (deferred to human):** *Both, but the canonical encoding
lives in `awp-core`.* `TaskExecution` and `ExecutionStatus` are defined in
`crates/awp-core/src/task.rs` (not in `awp-agents`) so any consumer — the
Dispatcher today, a dashboard tomorrow, the Phase 3 Batcher — can serialize
and reason about an execution without depending on the agents crate. The
running Dispatcher holds the in-memory `TaskExecution` for the duration of
one task; the durable copy lives in `data/executions.json` as an
id-referenced record (`TaskExecutionRecord`) that re-joins with
`data/attestations.json` on load via `load_execution(id)`
(`crates/awp-core/src/execution.rs`).

The split — full attestations in one file, executions referencing them by
id in another — keeps both files independently appendable and avoids
duplicating the (much larger) attestation payloads in two places. Phase 3
collapses this into SQLite, but the Phase 2 layout is a clean stepping
stone: the SQLite schema in the plan already mirrors the two-table
attestations + executions split.

**Recommended decision for the human:** ratify "data structures in
`awp-core`, file IO in `awp-core::execution`, lifecycle ownership in
Dispatcher". The Dispatcher does not own the type definition because that
would force the Batcher (Phase 3) and any future external consumer to
take an `awp-agents` dependency just to deserialise an execution.

---

### Q2. How does AutoAgents handle agent-to-agent communication?

**Proposed answer (deferred to human):** *Unknown — and Phase 2 sidesteps
the question.* As noted in Phase 1 Q1, AutoAgents v0.3.7 ships single-agent
examples (`AgentBuilder<_, DirectAgent>::new(...).run()`) but no documented
worker-verifier or multi-agent coordination pattern. Phase 1 already
deferred the framework integration; Phase 2 inherits that deferral and
ships the Dispatcher as a plain async type that takes `&dyn WorkerAgent`
and `&dyn VerifierAgent` trait objects.

In practice, the Dispatcher's "agent communication" reduces to two trait
calls:

```rust
let worker_att = worker.run(&task).await?;
let verifier_att = verifier.run(&task, &worker_att).await?;
```

This is precisely what an AutoAgents-native Dispatcher would also do —
*await one agent, pass its output as context to the next* — so the swap
to a framework-driven actor model later should be a surface change to the
Dispatcher, not a re-design. The interesting question Phase 4 needs to
answer is whether AutoAgents' actor model adds value over plain trait
objects for *this* coordination shape, or whether the actor abstraction
is gratuitous when the only message pattern is sequential request /
response with a typed return.

**Recommended decision for the human:** keep the trait-object Dispatcher
through Phase 4 as the framework-comparison baseline. If AutoAgents'
actor channels add real value (e.g. for the parallel-verifiers stretch
task), that's a strong signal for Decision Option A; if not, it's a
signal for C (custom on Rig).

---

### Q3. What's the cleanest way to pass attestation context to the Verifier?

**Proposed answer (deferred to human):** *Pass the typed `Attestation`
struct directly, not its JSON.* The `VerifierAgent` trait takes
`worker_attestation: &Attestation` (`crates/awp-agents/src/verifier.rs`)
so the Dispatcher hands over the Worker's signed record by reference. The
Verifier still calls `verify_attestation_struct` itself — *the Dispatcher
does not pre-verify and pass a "trust me" boolean* — because the Verifier's
job is to be the sceptic, not delegate scepticism upstream. This matches
the plan's "the Verifier should still call `verify_attestation` itself
rather than trusting the Dispatcher's hand-off".

We considered three alternatives:

1. **Pass the JSON string** — matches the AutoAgents tool surface
   (`verify_attestation(attestation_json: String)`) but loses type safety
   at every internal hand-off and forces a parse round-trip the
   Dispatcher already paid for once.
2. **Pass only the attestation id, have the Verifier reload from disk** —
   tempting because it would let Verifiers pick up work asynchronously,
   but adds a disk read and a "what if the file isn't fsynced" failure
   mode for no Phase-2 benefit.
3. **Pass `&Attestation` directly (chosen)** — type-checked, zero-copy,
   keeps the Worker→Verifier contract obvious.

When the framework-driven path (LLM tool calls) lands, the
`verify_attestation` tool's JSON-string signature stays as the *tool*
surface, while the in-process Dispatcher → Verifier hand-off remains
typed. They serve different audiences (LLM vs. compiler).

**Recommended decision for the human:** ratify the typed hand-off as the
in-process contract. Revisit only if Phase 4's stretch task (parallel
Verifiers / MCP) makes a string-based wire protocol unavoidable.

---

## Phase 3 — Attestation Batching

### Q1. What's the right batch trigger balance — time vs count?

**Proposed answer (deferred to human):** *Both, with count as the primary
trigger and time as a liveness backstop.* The Batcher
(`crates/awp-agents/src/batcher.rs`) checks the count condition
synchronously inside `submit` so a busy producer hits a deterministic
batch size of `max_batch_size`, while a separate `tokio::time::interval`
polls the age of the oldest buffered attestation once per second to fire
the time-based trigger when traffic is sparse. Defaults match the plan
(`max_batch_size: 10, max_batch_age_secs: 60, min_batch_size: 1`), so a
high-volume run produces tightly-packed batches of 10 and a low-volume
run still flushes within ~60 seconds even if a single attestation arrives
in isolation.

The cost of the time backstop is one wakeup per second per Batcher; the
cost of *not* having it is unbounded latency for the last attestation
in a quiet hour. We accept the wakeup. `min_batch_size` exists for
shutdown-only behaviour (Phase 3 prompt: "On shutdown, flushes any
buffered attestations as a final batch (respecting `min_batch_size`)") —
with the default of 1, shutdown always flushes; a non-default
`min_batch_size > 1` lets a caller drop a too-small final buffer rather
than seal it.

**Recommended decision for the human:** ratify "count primary, time
backstop, shutdown honours `min_batch_size`". Phase 4 might revisit if
on-chain anchoring (Phase 2 of AWP overall) introduces gas-cost
incentives to delay flushes — at that point count alone is too rigid and
the trigger should also consider chain congestion.

---

### Q2. Should proof be stored, or regenerated on demand?

**Proposed answer (deferred to human):** *Store the proof.* Phase 3's
SQLite schema gives `proofs` its own table, with one row per attestation
holding the `proof_path` as a JSON BLOB. Three reasons:

1. **The exit-criterion demands it.** The plan requires "Proof
   verification works given only root + proof + attestation" — if the
   proof has to be re-derived from the full leaf set, that contract is
   leaking the rest of the batch as an implicit input. Storing the proof
   makes "given only root + proof + attestation" literally true.
2. **Regeneration costs the full leaf set.** rs_merkle needs every leaf
   to walk the proof path; we'd be loading the entire batch from disk on
   every verification. A 10-leaf batch is cheap, but a 1000-leaf batch
   (a plausible Phase 2-of-AWP scale) is not.
3. **Proofs are write-once.** Each batch is sealed in one transaction and
   never mutated, so storage cost grows linearly with attestation count
   and there's no consistency-management overhead. A proof_path of 10
   sibling hashes plus positions encodes to ~700 bytes of JSON — well
   under the size of the attestation it witnesses.

**Edge case considered:** if the batch root is somehow corrupted, a
stored proof points at a now-invalid root and verification fails
silently. We accept this — corruption of the `batches.merkle_root`
column is observable from outside the proof system (the batch's
attestations, when re-hashed, won't match the stored root) so a future
auditor still has a way to detect the tampering.

**Recommended decision for the human:** ratify "store the proof, never
regenerate". Phase 4 might revisit only if storage growth becomes a
concern — and even then, the right answer is probably "tier old batches
to cheaper storage", not "regenerate on demand".

---

### Q3. How to handle late attestations (batch already created)?

**Proposed answer (deferred to human):** *Late attestations join the next
batch — there is no "this attestation belonged in batch X" recovery
path.* The Batcher's `submit` always pushes onto the live buffer; it
doesn't reach into already-sealed batches to retroactively splice
anything in. This is a deliberate consequence of the plan's exit
criterion ("All data persisted to SQLite", "Inclusion proof generated
for each attestation"): a sealed batch's Merkle root is final, and the
proofs covering it are only meaningful relative to that root. Mutating
the batch would invalidate every proof we already issued.

In practice, two scenarios drive the question:

1. **Slow Worker / Verifier produces an attestation after its
   Dispatcher run already returned.** Phase 3 sidesteps this — the
   Dispatcher only submits to the Batcher *after* both attestations are
   collected (`crates/awp-agents/src/dispatcher.rs`), and a stage
   timeout produces a `Failed` execution that never reaches the
   batcher-submit branch. The Worker/Verifier outputs that arrive
   "late" never get submitted at all.
2. **Producer and Batcher are decoupled (e.g. across an HTTP boundary, a
   future Phase 2-of-AWP feature).** Here a late attestation can arrive
   after the batch deadline. The Batcher still accepts it; it lands in
   the next batch. Anyone correlating attestations to wall-clock time
   needs to read `attestations.timestamp` (signed by the producer) — the
   `batches.created_at` field is the *Batcher's* wall clock, not the
   producer's, and they may differ.

The trade-off we are explicitly *not* making is the "open batch"
strategy where a batch stays mutable until anchored on chain. That would
require deferring Merkle root computation, breaking the exit criterion
(no proofs until the batch is closed), and complicating the schema. The
prototype's append-only-once model is simpler and matches the durable
attestation log Phase 1 already established.

**Recommended decision for the human:** ratify "late attestations join
the next batch; sealed batches are immutable". Revisit only if Phase 2
of AWP overall introduces a producer-side retry path that needs
"this should have been in batch X" semantics — at which point a
separate "supplementary batches reference earlier attestations" table
is probably cleaner than mutable batches.

---

## Recurring friction (cross-phase)

*To be populated as Phase 4 surfaces patterns.*
