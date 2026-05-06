# Agent Prompts — AWP Prototype

These prompts dispatch Claude Code agents to implement the AWP prototype defined in [`awp-prototype-plan.md`](awp-prototype-plan.md). Each prompt is self-contained — agents work in isolated git worktrees and do not share state during execution.

The four phases are strictly sequential: each phase's exit criteria gate the next. There is no parallelism in this plan, so there is no sequencing diagram — just dispatch the next prompt only after the previous PR is merged to `main` and its exit criteria are met.

The conventions referenced below live in [`/.claude/`](../.claude/):

- [`commits.md`](../.claude/commits.md) — `make check` before every commit; `type(scope): description`
- [`testing.md`](../.claude/testing.md) — `make check` is the gate; tests required for features and regressions
- [`review-gate.md`](../.claude/review-gate.md) — review agent before PR
- [`pull-requests.md`](../.claude/pull-requests.md) — draft PR + Agent Run Report comment
- [`agent-prompts.md`](../.claude/agent-prompts.md) — the template these prompts follow

## Scope notes for the human

A few sections of `awp-prototype-plan.md` are **not** for the dispatched agent:

- **Weekly log entries** in `docs/DECISIONS.md` — write these yourself; the agent does not know how many hours you spent.
- **Phase 4 (Weeks 7-8) framework decision** — the agent can do the comparison legwork and stretch tasks, but the actual go/no-go on AutoAgents vs. swarms-rs vs. custom-on-Rig is yours.
- **"Key Questions to Answer"** in each phase — the agent should propose answers in a `docs/PAIN_POINTS.md` entry, but you decide which answer ships.

The verification commands in each prompt assume `make check` exists. Phase 1's first task is to create it; subsequent phases inherit it.

---

## Phase 1 — Weeks 1-2: Attestations & Signing

**Branch:** `phase/1-attestations-signing`

**Prompt:**

You are implementing Phase 1 of the AWP prototype. The full specification is in `planning/awp-prototype-plan.md` under "Weeks 1-2: Attestations & Signing". Read that section carefully, plus the "Repo Structure" section near the bottom of the same file, before writing any code.

### Context

The repository currently contains only planning documents and the agent-workflow conventions in `.claude/`. There is no Rust code yet, no Cargo workspace, no `Makefile`, and no `make check` gate. You are the first implementation agent — you set the foundations the next three phases will build on.

