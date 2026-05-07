# Agent Prompts — GTM Phase 1 (Pilot Readiness)

These prompts dispatch Claude Code agents to implement the four steps defined in [`gtm-phase-1-plan.md`](gtm-phase-1-plan.md). Each prompt is self-contained — agents work in isolated git worktrees and do not share state during execution.

This is a GTM-driven phase, not a protocol-driven phase. The 8-week prototype plan ([`awp-prototype-plan.md`](awp-prototype-plan.md)) is complete; the next move is making the prototype demo-able and pilot-ready for the buyer personas described in [`../awp-market-research.md`](../awp-market-research.md). The framing for that change-of-direction lives in [`../docs/USER_JOURNEYS.md`](../docs/USER_JOURNEYS.md) and [`../docs/PHASE1_REVIEW.md`](../docs/PHASE1_REVIEW.md).

The conventions referenced below live in [`/.claude/`](../.claude/):

- [`commits.md`](../.claude/commits.md) — `make check` before every commit; `type(scope): description`
- [`testing.md`](../.claude/testing.md) — `make check` is the gate; tests required for features and regressions
- [`review-gate.md`](../.claude/review-gate.md) — review agent before PR
- [`pull-requests.md`](../.claude/pull-requests.md) — draft PR + Agent Run Report comment
- [`agent-prompts.md`](../.claude/agent-prompts.md) — the template these prompts follow

## Sequencing Overview

```
Week 1 (parallel):   Step 1 — Audit Viewer       Step 2 — KYC Receipts Demo
                              │                            │
                              └────────────┬───────────────┘
                                      merge to main
                                           │
Week 2-3 (parallel): Step 3 — Persistent Identity   Step 4 — SR 11-7 Mapping
                              │                            │
                              └────────────┬───────────────┘
                                      merge to main
                                           │
Week 4 (single):     Integration polish + Month 2 launch readiness
```

**Sequencing rules:**

- Steps 1 and 2 may run in parallel — different files, different domains. Step 1 is HTML/JS in `tools/audit-viewer/`; Step 2 is Rust in `crates/` and `examples/`.
- **Both Step 1 and Step 2 must be merged to `main`** before Step 3 starts. Step 3 changes constructor signatures on `Worker` and `Verifier`, which Step 2's example consumes; the audit viewer renders Step 2's output. Merge ordering keeps the demos working.
- Steps 3 and 4 may run in parallel — Step 3 is Rust code, Step 4 is markdown documentation; they cannot collide.
- Use separate git worktrees for parallel pairs.

## Scope notes for the human

A few items in `gtm-phase-1-plan.md` are **not** for the dispatched agent:

- **Recording the 3-minute demo video.** Step 4's exit checklist ends with "a 3-minute demo video script can plausibly be drafted." Drafting and recording is GTM Month 2 work, not GTM Phase 1 implementation work.
- **Final Persona A conversations.** The SR 11-7 mapping must be read by someone with actual regulatory knowledge before being shown to a buyer. The reviewing agent checks the cite-correctness; you check the regulatory plausibility.
- **Decisions on overclaim borderlines.** Step 4's review-gate flags overclaims; deciding which clause survives is your call.

The verification commands in each prompt assume `make check` exists (unchanged from the prototype phases). Step 4 is markdown-only and does not invoke `make check`.

---

## Step 1 — GTM Phase 1: Audit Viewer

**Branch:** `gtm-phase-1/audit-viewer`

**Prompt:**

You are implementing Step 1 of GTM Phase 1 of AWP. The full specification is in `planning/gtm-phase-1-plan.md` under "Step 1 — Audit Viewer". Read that section carefully before writing any code.

### Context

The repository contains a complete prototype: `awp-core` (attestations, signing, Merkle, SQLite), `awp-agents` (Worker, Verifier, Dispatcher, ParallelDispatcher, Batcher), and four CLI examples that produce JSONL logs at `data/attestations.json` and `data/executions.json`. Run any example (e.g. `cargo run --example dispatcher_flow`) to populate those files; their schemas are documented in `crates/awp-core/src/attestation.rs`, `crates/awp-core/src/task.rs`, and `crates/awp-core/src/execution.rs`.

