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
`AttestationStatus::Failed { reason }` is emitted and *that* is signed — the
agent always produces a record of what happened, never silently swallows the
error.

**Open edge case:** if `Worker::run()` panics between executing the tool and
signing the attestation, no record is produced. Production-quality handling
would catch panics and emit `Failed { reason: "agent panicked" }`, but
the prototype plan's "Production error handling (prototype quality
acceptable)" deferral covers this. Flag it for Phase 4 if it bites.

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
Worker emitted `Failed { reason }` and the Verifier's independent solve
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

## Recurring friction (cross-phase)

*To be populated as Phase 2/3/4 surface patterns.*
