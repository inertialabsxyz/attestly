# GTM Phase 1 — Pilot Readiness Plan

**Duration:** 4 weeks
**Driver:** Six-month GTM plan in [`../awp-market-research.md`](../awp-market-research.md), Months 1–3 (positioning → public launch → design partner hunt).
**Goal:** Make the prototype demo-able to a Persona A compliance lead and pilot-ready for a Persona B agent platform operator.

## Context

The 8-week prototype plan ([`awp-prototype-plan.md`](awp-prototype-plan.md)) is complete: signed attestations, Worker–Verifier–Dispatcher orchestration, Merkle batching with SQLite persistence and inclusion proofs, and a Phase 4 stretch task (parallel verifiers). All four phases are merged to `main` and the system runs end-to-end (see [`../docs/PROCESS_OVERVIEW.md`](../docs/PROCESS_OVERVIEW.md)).

This plan supersedes the protocol-driven "Phase 2 of AWP overall" sequencing in `awp-prototype-plan.md` → "Next Steps After Week 8" for the next four weeks. The supersession is documented in [`../docs/PHASE1_REVIEW.md`](../docs/PHASE1_REVIEW.md) and [`../docs/USER_JOURNEYS.md`](../docs/USER_JOURNEYS.md).

The driving insight: **two of the four "deferred" gaps in [`../docs/PROCESS_OVERVIEW.md`](../docs/PROCESS_OVERVIEW.md) are load-bearing for buyer-persona pilots, not just deferred items.** Persistent agent identity is load-bearing for Persona B (Marcus, agent platform operator); a human-readable audit viewer is load-bearing for Persona A (Sarah, compliance lead). Closing them — plus shipping a vertical-flavoured demo and a single compliance pre-mapping — is the next-six-months priority per the GTM plan.

## Scope

In scope for this phase:

1. **Audit viewer** — static HTML reading `attestations.json` + `executions.json`, rendering a chronological timeline with verification badges. Sarah's pilot moment.
2. **Vertical receipts demo** — KYC-flavoured CLI example replacing arithmetic with a deterministic risk-rule decision. Gives the viewer something legible to render and shifts the headline example away from `7+13=20`.
3. **Persistent agent identity** — disk-backed keypair store; agents survive process restart with stable identity. Marcus's pilot unblock.
4. **Compliance pre-mapping** — one document mapping AWP attestation features to specific SR 11-7 (model risk management) clauses. Persona A sales asset and the first concrete instance of GTM §4.2 lever #5.

Explicitly **out of scope** for this phase:

- **On-chain anchoring.** Per GTM §4.1 lock #1, chain integration is a footnote feature, not the headline. Defer indefinitely until a Persona C deal explicitly asks for it.
- **AutoAgents framework integration.** Per [`../docs/DECISIONS.md`](../docs/DECISIONS.md) recommendation, defer until LLM integration is the next-task. None of the GTM Phase 1 steps wire in an LLM.
- **Python / TypeScript SDKs.** Useful for Persona B but a multi-week effort each; deferred to GTM Phase 2.
- **Hosted service (`awp-cloud`).** Per GTM §4.1 lock #3, this is non-negotiable for revenue but is Month 3+ work. The prototype's library-and-CLI shape is sufficient for Month 2's demo video and Month 3's design-partner conversations.
- **`TaskExecution` coordination-type generalisation.** PAIN_POINTS synthesis #3 flagged this as compounding, but the GTM Phase 1 work does not introduce a third coordination shape. Revisit in GTM Phase 2.
- **Storage consolidation.** PAIN_POINTS synthesis #4 — three storage models — does not bite any of the four GTM Phase 1 steps. Revisit in GTM Phase 2.