The system has no UI today. The architecture doc ([`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md)) describes the data shapes; you can read JSONL files directly to confirm structure.

Your job is to add a static HTML viewer that renders the receipts legibly to a non-developer audience. This is the buyer-facing artefact for Persona A (compliance lead Sarah) per [`docs/USER_JOURNEYS.md`](../docs/USER_JOURNEYS.md).

### Your Task

1. **Create `tools/audit-viewer/index.html`** — a single-file static HTML+CSS+vanilla-JS app. **No build step. No `npm install`. No framework.** Open in a browser by double-click.

2. **File loading UI:** drag-and-drop area plus `<input type="file" multiple>` accepting both `attestations.json` and `executions.json`. Display the loaded file names and record counts.

3. **Schema reference:**
   - `Attestation` — see `crates/awp-core/src/attestation.rs`. Fields: `id, agent_id, agent_pubkey ([u8;32]), task_hash, output_hash, output, status, references, timestamp (i64 epoch seconds), signature ([u8;64])`.
   - `AttestationStatus` — `"Completed"`, `{"Failed": "<reason>"}`, `{"Verified": {"attestation_valid": bool, "answer_correct": bool}}`.
   - `TaskExecutionRecord` (in `executions.json`) — see `crates/awp-core/src/execution.rs`. Fields: `task_id, task_input, status, worker_attestation_id, verifier_attestation_id, started_at, completed_at`. The execution joins to attestations by id.
   - JSONL format: one JSON object per line, no surrounding array.

4. **Timeline rendering:** chronological table, one row per `TaskExecutionRecord`, columns: timestamp, agent ids (worker → verifier), task input (truncated), decision (worker output, truncated), verification status badge.

5. **Status badges:**
   - 🟢 green — `Verified{attestation_valid: true, answer_correct: true}`
   - 🟡 amber — `Verified{...}` with either field false
   - 🔴 red — `Failed`
   - ⚪ grey — orphaned attestation with no execution row

6. **Row expansion:** click a row to show full signed attestation payloads (worker + verifier), agent public keys (hex, truncate middle to first 8 / last 8 chars), `references` link, status detail, signature (hex, truncated similarly).

7. **In-browser signature verification:** vendor a small ed25519 library (e.g. `@noble/ed25519` — minified UMD bundle copied into `tools/audit-viewer/vendor/`) and re-verify each `Attestation`'s signature client-side. Display "✓ signature verified in browser" when valid; "✗ signature invalid" in red when not. The verification must use the same canonical signing payload as the Rust implementation: every field except `signature`, in the same order, serialised as canonical JSON. Read `Attestation::signing_payload` in `crates/awp-core/src/attestation.rs` and replicate it exactly. Add a unit test in JS confirming you produce byte-identical output for a known attestation.

8. **Aesthetic constraint:** clean, monochrome, audit-document feel. System fonts. No flashy animations. Sarah is showing this to her external auditor.

9. **`tools/audit-viewer/README.md`:** explain (a) how to use it (open in browser, drag JSONL files), (b) the in-browser verification claim and what it means, (c) how to produce sample data using `cargo run --example dispatcher_flow`.

### Stubs to pre-place

This step does not create any Rust files, so there are no merge collisions with Step 2 (which is Rust-only). No stubs required.

### Do Not Touch

- `crates/` — Step 2's domain (and you have no reason to change Rust code from a viewer)
- `examples/` — Step 2's domain
- `docs/` — Step 4 will edit `docs/USER_JOURNEYS.md`; do not modify it
- The `Attestation::signing_payload` Rust implementation — your JS must match it, not the other way around. If they disagree, Rust is the source of truth.

### Closing the Loop

When implementation is complete:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-1-plan.md` → "Step 1 — Audit Viewer", with emphasis on the exit criteria (especially "tampering with the JSONL by hand causes the badge to flip red").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(viewer): gtm phase 1 audit viewer for receipts`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
# Generate sample data
cargo run --example dispatcher_flow
# → data/attestations.json and data/executions.json populated

# Open the viewer
open tools/audit-viewer/index.html
# → drag in both files
# → timeline renders 3 rows (one per task)
# → all badges accurate (one green, one red, one amber)
# → click a row → expanded view shows truncated pubkeys, signatures, references
# → "✓ signature verified in browser" appears under each receipt

# Tamper test
# (manually edit one byte of a signature in attestations.json)
# Reload viewer with the tampered file
# → "✗ signature invalid" shows for that record
# → badge flips red

# Make check still passes (no Rust changes)
make check
# → passes
```

---

## Step 2 — GTM Phase 1: KYC Receipts Demo

**Branch:** `gtm-phase-1/kyc-receipts`

**Prompt:**

You are implementing Step 2 of GTM Phase 1 of AWP. The full specification is in `planning/gtm-phase-1-plan.md` under "Step 2 — KYC Receipts Demo". Read that section carefully before writing any code.

### Context

The repository contains a complete prototype with four examples (`simple_attestation`, `dispatcher_flow`, `full_pipeline`, `parallel_verifiers`). All use a `WorkerTask` containing an arithmetic expression, with a `calculate` tool in `crates/awp-agents/src/tools.rs`. The Worker and Verifier agents (`crates/awp-agents/src/worker.rs`, `verifier.rs`) construct attestations after running the tool. The `Dispatcher` (`crates/awp-agents/src/dispatcher.rs`) coordinates them with timeouts.

Your job is to add a vertical-flavoured demo that replaces arithmetic with a KYC (Know Your Customer) decision rule. This shifts the headline example from `7+13=20` to a customer-resonant scenario per `docs/USER_JOURNEYS.md` (Persona A: compliance lead Sarah).

This step is **Rust-only.** Step 1 (the audit viewer) ships in parallel and is HTML/JS — no merge collisions.

### Your Task

1. **Add a `KycRequest` and `KycDecision` type** in `crates/awp-core/src/lib.rs` (or a new `kyc.rs` module — your call, justify in the PR):
   ```rust
   pub struct KycRequest {
       pub customer_id: String,
       pub amount_cents: u64,
       pub jurisdiction: String,
       pub transaction_type: String,
   }

   pub enum KycDecision {
       Approve,
       Flag { reasons: Vec<String> },
       Reject { reasons: Vec<String> },
   }
   ```
   Add `serde::Serialize`/`Deserialize` derives. Add `canonical_bytes()` on `KycRequest` returning a deterministic byte serialisation (use serde_json with sorted keys, or hand-roll — match what `WorkerTask::canonical_bytes` does).

2. **Add a `kyc_decide` tool** in `crates/awp-agents/src/tools.rs`:
   ```rust
   pub fn kyc_decide(request: &KycRequest) -> KycDecision { ... }
   ```
   Rule (deterministic, no external state):
   - High-risk jurisdictions (hardcoded list, e.g. `["XX", "YY"]` — use ISO-3166-style placeholders, *not* real country codes, to avoid implying a real geopolitical claim): **Flag** with reason "high-risk jurisdiction"
   - Amount > 10_000_00 cents ($10,000 USD-equivalent): **Flag** with reason "amount exceeds threshold"
   - Transaction type "wire" with amount > 100_000_00 cents ($100,000): **Reject** with reason "high-value wire requires manual review"
   - Otherwise: **Approve**
   Add unit tests covering all branches.

3. **Worker / Verifier reuse:** the existing `WorkerAgent` / `VerifierAgent` traits are parameterised over a fixed `WorkerTask` type. You have two options:
   - **(a)** Generalise the traits to be generic over the task type. Higher refactor cost; benefits future demos.
   - **(b)** Add `KycWorker` and `KycVerifier` structs alongside the existing `Worker` / `Verifier`, satisfying the same trait shape but for `KycRequest`/`KycDecision`. Lower refactor cost; some duplication.

   Pick option (b) unless option (a) is cleaner than expected — call out the choice in the PR description with reasoning.

4. **Create `examples/kyc_receipts.rs`** modelled on `examples/dispatcher_flow.rs`. Run three scenarios:
   - **Approve scenario:** small domestic transaction. Worker decides Approve; Verifier independently re-decides and agrees.
   - **Flag scenario:** large amount. Worker decides Flag with reasons; Verifier agrees.
   - **Tampered scenario:** Worker decides Flag, signs the attestation, then **after signing** the example code mutates the `output` field in the in-memory attestation (or its persisted form before the Verifier loads it). Verifier's `verify_attestation_struct` call should detect the tamper (signature invalid). Verifier emits `Verified{attestation_valid: false, ...}`. The demo's stdout must clearly explain *why* the verifier rejected the receipt.

   For the tampered scenario you have flexibility in how you induce the tamper — keep it simple and document what you did.

5. **Output format:** print customer-resonant strings, *not* `attestation_valid=true answer_correct=true`. Example:
   ```
   Customer #4711 — Decision: APPROVE
   Worker:    agent-kyc-01  signature ✓
   Verifier:  agent-kyc-02  signature ✓ verdict ✓
   Receipt:   data/attestations.json (id: <uuid>)
   Audit-ready: yes
   ```
   For the tampered scenario:
   ```
   Customer #9999 — Decision: FLAG
   Worker:    agent-kyc-01  signature ✓
   Verifier:  agent-kyc-02  signature ✗ — receipt rejected
   Reason:    payload was modified after signing; receipt is not audit-ready.
   ```

6. **Update the repo `README.md`:** lead the "Examples" section with `kyc_receipts` (with one-paragraph description), demote `simple_attestation` to "Phase 1 checkpoint." Keep the others in their existing order.

7. **Tests:**
   - Unit tests in `tools.rs` for `kyc_decide` (each branch)
   - Integration test in `crates/awp-agents/tests/` exercising the tampered path end-to-end and asserting the Verifier returns `attestation_valid: false`

### Stubs to pre-place

If you anticipate that Step 3 will need to know about `AgentIdentity` in your example, **do not** pre-place stubs — Step 3 explicitly preserves backwards-compat constructors, so your example will keep working without changes. The Step 3 agent updates your example to use the persistent path; you do not need to.

### Do Not Touch

- `tools/audit-viewer/` — Step 1's domain (it's HTML/JS; you have no reason to touch it)
- `crates/awp-core/src/identity.rs` — does not exist yet, Step 3's domain
- `docs/compliance/` — does not exist yet, Step 4's domain
- The four existing examples — must continue to work unchanged (regression risk if you generalise the trait under option (a)). If you take option (a), prove the existing examples still work in CI.

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-1-plan.md` → "Step 2 — KYC Receipts Demo", with emphasis on the exit criteria (especially "tampered scenario produces clear stdout").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(examples): gtm phase 1 kyc receipts demo`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass (including new kyc_decide unit tests + tampered integration test)

# All existing examples still work
cargo run --example simple_attestation
cargo run --example dispatcher_flow
cargo run --example full_pipeline
cargo run --example parallel_verifiers
# → all four succeed unchanged

# New demo
cargo run --example kyc_receipts
# → 3 scenarios printed:
#   - "Customer #... Decision: APPROVE  signature ✓ verdict ✓"
#   - "Customer #... Decision: FLAG     signature ✓ verdict ✓"
#   - "Customer #... Decision: FLAG     signature ✗ — receipt rejected"
# → data/attestations.json and data/executions.json populated

# README leads with kyc_receipts
grep -A 2 "## Examples" README.md | head -5
# → first listed example is kyc_receipts
```

---

## Step 3 — GTM Phase 1: Persistent Agent Identity

**Branch:** `gtm-phase-1/persistent-identity`
**Depends on:** Steps 1 and 2 merged to `main`

**Prompt:**

You are implementing Step 3 of GTM Phase 1 of AWP. The full specification is in `planning/gtm-phase-1-plan.md` under "Step 3 — Persistent Agent Identity". Read that section carefully before writing any code.

### Context

Steps 1 and 2 are complete and merged. The repository now contains:

- The full prototype (`awp-core`, `awp-agents`, four original examples)
- A new `kyc_receipts` example using the `Dispatcher` with `KycWorker` / `KycVerifier` (or generic `Worker` / `Verifier` if Step 2 took option (a))
- A static HTML audit viewer at `tools/audit-viewer/` that renders attestations and re-verifies signatures in-browser
- All `Worker::new(agent_id)` and `Verifier::new(agent_id)` constructors generate ephemeral keypairs internally — restart the process and the agent has a new identity

This is the load-bearing gap for Persona B (Marcus, agent platform operator) per `docs/USER_JOURNEYS.md`. Settlement systems cannot use receipts whose signing keys vanish on restart. Your job is to add disk-backed persistent identity while preserving backwards compatibility for the four original examples.

### Your Task

1. **Create `crates/awp-core/src/identity.rs`** with:
   ```rust
   pub struct AgentIdentity {
       pub agent_id: String,
       pub keypair: AgentKeypair,
   }

   pub trait IdentityStore {
       fn load(&self, agent_id: &str) -> Result<Option<AgentIdentity>>;
       fn save(&self, identity: &AgentIdentity) -> Result<()>;
   }

   pub struct FileIdentityStore { /* path to identities dir */ }
   ```

2. **`AgentIdentity` constructors:**
   - `AgentIdentity::generate(agent_id: impl Into<String>) -> Self` — fresh keypair, no persistence (this is what `Worker::new` will call internally to preserve backwards compatibility)
   - `AgentIdentity::load_or_create(store: &dyn IdentityStore, agent_id: &str) -> Result<Self>` — load from store if present; else generate and save before returning

3. **`FileIdentityStore`:**
   - Constructor: `FileIdentityStore::new(dir: impl AsRef<Path>) -> Self` — directory where identity files live (default `data/identities/`)
   - On-disk format: `<dir>/<agent_id>.json` containing `{"agent_id": "...", "secret_key": "<hex>", "public_key": "<hex>"}`. Hex-encode secret and public bytes (32 bytes each). Use serde for the file shape.
   - File permissions on Unix: 0600 for the JSON file (`std::os::unix::fs::PermissionsExt`). On Windows, document the gap in module-level rustdoc and emit a warning to stderr if running on Windows on first save.
   - On `load`: if the file does not exist, return `Ok(None)`. If it exists but does not parse, return `Err`. If permissions are too permissive on Unix, log a warning to stderr but still load.
   - On `save`: create the directory if it does not exist; write atomically (write to a tempfile, rename) to avoid partial writes.

4. **Update `Worker` and `Verifier` constructors:**
   - Add `Worker::with_identity(identity: AgentIdentity) -> Self` and `Verifier::with_identity(identity: AgentIdentity) -> Self`
   - **Preserve backwards compatibility:** keep `Worker::new(agent_id: impl Into<String>)` working — it should now internally call `Self::with_identity(AgentIdentity::generate(agent_id))`. Same for `Verifier`. The four original examples must keep running unchanged.
   - Same for `KycWorker` / `KycVerifier` if they exist as separate types from Step 2.

5. **Update `examples/kyc_receipts.rs`** (Step 2's example) to use the persistent path:
   ```rust
   let store = FileIdentityStore::new("data/identities");
   let worker_identity = AgentIdentity::load_or_create(&store, "agent-kyc-01")?;
   let worker = KycWorker::with_identity(worker_identity);
   ```
   Same for the Verifier. Re-running the demo should produce attestations from the same agent IDs and public keys as the previous run.

6. **Update the audit viewer (Step 1)** to display "Same identity as N previous receipts in this view" or similar when it sees an agent public key it has rendered before in the **current session** (no persistence in the viewer itself — just per-page-load deduplication). This is a small JS change to `tools/audit-viewer/index.html`.

7. **Module-level rustdoc on `identity.rs`:** document the on-disk format, file permissions, and a security note that the file-based store is appropriate for development and demo use; production deployments should use OS keychain, HSM, or KMS-backed stores. Include an example impl skeleton in the doc:
   ```rust
   /// // Production example skeleton:
   /// // struct KmsIdentityStore { client: KmsClient }
   /// // impl IdentityStore for KmsIdentityStore { ... }
   ```

8. **Tests:**
   - Unit tests for `FileIdentityStore::save` / `load` round-trip
   - Unit test for `load_or_create` — first call generates and saves; second call loads the same identity
   - Unit test confirming Unix file permissions are 0600 after save (Unix only — `#[cfg(unix)]`)
   - Signature round-trip test: load an identity, sign an attestation, verify with the loaded public key
   - Test that the four original examples (which use `Worker::new` / `Verifier::new`) still produce ephemeral identities and work unchanged

### Stubs to pre-place

None required — this is the last code step in this phase.

### Do Not Touch

- `tools/audit-viewer/` other than the small "same identity as N previous" change in (6) — keep your changes minimal and visible in the diff
- `docs/compliance/` — Step 4's domain (in flight in parallel)
- `examples/simple_attestation.rs`, `examples/dispatcher_flow.rs`, `examples/full_pipeline.rs`, `examples/parallel_verifiers.rs` — must keep working unchanged via the `Worker::new` / `Verifier::new` backwards-compat path

### Closing the Loop

When implementation is complete and `make check` passes:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-1-plan.md` → "Step 3 — Persistent Agent Identity", with emphasis on the exit criteria (especially "two consecutive `kyc_receipts` runs produce attestations from the same `agent_pubkey`").
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `feat(core): gtm phase 1 persistent agent identity`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
make check
# → fmt + clippy clean, all tests pass

# Identity persistence
rm -rf data/identities
cargo run --example kyc_receipts
# → first run generates and saves data/identities/agent-kyc-01.json + agent-kyc-02.json
# → note the agent_pubkey values in the stdout

cargo run --example kyc_receipts
# → second run reuses the same keypairs
# → agent_pubkey values match the first run

ls -la data/identities/
# → on Unix: -rw------- (mode 0600)

# Removing identity → regenerate
rm data/identities/agent-kyc-01.json
cargo run --example kyc_receipts
# → agent-kyc-01 regenerates with a new pubkey; agent-kyc-02 keeps its existing one

# Backwards compat — original examples
cargo run --example simple_attestation
cargo run --example dispatcher_flow
cargo run --example full_pipeline
cargo run --example parallel_verifiers
# → all four work unchanged; they use ephemeral identities (no files in data/identities/)

# Audit viewer surfacing same-identity badge
open tools/audit-viewer/index.html
# → drag in data/attestations.json (which contains 2+ runs of kyc_receipts)
# → "Same identity as N previous receipts" badge appears for repeated agent pubkeys
```

---

## Step 4 — GTM Phase 1: SR 11-7 Compliance Pre-Mapping

**Branch:** `gtm-phase-1/sr-11-7-mapping`
**Depends on:** Steps 1 and 2 merged to `main`

**Prompt:**

You are producing the SR 11-7 compliance pre-mapping for AWP. The full specification is in `planning/gtm-phase-1-plan.md` under "Step 4 — SR 11-7 Compliance Pre-Mapping". Read that section carefully — including the "Critical guardrail" — before writing.

This step is **markdown-only**. There are no Rust changes. The verification is doc accuracy and reviewer-checked non-overclaim, not `make check`.

### Context

The repository contains a complete prototype that produces signed `Attestation` records of agent decisions, with cryptographic verification, durable JSONL/SQLite logs, and (after Steps 1-3) a human-readable audit viewer plus persistent agent identity. The KYC receipts demo from Step 2 is the working example you cite for concreteness.

The buyer for this document is the compliance lead at a US-regulated mid-market firm (Persona A in `docs/USER_JOURNEYS.md`). They will read this document either before or alongside the 3-minute demo video. Per the GTM plan (`awp-market-research.md` §4.2 lever #5), this is "legal interpretation work that compounds" — it is both a sales asset and a defensibility lever.

### Critical guardrail (must read)

**SR 11-7 covers model lifecycle governance broadly. AWP attests to *agent execution events*.** That is one strand of one section. Any clause you cite as "covered by AWP" must be honestly defensible — overclaiming damages credibility with the exact buyers we are trying to win.

The reviewing agent (Closing the Loop step) will explicitly check for overclaims. If a cited AWP feature is *necessary but not sufficient* for compliance with a clause, say so plainly. The "What AWP does not provide" section must be at least as substantial as the "covered" section.

### Your Task

1. **Create `docs/compliance/SR_11_7.md`** with these sections in order:

   **(a) Preamble** (~150 words)
   - What SR 11-7 is: US Federal Reserve Supervisory Letter SR 11-7, "Guidance on Model Risk Management" (2011), now widely referenced for AI / agent governance in regulated US financial services. Cite the document name and date; do not link to a paywalled source.
   - Why it matters for agent-driven decisions: SR 11-7 covers model lifecycle, including ongoing use, documentation, and audit trails. AI agents that make decisions ("approve this loan," "flag this transaction") fall within its scope as the institution's models.
   - AWP's claim scope: AWP attests to *what the agent did* (input, output, time, signature, optional independent verification). It does not attest to *whether the model is correct under SR 11-7's broader requirements*.

   **(b) Scope and limits** (~200 words)
   - What AWP attestations help with: non-repudiable record of agent decisions, independent verification by a second agent (caught tamper / disagreement), durable audit trail, cryptographic timestamps, batched inclusion proofs for efficient long-term storage.
   - What AWP attestations do *not* address: model validation methodology, ongoing performance monitoring, governance committee structures, change management, risk appetite frameworks, third-party model risk.
   - Recommended posture: AWP is one of multiple controls in an SR 11-7-aligned program. It is the "agent decision provenance" control specifically.

   **(c) Clause-by-clause mapping table** (the centrepiece)
   - Table with columns: SR 11-7 clause / subsection | Requirement summary | AWP feature(s) that help | Necessary-but-not-sufficient? (Y/N) | Notes
   - Cite specific SR 11-7 sections — at minimum, you should cover sections in:
     - III. Model Development, Implementation, and Use (especially: documentation, ongoing use)
     - IV. Model Validation (especially: outcome analysis — where AWP's verifier disagreement detection helps)
     - V. Governance, Policies, and Controls (especially: documentation, internal audit)
   - Cite the AWP feature by file or doc reference: e.g. *"`Attestation.timestamp` + `Attestation.signature` (`crates/awp-core/src/attestation.rs`)"*, *"Verifier disagreement detection (`crates/awp-agents/src/dispatcher.rs`)"*, *"Merkle inclusion proofs (`crates/awp-core/src/merkle.rs`)"*.
   - For each row, the "Necessary-but-not-sufficient?" column is critical. Most rows should be **Y**. Only mark a row **N** (i.e. AWP fully satisfies the clause) if you are confident — and even then, document the assumption.

   **(d) Sample audit narrative** (~250 words)
   - A worked example: a Risk Officer at a mid-market bank is asked by an examiner, *"Show me how you ensure your KYC agent's decisions are auditable and verifiable."*
   - The Risk Officer's answer references: the KYC receipts demo's output (audit viewer screenshot or stdout block), the signed attestations, the verifier's independent re-decision, and the long-term retention via Merkle batching.
   - Conclude with what the examiner *still needs to see beyond AWP* (model validation reports, governance documentation, etc.) — because the answer "AWP shows everything" would be an overclaim.

   **(e) What AWP does not provide** (~250 words, MUST be at least as substantial as section (b))
   - Itemise SR 11-7 requirements that are out of scope:
     - Model validation methodology (how was the KYC rule chosen? AWP does not address this)
     - Ongoing model performance monitoring (drift, accuracy over time — observability tools, not AWP)
     - Governance committee structures (who approves the model? AWP does not encode this)
     - Change management (how is a model update controlled? AWP can attest to use-of-version-X but does not approve version X)
     - Third-party model risk (vendor LLMs, MCP tools — AWP attests to *use*, not to vendor risk)
     - Risk appetite frameworks
     - Stress testing
   - For each: one sentence on why it is out of scope, and one sentence on what control or tool typically addresses it.

2. **Create `docs/compliance/README.md`** as the index:
   - One-paragraph overview: this directory contains AWP-to-regulation mapping documents. Each document maps AWP attestation features to specific clauses of one regulation. They are decision-support, not legal advice.
   - List of current and planned mappings:
     - **SR 11-7** ([`SR_11_7.md`](SR_11_7.md)) — current. US Federal Reserve guidance on model risk management.
     - **EU AI Act** — planned (GTM Phase 2 or later)
     - **HIPAA** — planned (GTM Phase 2 or later, healthcare vertical)

3. **Update `docs/USER_JOURNEYS.md`'s "Where the prototype's gaps bite each journey" table:** the row reading "Pilot → Adoption (Sarah) | No compliance pre-mapping doc" should be updated to *"✓ SR 11-7 ([`compliance/SR_11_7.md`](compliance/SR_11_7.md)) — additional regulations TBD"* (or similar). Severity column to "Resolved (partial)".

### Stubs to pre-place

None — this is markdown only.

### Do Not Touch

- All Rust code — this is a markdown-only step
- `tools/audit-viewer/` — Step 1's domain
- `docs/PROCESS_OVERVIEW.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `docs/PAIN_POINTS.md`, `docs/PHASE1_REVIEW.md` — they reference the prototype, not the compliance work; keep them as-is
- `awp-market-research.md` — it is the source of the GTM framing, not something to amend in this step

### Closing the Loop

When the document is complete:
1. Spawn the review agent per `.claude/review-gate.md` against `planning/gtm-phase-1-plan.md` → "Step 4 — SR 11-7 Compliance Pre-Mapping", with **explicit emphasis on the overclaim guardrail.** The reviewer must check that every cited AWP feature is verifiable in code and that the "necessary-but-not-sufficient" flag is correctly applied. The "What AWP does not provide" section must be at least as substantial as the "Scope and limits" section.
2. Capture the review agent's structured report.
3. Open a draft PR per `.claude/pull-requests.md` (target `main`, title `docs(compliance): gtm phase 1 SR 11-7 pre-mapping`).
4. Post the Agent Run Report comment combining `git log main..HEAD --oneline` with the review report.

### Verification

```bash
# All Rust still works (you didn't touch it, but let's be sure)
make check
# → passes unchanged

# Documents exist and are well-formed markdown
test -s docs/compliance/SR_11_7.md
test -s docs/compliance/README.md
grep -q "What AWP does not provide" docs/compliance/SR_11_7.md
grep -q "Necessary-but-not-sufficient" docs/compliance/SR_11_7.md
# → all true

# Cross-link from USER_JOURNEYS.md
grep -q "compliance/SR_11_7.md" docs/USER_JOURNEYS.md
# → true

# Word count sanity (rough)
wc -w docs/compliance/SR_11_7.md
# → ~1000-1500 words; if much shorter, sections are likely thin

# Reviewer (Closing the Loop) confirms no overclaim
# → see the review agent's report
```