The prototype is a Rust workspace using [AutoAgents](https://github.com/liquidos-ai/AutoAgents) for multi-agent orchestration. The starting point described in the plan is "Working Worker-Verifier prototype" — interpret this as: get a minimal Worker and Verifier running using AutoAgents, then layer signed attestations on top. If AutoAgents has an example worker-verifier pattern in its docs, base your scaffolding on that and cite the source in a code comment.

### Your Task

1. **Create the Cargo workspace** matching the layout in `planning/awp-prototype-plan.md` → "Repo Structure":
   - Workspace root `Cargo.toml` declaring `crates/awp-core` and `crates/awp-agents` as members
   - `crates/awp-core/` with `attestation.rs`, `signing.rs`, plus empty `merkle.rs` and `storage.rs` files containing only a `// Phase N stub` comment (these will be fleshed out in Phase 3)
   - `crates/awp-agents/` with `tools.rs`, `worker.rs`, `verifier.rs`, plus empty `dispatcher.rs` and `batcher.rs` stubs
   - `examples/simple_attestation.rs` as the Phase 1 checkpoint binary
   - `data/.gitkeep`
   - `.gitignore` covering `target/`, `data/*` (but not `.gitkeep`), `attestations.json`

2. **Create the `Makefile`** — the testing/commits gate depends on it. Targets must match `.claude/testing.md`:
   ```
   make check       # cargo fmt --check && cargo clippy -- -D warnings && cargo test
   make lint        # cargo fmt --check && cargo clippy -- -D warnings
   make test        # cargo test --workspace
   make test-unit   # cargo test --workspace --lib
   make test-int    # cargo test --workspace --tests
   make fix         # cargo fmt && cargo clippy --fix --allow-dirty
   ```

3. **Implement the `Attestation` struct in `awp-core`** exactly as specified in the plan's "Data Structures" subsection (struct fields and `AttestationStatus` variants verbatim). Add `serde::Serialize`/`Deserialize` derives. Implement a `signing_payload(&self) -> Vec<u8>` method that returns the canonical bytes signed by the agent (everything except the `signature` field). Pre-place the field shapes the next phases will need but keep them minimal for now — do **not** add `trace_hash`, `trace_location`, `zk_proof`, or `verification_mode` from Appendix A; the plan's Recommendation defers that.

4. **Implement `signing.rs`** using `ed25519-dalek`:
   - `AgentKeypair` wrapping `SigningKey` + `VerifyingKey`
   - `AgentKeypair::generate()` for ephemeral startup keys (persistence is deferred)
   - `sign_attestation(&self, attestation: &mut Attestation)` populating `agent_pubkey` and `signature`
   - `verify_attestation_signature(&Attestation) -> bool` for round-trip verification
   - SHA-256 helpers for `task_hash` and `output_hash` (use the `sha2` crate)

5. **Storage in `awp-core`** — implement append-only JSON storage in `attestation.rs` (or a small `storage.rs` if you prefer; it's marked stub in the layout but a minimal implementation here is fine):
   - `append_attestation(path: &Path, a: &Attestation) -> io::Result<()>`
   - `load_attestations(path: &Path) -> io::Result<Vec<Attestation>>`
   - On load, verify each attestation's signature and skip + log invalid ones

6. **Implement the `verify_attestation` tool** in `crates/awp-agents/src/tools.rs` exactly per the plan's "Verifier Tool: `verify_attestation`" section. Return the `VerificationResult` struct with the four fields specified plus `overall_valid`.

7. **Wire Worker and Verifier agents** in `worker.rs` and `verifier.rs` using AutoAgents. Worker generates a keypair on startup, executes a task (the example task in the plan is arithmetic via a `calculate` tool — implement that too), and emits a signed `Attestation`. Verifier calls `verify_attestation` first, then independently calls `calculate`, then reasons about both — producing its own attestation with `status: Verified { attestation_valid, answer_correct }` and `references: Some(worker_attestation.id)`.

8. **`examples/simple_attestation.rs`** runs the full Worker → Verifier flow end-to-end on a hardcoded task, writes both attestations to `data/attestations.json`, then re-loads and re-verifies them. Print a summary to stdout with both attestation IDs and statuses.

9. **Tests** (per `.claude/testing.md`):
   - Unit tests in `awp-core/src/attestation.rs` and `signing.rs` for hashing, signing, signature round-trip, and tampered-attestation rejection
   - An integration test in `crates/awp-agents/tests/` that runs the Worker → Verifier flow and asserts both attestations are produced, linked, and cryptographically valid

10. **Address the "Key Questions to Answer"** in `docs/PAIN_POINTS.md` — propose your answer to each of the four questions with one paragraph each. Do not make architectural decisions unilaterally; flag where the human should weigh in.

### Do Not Touch

- `crates/awp-core/src/merkle.rs` — Phase 3's domain, leave the stub comment
- `crates/awp-agents/src/dispatcher.rs` — Phase 2's domain, leave the stub comment
- `crates/awp-agents/src/batcher.rs` — Phase 3's domain, leave the stub comment
- The Appendix A LuaI integration — the plan's Recommendation explicitly defers it; do not add `trace_hash` / `zk_proof` fields to `Attestation`

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/awp-prototype-plan.md` → "Weeks 1-2: Attestations & Signing", with extra emphasis on the "Exit Criteria" checklist.
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(core): phase 1 attestations and signing`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass

cargo run --example simple_attestation
# → prints worker attestation id + status: Completed
# → prints verifier attestation id + status: Verified { attestation_valid: true, answer_correct: true }
# → data/attestations.json contains exactly 2 records

cargo run --example simple_attestation
# → appends 2 more attestations, all 4 verify on reload
```

---

## Phase 2 — Weeks 3-4: Dispatcher & Orchestration

**Branch:** `phase/2-dispatcher-orchestration`
**Depends on:** Phase 1 merged to `main`

**Prompt:**

You are implementing Phase 2 of the AWP prototype. The full specification is in `planning/awp-prototype-plan.md` under "Weeks 3-4: Dispatcher & Orchestration". Read that section carefully before writing any code.

### Context

Phase 1 is complete and merged. The repository now contains:

- A Cargo workspace with `crates/awp-core` (Attestation, signing, JSON storage) and `crates/awp-agents` (Worker, Verifier, `verify_attestation` tool, `calculate` tool)
- `examples/simple_attestation.rs` runs Worker → Verifier directly with no coordinator
- `data/attestations.json` is the append-only attestation log
- `make check` runs fmt + clippy (`-D warnings`) + workspace tests
- `crates/awp-agents/src/dispatcher.rs` exists but contains only a `// Phase 2 stub` comment

Worker and Verifier are AutoAgents agents that emit signed `Attestation`s. Your job is to add a Dispatcher that owns the task lifecycle, routes work to the Worker, then to the Verifier (passing the Worker's attestation as context), and aggregates both attestations into a `TaskExecution` record.

### Your Task

1. **Implement `TaskExecution` and `ExecutionStatus`** in `crates/awp-core/src/lib.rs` (or a new `task.rs` module) exactly per the plan's "Coordination State" subsection. Include `serde` derives. Decide whether `TaskExecution` lives in `awp-core` (data structure) or `awp-agents` (coordination concern); the plan says "where does coordination state live" is a key question — answer it in `docs/PAIN_POINTS.md` and pick one. Bias toward `awp-core` so the type can be serialised by anyone.

2. **Implement `Dispatcher`** in `crates/awp-agents/src/dispatcher.rs`:
   - Receives a task input
   - Creates a `TaskExecution` with status `Pending` → `WorkerRunning`
   - Routes to the Worker, awaits its attestation
   - Transitions to `WorkerComplete`, then `VerifierRunning`
   - Passes the Worker's signed attestation to the Verifier as task context — the Verifier should still call `verify_attestation` itself rather than trusting the Dispatcher's hand-off
   - Awaits the Verifier's attestation, transitions to `Complete { attestation_valid, answer_correct }`
   - On any agent error or timeout, transitions to `Failed { stage, reason }`

3. **Timeout handling** — apply a configurable timeout per stage (default 30 seconds, per the plan's exit criteria). If Worker or Verifier exceeds it, the Dispatcher records `Failed { stage, reason: "timeout" }` and returns. Use `tokio::time::timeout`. Add `tokio` to workspace dependencies if Phase 1 didn't already (AutoAgents likely pulls it in transitively — verify and prefer the workspace's existing version).

4. **Verifier disagreement** — if the Verifier returns `Verified { answer_correct: false, .. }` or `Verified { attestation_valid: false, .. }`, the Dispatcher logs a structured warning and stores the `TaskExecution` with `Complete { ... }` reflecting the disagreement. It does **not** halt or retry. Add a clear log message that an operator could grep for.

5. **Persistence** — extend `awp-core`'s storage to include `TaskExecution` records. Store them in `data/executions.json` (append-only JSON, same pattern as `attestations.json`). Each `TaskExecution` references its attestations by UUID — the attestations themselves remain in `attestations.json`. Provide `load_execution(id)` that joins both files into a fully-populated `TaskExecution`.

6. **`examples/dispatcher_flow.rs`** — the Phase 2 checkpoint. Submits 3 tasks via the Dispatcher: one normal, one where you induce a Worker timeout (e.g. by injecting a sleep into the task), one where the Verifier disagrees (e.g. by feeding the Worker a deliberately wrong tool result). Print each `TaskExecution`'s final status. Confirm both files (`attestations.json`, `executions.json`) are written correctly.

7. **Integration test** in `crates/awp-agents/tests/` that exercises a successful task end-to-end through the Dispatcher and asserts the `TaskExecution` reaches `Complete` with both attestations linked.

8. **Update `docs/PAIN_POINTS.md`** with answers to the three "Key Questions to Answer" in the plan's Phase 2 section.

### Do Not Touch

- `crates/awp-core/src/merkle.rs` — Phase 3's domain, leave the stub comment
- `crates/awp-agents/src/batcher.rs` — Phase 3's domain, leave the stub comment
- The `Attestation` struct itself — Phase 1's contract, no field changes; Phase 4 may revisit
- `examples/simple_attestation.rs` — Phase 1's checkpoint, must continue to work; do not modify it to use the Dispatcher

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/awp-prototype-plan.md` → "Weeks 3-4: Dispatcher & Orchestration", with emphasis on the "Exit Criteria".
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(agents): phase 2 dispatcher and orchestration`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass (including the new dispatcher integration test)

cargo run --example simple_attestation
# → still works exactly as before (Phase 1 contract preserved)

cargo run --example dispatcher_flow
# → 3 TaskExecutions printed:
#   - normal task: Complete { attestation_valid: true, answer_correct: true }
#   - timeout task: Failed { stage: "WorkerRunning", reason: "timeout" }
#   - disagreement task: Complete { attestation_valid: true, answer_correct: false } with warning logged
# → data/executions.json contains 3 records
# → data/attestations.json appended with new attestations
```

---

## Phase 3 — Weeks 5-6: Attestation Batching

**Branch:** `phase/3-batching-merkle`
**Depends on:** Phase 2 merged to `main`

**Prompt:**

You are implementing Phase 3 of the AWP prototype. The full specification is in `planning/awp-prototype-plan.md` under "Weeks 5-6: Attestation Batching". Read that section carefully before writing any code.

### Context

Phases 1 and 2 are complete and merged. The repository now contains:

- `crates/awp-core` with Attestation, signing, JSON storage, `TaskExecution`
- `crates/awp-agents` with Worker, Verifier, Dispatcher, and shared tools
- Two examples: `simple_attestation.rs` and `dispatcher_flow.rs`
- `data/attestations.json` and `data/executions.json` as append-only JSON logs
- `make check` is the gate
- `crates/awp-core/src/merkle.rs` and `crates/awp-core/src/storage.rs` exist but contain only `// Phase N stub` comments
- `crates/awp-agents/src/batcher.rs` exists but contains only a `// Phase 3 stub` comment

Your job is to add a Batcher service that consumes attestations as the Dispatcher produces them, builds Merkle trees over attestation hashes, generates inclusion proofs, and persists everything to SQLite.

### Your Task

1. **Add dependencies** to the workspace `Cargo.toml`: `rs_merkle` for Merkle trees, `rusqlite` with the `bundled` feature for SQLite. Pin versions in the workspace root and have crates inherit via `workspace = true`.

2. **Implement the Merkle layer** in `crates/awp-core/src/merkle.rs`:
   - `attestation_leaf_hash(&Attestation) -> [u8; 32]` — SHA-256 over the attestation's `signing_payload()` plus its signature, so the leaf hash binds the full signed record
   - `build_tree(leaves: &[[u8; 32]]) -> MerkleTree` — wrap `rs_merkle`
   - `inclusion_proof(tree, index) -> InclusionProof` — produce the struct shape from the plan's "Data Structures" subsection
   - `verify_inclusion(root, attestation_hash, proof) -> bool` — must succeed given only those three inputs (this is the hard exit-criterion)

3. **Implement `Batch`, `InclusionProof`, `ProofNode`, `Position`** structs/enums in `merkle.rs` exactly per the plan's "Data Structures" subsection. `serde` derives required.

4. **Implement SQLite persistence** in `crates/awp-core/src/storage.rs`:
   - Schema: tables `batches` (id, merkle_root, attestation_count, created_at, anchor_tx, anchor_chain), `attestations` (id, agent_id, agent_pubkey, task_hash, output_hash, output, status, references, timestamp, signature, batch_id NULL until batched), `proofs` (attestation_id PK, batch_id FK, proof_path BLOB)
   - Database file at `data/awp.db`, created on first use with `CREATE TABLE IF NOT EXISTS`
   - Functions: `insert_attestation`, `insert_batch_with_proofs`, `get_attestation`, `get_batch`, `get_proof`, `verify_attestation_inclusion(attestation_id) -> bool` (loads batch root + proof and re-verifies cryptographically)
   - All writes wrapped in transactions

5. **Implement `BatcherConfig`** in `crates/awp-agents/src/batcher.rs` matching the plan's defaults exactly: `max_batch_size: 10, max_batch_age_secs: 60, min_batch_size: 1`.

6. **Implement the Batcher service** in `batcher.rs`:
   - Buffers incoming attestations in memory
   - Triggers a batch when (a) buffer reaches `max_batch_size`, or (b) the oldest buffered attestation exceeds `max_batch_age_secs`
   - On trigger: builds the Merkle tree, computes per-leaf inclusion proofs, persists the `Batch` and all proofs in a single SQLite transaction, clears the buffer
   - Exposes a `submit(attestation)` method and a background task driven by `tokio::time::interval` for the age-based trigger
   - On shutdown, flushes any buffered attestations as a final batch (respecting `min_batch_size`)

7. **Wire the Dispatcher to the Batcher** — after both Worker and Verifier attestations are collected for a task, the Dispatcher submits both to the Batcher. Continue writing JSON files for backwards compatibility (the prior examples must still work), but SQLite is now the source of truth for batching. Add a `--no-batch` flag or equivalent if you need to preserve the Phase 1/2 examples without involving the Batcher; explain your choice in the PR description.

8. **`examples/full_pipeline.rs`** — the Phase 3 checkpoint. Submits 12 tasks (enough to trigger at least one count-based batch and exercise the size-10 boundary). Asserts:
   - SQLite contains attestations and at least one batch
   - For every attestation in a batch, `verify_attestation_inclusion(id)` returns `true` given only the root + proof
   - Tampering with one attestation's payload causes `verify_attestation_inclusion` to return `false`

9. **Tests**:
   - Unit tests in `merkle.rs` for tree construction with 1, 2, 3, and 16 leaves; proof verification; tampered-leaf rejection
   - Unit tests in `storage.rs` for insert/get round-trips
   - Integration test in `crates/awp-agents/tests/` that exercises the Batcher end-to-end (submit attestations, force a flush, verify a proof from the database)

10. **Update `docs/PAIN_POINTS.md`** with answers to the three "Key Questions to Answer" in the plan's Phase 3 section.

### Do Not Touch

- The `Attestation` struct's signed payload (`signing_payload`) — changing it invalidates Phase 1/2 attestations; if you need to bind extra fields, do it at the leaf-hash layer in `merkle.rs`, not by altering the signed payload
- On-chain anchoring — the plan defers it to Phase 2-of-AWP-overall (post-prototype). The `Batch` struct includes `anchor_tx` and `anchor_chain` fields; leave them as `Option::None` and write `NULL` to the DB. Do not add a chain client.
- Appendix A LuaI integration — still deferred per the plan's Recommendation
- `examples/simple_attestation.rs` and `examples/dispatcher_flow.rs` — they must continue to work; refactor only if necessary, document the change in the PR description

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/awp-prototype-plan.md` → "Weeks 5-6: Attestation Batching", with emphasis on the "Exit Criteria" — especially "Proof verification works given only root + proof + attestation".
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(core): phase 3 merkle batching and sqlite persistence`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass

cargo run --example simple_attestation
cargo run --example dispatcher_flow
# → both still work (Phase 1/2 contracts preserved)

cargo run --example full_pipeline
# → 12 tasks dispatched, attestations flowing into the Batcher
# → at least one batch flushed (count trigger at 10)
# → final batch flushed at shutdown (age trigger or shutdown flush)
# → SQLite shows batches + proofs
# → inclusion verified for every batched attestation, given only root + proof
# → tampering test confirms an altered attestation fails verification
```

---

## Phase 4 — Weeks 7-8: Evaluate & Decide

**Branch:** `phase/4-evaluation`
**Depends on:** Phase 3 merged to `main`

**Prompt:**

You are running the evaluation phase of the AWP prototype. The full specification is in `planning/awp-prototype-plan.md` under "Weeks 7-8: Evaluate & Decide". Read that section carefully before doing any work.

This phase is **not** a feature implementation — it is a structured assessment of the prior 6 weeks plus one exploratory stretch task. The human will make the final framework decision. Your job is to lay out the evidence and execute the stretch task.

### Context

Phases 1, 2, and 3 are complete and merged. The repository contains the full prototype: signed attestations, Worker–Verifier–Dispatcher orchestration, Merkle batching with SQLite persistence and inclusion proofs. The pain-points log at `docs/PAIN_POINTS.md` accumulated answers to per-phase key questions.

Three documents drive this phase:

- `docs/PAIN_POINTS.md` (already exists, populated phase-by-phase) — what was hard
- `docs/DECISIONS.md` (must be created or extended) — the design decision log + the final framework decision
- `awp-landing-page-v2.md` and `awp-prototype-plan.md` — what may need amending based on what you learned

### Your Task

1. **Read `docs/PAIN_POINTS.md` end to end** and produce a single-page synthesis at the top: the 3-5 biggest sources of friction across all three phases, each with a one-paragraph description and a concrete example (file path + symptom). Order by impact, not chronology.

2. **Pick exactly one stretch task** from the plan's Week 7-8 list:
   - **Option A: MCP integration** — expose one AWP tool (e.g. `verify_attestation`) over MCP so an external MCP client can call it. Worker/Verifier remain unchanged.
   - **Option B: Parallel workflow** — run two Verifier agents in parallel against the same Worker attestation; the Dispatcher records both verifier attestations and flags any disagreement.

   Pick the one most likely to surface a framework limitation, not the one most likely to succeed. Implement it on a feature branch off `main`. Add tests. `make check` must pass.

3. **Write `docs/DECISIONS.md`** if it does not exist; otherwise extend it. Required sections:
   - **Design decisions log** — every architectural choice made across phases 1-3 with one-paragraph rationale (e.g. "Why JSON + SQLite both?", "Why ephemeral keypairs?", "Why Dispatcher owns timeout vs. agent-internal timeouts?")
   - **Framework comparison** — fill the table from the plan's "Evaluation Criteria" section with concrete observations from the 6 weeks. Score AutoAgents based on lived experience; score swarms-rs and Custom-on-Rig based on documentation review (you may use `WebFetch` to read their docs and examples — do not actually port the code). Cite sources.
   - **Recommendation for the human** — one paragraph stating which of the four Decision Options (A/B/C/D in the plan) you think the evidence supports, with the strongest counter-argument. Be honest about uncertainty. The decision is the human's, not yours.

4. **Update `awp-prototype-plan.md`** with a "Phase 1 Postmortem" section appended after "Success Criteria (End of 8 Weeks)". List: actual scope shipped vs. planned, total commits, total LOC, number of pain points, the stretch task chosen, and a one-line link to `docs/DECISIONS.md` for the framework recommendation.

5. **Spec amendments** — if anything in `awp-landing-page-v2.md` is now wrong or misleading based on what was built, propose changes as a diff in the PR description. Do not amend the landing page directly without the human's nod (it is marketing-flavoured; your changes go through review).

### Do Not Touch

- The framework decision itself — recommend, do not decide
- Phase 2-of-AWP work (on-chain anchoring, identity registration, HTTP API) — the plan's "What's Explicitly Deferred" section is binding
- `awp-landing-page-v2.md` — propose diffs; do not commit changes to it
- The Phase 1/2/3 code beyond the stretch task scope — this is an evaluation phase, not a refactor

### Closing the Loop

When the evaluation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/awp-prototype-plan.md` → "Weeks 7-8: Evaluate & Decide". The reviewer should check that every Exit Criterion bullet is addressed in the deliverables, not that code is correct (this phase is mostly docs).
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `docs(plan): phase 4 evaluation and framework recommendation`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass (including the stretch-task tests)

# Phase 1-3 examples must still work
cargo run --example simple_attestation
cargo run --example dispatcher_flow
cargo run --example full_pipeline

# Stretch task:
# - If Option A (MCP): a documented invocation of the verify_attestation tool over MCP succeeds
# - If Option B (parallel verifiers): a new example demonstrates two Verifiers running concurrently with disagreement detection

# Documentation:
test -s docs/DECISIONS.md && grep -q "Framework comparison" docs/DECISIONS.md
test -s docs/PAIN_POINTS.md
grep -q "Phase 1 Postmortem" planning/awp-prototype-plan.md
# → all true
```