## Sequencing

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
Week 4 (single):     Integration polish + Month 2 launch readiness check
```

Steps 1 and 2 run in parallel: the viewer is HTML reading existing JSONL output, the demo is a new Rust example replacing the `calculate` task. Both must merge to `main` before Step 3 starts because Step 3 changes how keypairs are constructed, which both demos depend on.

Steps 3 and 4 run in parallel: code (identity) and markdown (mapping doc) cannot collide.

## Step 1 — Audit Viewer

**Owner:** dispatched agent on `gtm-phase-1/audit-viewer`

### Deliverables

1. `tools/audit-viewer/index.html` — single-file static HTML+CSS+vanilla-JS app (no build step, no framework dependency).
2. The viewer accepts JSONL files dropped onto the page (or selected via `<input type="file">`) — both `attestations.json` and `executions.json`.
3. Renders a chronological timeline: one row per `TaskExecution`, with sender, receiver (verifier), task input, decision, and verification status.
4. Verification status badges: green for `Verified{true,true}`, amber for partial (`{true,false}` etc.), red for `Failed`, grey for unbatched.
5. Click a row → expand to show the full signed `Attestation` payload, the verifier's verdict, and the agent public keys (hex-encoded, truncated).
6. Pure client-side: re-verify ed25519 signatures in JavaScript using a small ed25519 library (e.g. `@noble/ed25519` or equivalent vendored) so the viewer demonstrates "trust nothing, verify everything." Display "✓ signature verified in browser" under each receipt.
7. Aesthetic: clean, monochrome, audit-document-feel — not a flashy dashboard. Sarah is reading this *to her auditor*.
8. README at `tools/audit-viewer/README.md` explaining how to use it (open in browser, drag in JSONL files from `data/`).

### Exit criteria

- [ ] Open `tools/audit-viewer/index.html` in a browser; drag in `data/attestations.json` and `data/executions.json` from the existing examples
- [ ] Timeline renders with all executions visible
- [ ] Verification status badges accurate (cross-check against the CLI output)
- [ ] In-browser signature verification confirms each receipt independently — tampering with the JSONL by hand causes the badge to flip red
- [ ] No build step; no `npm install`; no server. Single file, double-clickable.
- [ ] README explains usage clearly enough for a non-developer to follow

## Step 2 — KYC Receipts Demo

**Owner:** dispatched agent on `gtm-phase-1/kyc-receipts`

### Deliverables

1. New example `examples/kyc_receipts.rs` modelled on `examples/dispatcher_flow.rs` (Dispatcher with single Verifier).
2. Replace the `calculate` arithmetic task with a `kyc_decision` rule:
   - Input: a `KycRequest` struct — `customer_id: String, amount_cents: u64, jurisdiction: String, transaction_type: String`
   - Rule: a deterministic risk function — e.g. `if amount_cents > 1_000_000 || HIGH_RISK_JURISDICTIONS.contains(&jurisdiction) { Flag } else { Approve }`
   - Output: a `KycDecision` enum — `Approve | Flag | Reject` with a structured `reasons: Vec<String>` field
3. New tool in `crates/awp-agents/src/tools.rs`: `kyc_decide(request: &KycRequest) -> KycDecision`. Pure function; deterministic; no external state.
4. Worker / Verifier reuse: do **not** introduce a new agent type. The existing `Worker` / `Verifier` traits should be parameterised over the task type, or a new `KycWorker` / `KycVerifier` should ship that satisfies the same traits. Pick whichever requires fewer changes — call out the choice in the PR description.
5. The demo runs three scenarios printed to stdout in order:
   - **Approve** — small domestic transaction, unanimous verifier agreement
   - **Flag** — large amount, unanimous verifier agreement
   - **Tampered** — Worker emits a `Flag` decision but the underlying `KycRequest` is altered post-signing; Verifier catches the tamper via signature mismatch and emits `Verified{attestation_valid: false, ...}`
6. Output prints customer-resonant strings — *not* `attestation_valid=true answer_correct=true`. Example:

   ```
   Customer #4711 — Decision: APPROVE
   Worker:    agent-kyc-01  signature ✓
   Verifier:  agent-kyc-02  signature ✓ verdict ✓
   Receipt:   ./data/attestations.json (id: ...)
   Audit-ready: yes
   ```

7. Update `README.md` at the repo root: lead the "Examples" section with `kyc_receipts`, demote `simple_attestation` to "Phase 1 checkpoint."

### Exit criteria

- [ ] `cargo run --example kyc_receipts` succeeds with all three scenarios printing the expected outputs
- [ ] `data/attestations.json` and `data/executions.json` get the expected appended records
- [ ] Tampered scenario produces a Verifier attestation with `attestation_valid: false` and the demo's stdout flags it clearly
- [ ] Existing `simple_attestation`, `dispatcher_flow`, `full_pipeline`, `parallel_verifiers` examples still work unchanged
- [ ] `make check` passes (new tests for `kyc_decide` and at least one integration test for the tampered path)
- [ ] README's Examples section leads with `kyc_receipts`

## Step 3 — Persistent Agent Identity

**Owner:** dispatched agent on `gtm-phase-1/persistent-identity`

### Deliverables

1. New module `crates/awp-core/src/identity.rs` defining:
   - `AgentIdentity` struct: `agent_id: String, keypair: AgentKeypair`
   - `IdentityStore` trait: `load(agent_id: &str) -> Result<Option<AgentIdentity>>`, `save(identity: &AgentIdentity) -> Result<()>`
   - `FileIdentityStore` impl: stores keypairs at `data/identities/<agent_id>.json` (or similar); the on-disk format includes `agent_id`, the secret key bytes, and the public key bytes; secret key bytes are written with restrictive file permissions (0600 on Unix)
   - `AgentIdentity::load_or_create(store: &dyn IdentityStore, agent_id: &str) -> Result<AgentIdentity>` — load if present, otherwise generate and save before returning
2. `Worker::new` and `Verifier::new` accept an `AgentIdentity` instead of generating a fresh keypair internally. Add `Worker::with_identity` and `Verifier::with_identity` constructors. Keep `Worker::new(agent_id)` working as a convenience that calls `AgentIdentity::generate(agent_id)` for ephemeral identities (preserving backwards compatibility with all existing examples).
3. The `kyc_receipts` demo uses the persistent path: `AgentIdentity::load_or_create(&store, "agent-kyc-01")` etc. Re-running the demo should produce attestations from the **same** agent IDs and public keys as the previous run.
4. Update the audit viewer (Step 1) to display "Same identity as N previous receipts" or similar when it sees an agent public key it has rendered before in the same session. (Lightweight — does not require persistence in the viewer itself.)
5. Document the on-disk format in `crates/awp-core/src/identity.rs` module-level rustdoc — fields, JSON shape, file permissions.
6. **Security note in the rustdoc:** the file-based store is appropriate for development and demo use; production deployments should use OS keychain, HSM, or KMS-backed stores. Provide an example impl skeleton in the doc.

### Exit criteria

- [ ] Run `cargo run --example kyc_receipts` twice; both runs produce attestations from the same `agent_pubkey` for matching agent IDs
- [ ] Delete `data/identities/<agent_id>.json` between runs → next run regenerates a new keypair (the load-or-create branch)
- [ ] All existing examples (`simple_attestation`, `dispatcher_flow`, `full_pipeline`, `parallel_verifiers`) continue to work unchanged — they keep using ephemeral identities
- [ ] `make check` passes (unit tests for the store, signature round-trip across save/load, file permission verification on Unix)
- [ ] The audit viewer's "Same identity as N previous receipts" badge surfaces correctly when consecutive runs produce identical agent pubkeys
- [ ] No secret-key bytes appear in the audit viewer's UI (defence in depth; the module shouldn't expose them but the viewer doesn't render them either)

## Step 4 — SR 11-7 Compliance Pre-Mapping

**Owner:** dispatched agent on `gtm-phase-1/sr-11-7-mapping`

### Deliverables

1. New document `docs/compliance/SR_11_7.md`. Structure:
   - **Preamble:** what SR 11-7 is (US Federal Reserve guidance on model risk management), why it matters for agent-driven decisions, and AWP's claim scope (we attest to *what the agent did*, not *whether the model is correct under SR 11-7*).
   - **Scope and limits:** explicit list of what AWP's attestations help with and what they do not. Avoid overclaiming — see the warning below.
   - **Clause-by-clause mapping table:** each row is one specific SR 11-7 clause / subsection, the requirement it imposes, and which AWP feature(s) help meet it. Cite the AWP feature by file or doc reference (e.g. *"AWP `Attestation.timestamp` + `signature` covers SR 11-7 §V.A.3 'maintain a log of model use' for agent decisions"*).
   - **Sample audit narrative:** a worked example of how a Risk Officer would use AWP receipts to answer an SR 11-7 examination question — referencing the KYC demo's output for concreteness.
   - **What AWP does *not* provide:** a candid section listing SR 11-7 requirements that are out of scope (model validation, ongoing monitoring of model performance, governance committee structures, etc.).
2. New index `docs/compliance/README.md` with a one-paragraph overview and links to current and future mapping docs.
3. Update `docs/USER_JOURNEYS.md`'s "Where the prototype's gaps bite each journey" table — the "compliance pre-mapping" gap row for Sarah's adoption stage should now read "✓ SR 11-7 ([`compliance/SR_11_7.md`](compliance/SR_11_7.md)) — additional regulations TBD".

### Critical guardrail

**This document is legal-interpretation work. It must not overclaim AWP's coverage.** SR 11-7 covers model lifecycle governance broadly; AWP attests to *agent execution events*, which is one strand of one section. Any clause we cite as "covered" must be honestly defensible — overclaiming damages credibility with the exact buyers we're trying to win.

The reviewing agent (Closing the Loop step) should explicitly check for overclaims and flag any clause where the AWP feature cited is necessary-but-not-sufficient for compliance.

### Exit criteria

- [ ] `docs/compliance/SR_11_7.md` exists with all five sections populated
- [ ] Each "covered" clause cites a specific AWP type / file / behaviour
- [ ] The "What AWP does not provide" section is at least as substantial as the "covered" section
- [ ] `docs/compliance/README.md` exists as the index
- [ ] `docs/USER_JOURNEYS.md` updated; cross-link from `PHASE1_REVIEW.md` if Sarah's adoption gap row is referenced there
- [ ] Reviewer (review-gate) confirms no overclaim — every cited AWP feature is verifiable in code
- [ ] No `make check` requirement (markdown-only step) — verification is doc accuracy, not test pass/fail

## Phase exit checklist

End of Week 4:

- [ ] All four steps merged to `main`
- [ ] `cargo run --example kyc_receipts` ships the three-scenario flow with persistent identities
- [ ] `tools/audit-viewer/index.html` renders the demo's output legibly with in-browser signature verification
- [ ] `docs/compliance/SR_11_7.md` exists and has been read by someone with regulatory knowledge (human review, not just the agent)
- [ ] All existing examples still work
- [ ] `make check` clean
- [ ] `docs/USER_JOURNEYS.md` updated to reflect closed gaps
- [ ] A 3-minute demo video script can plausibly be drafted from these artefacts (this is not a deliverable for this phase, but it is the proof that GTM Phase 1 succeeded)

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Audit viewer scope creep into a full SPA | Medium | Medium | Hard rule: no build step. Vanilla JS only. |
| KYC rule resembles real KYC closely enough to confuse | Low | Medium | Demo data uses obviously-fake customer IDs and amounts; README disclaims it |
| Identity file permissions don't work on Windows | Medium | Low | Document Unix-only enforcement; degrade gracefully on Windows with a doc warning |
| SR 11-7 mapping overclaims coverage | Medium | High | Reviewer mandate + the "What AWP does not provide" section gates this |
| Step 3 breaks Phase 1-4 examples by changing constructor signatures | Low | Medium | Backwards-compat constructor explicitly required in the deliverables |

## What success looks like

A 30-minute conversation with a compliance lead at a mid-market insurer:

1. They watch a 3-minute video showing the KYC demo and audit viewer in action
2. They open the SR 11-7 mapping and recognise the regulatory shape
3. Their engineering lead confirms the prototype runs end-to-end
4. The conversation ends with "let's discuss a paid pilot"

That conversation is GTM Month 3 work. GTM Phase 1 makes it possible.
