# Attestly — Landing Page (thinking draft)

A founder-facing landing page. Not for buyers. Not for the eventual public marketing site. The purpose of this document is to force one coherent pitch into ~600 words so the gaps in conviction become visible.

If a section reads as evasive or generic, the underlying strategy is thin and the section is where the work is.

---

## Cryptographic receipts for AI agents.

A signed, verifiable record of every task your agents complete — for audit, billing, and dispute resolution.

## The problem

Your AI agent just made a decision that affects a customer.

Six months from now, the regulator, the customer, or your own internal audit team asks: *what exactly did the agent do, and how do you know the record hasn't been edited?*

You can show them application logs. Those logs say what your *server* thinks the agent did. They don't prove the agent did it. They were written by the same system that's now telling you they're correct, and they could have been changed yesterday by anyone with write access to the database.

This is fine until it isn't. The moment an examiner, a disputant, or a counterparty asks for proof — not records, *proof* — application logs become exhibits of your honour system, not your controls.

## What Attestly gives you

Three things, none of them speculative:

1. **A signed receipt for every agent decision.** At the moment the agent produces an output, it signs an attestation binding the agent identity, the task input, the output, the timestamp, and a hash of the result. The signature is ed25519 over a canonical payload — tampering with any field after signing invalidates it.

2. **Independent verification by a second agent.** A separate Verifier agent re-runs the task, checks the Worker's signature, and signs its own attestation recording its verdict. Disagreement is recorded, not hidden. The Verifier's signature is also ed25519, on its own keypair. You now have two independent cryptographic claims about one decision.

3. **Audit-ready storage.** Receipts batch into Merkle trees. Anyone given just a root hash, an attestation, and an inclusion proof can verify the receipt was part of the batch — without seeing the rest of the batch. The audit viewer renders this as a chronological timeline that re-verifies every signature in the browser. No trust required; no network call.

## Why you'd pay

Three specific outcomes, one per buyer:

- **Compliance officer at a regulated firm:** the difference between a clean SR 11-7 examination and a finding. *"Show me what your agent did and prove it"* moves from a problem to a one-click answer. The pay-for is the six-figure remediation cost of a finding that didn't happen.

- **Platform operator running agent commerce:** the resolved dispute. When a buyer says *"the agent didn't do what I paid for"*, the receipt is the evidence. Today you absorb the dispute or lose the customer. With Attestly, the receipt closes it. The pay-for is a percentage of disputed revenue that previously evaporated.

- **Counterparty in a cross-party agent workflow:** the answer to *"I don't trust your logs."* Attestly attestations are signed by the agent, not by your platform — so a third party can verify them without trusting you. The pay-for is the deals that previously stalled at procurement because *"we don't know what their agent actually did."*

## What it looks like

A working KYC scenario, three minutes:

```
Customer #4711 — Decision: APPROVE     signature ✓  verdict ✓
Customer #8842 — Decision: FLAG        signature ✓  verdict ✓
Customer #9999 — Decision: FLAG        signature ✗ — receipt rejected
```

The third row is a tampered receipt. The Verifier caught it. If application logs were the only record, customer #9999's FLAG could have been quietly changed to APPROVE and no one would know. Attestly makes that class of edit impossible to hide.

Open the audit viewer in a browser, drag the receipt files in, and every signature is re-verified locally. The viewer is 800 lines of static HTML. There is no server.

## What Attestly is not

- **Not a replacement for your observability stack.** Datadog, LangSmith, Helicone — these answer *what happened on my infrastructure*. Attestly answers *what did the agent claim, and can I prove it*. Complementary, not competing.
- **Not a judgement on whether the agent's answer was right.** Attestly attests to what was claimed, not to whether the claim is correct under your business rules. Model validation is your job; Attestly makes the claims auditable.
- **Not on-chain AI.** Optional Merkle root anchoring exists for use cases that need it (cross-party agent networks, settlement). Most buyers will never anchor anything on a chain, and that's fine — the protocol works without one.
- **Not a token, not a DAO, not a reputation score.** No tokenomics. No governance theatre. No "stake to verify." Just signatures and hashes.

## What it costs you to integrate

Honestly: **less than you think for the receipt layer; more than you think for the production story.**

A working integration of the Worker → Verifier loop is ~100 lines of Rust today (see `examples/kyc_receipts.rs`). The cryptographic core is mature. The audit viewer is browser-native and works against the JSONL output as-is.

The integration *cost* you should expect:

- Identity management — production deployments need keys in an HSM or KMS, not on disk. The prototype's `FileIdentityStore` is dev-grade. Realistic enterprise spike: 2–3 weeks.
- Storage consolidation — the prototype writes to three places (JSONL × 2 + SQLite). Production wants one source of truth. Realistic: 1 week.
- SDK availability — the reference is Rust; most agent platforms are Python or TypeScript. Wrapping the spec for those runtimes: 2 weeks each.

The protocol itself is small and stable. The work is what's around it.

## Open questions I'm still resolving

This section is for me, not buyers. The questions below are where the strategy is genuinely uncertain.

1. **Hosted service vs. library-only.** GTM §4.1 says the hosted offering is non-negotiable for revenue. I haven't committed to building one yet, and the prototype is library-only. Until there's a paid hosted SKU, the revenue path is hypothetical.

2. **Framework choice.** `DECISIONS.md` recommends Rig over AutoAgents when LLM integration becomes the next-task. That day hasn't arrived. The decision is deferrable but the *answer* is sitting in the doc unverified — a 4-hour Rig spike would convert medium confidence to high.

3. **The "why hasn't this been built yet" question.** LangSmith, Braintrust, Anthropic, OpenAI, Stripe — any of them could ship a signed-attestation primitive in a quarter. My honest read: they haven't because the cross-framework neutrality is harder than it looks, and because the regulated-enterprise pull isn't strong enough yet for them to prioritise. Both could change. I'm betting the window stays open 6–12 months. If it doesn't, this is a feature, not a company.

4. **SR 11-7 was the easy mapping.** EU AI Act and HIPAA are harder; neither is drafted. Until they're mapped honestly (with the same overclaim discipline as `docs/compliance/SR_11_7.md`), the "regulated enterprise" pitch leans on one US-only document.

5. **The crypto-native distribution path.** Persona C is fast and free but small in dollar terms. I'm leaning on her for credibility while pretending she's not the buyer. If the enterprise pitch stalls, Persona C is the fallback — and the fallback is a much smaller business. I should be honest with myself about whether I'd be content with the Persona-C-only outcome.

6. **The demo's avoided-harm narrative.** The KYC tampered scenario shows the cryptography catches a tamper. It does *not yet* show that the tamper would otherwise have caused a customer or compliance harm. The leap from "signature invalid" to "and here's what would have happened" is the leap a buyer makes for me unless I make it explicit in the demo. Two-paragraph fix; haven't done it.

If sections 1–6 don't feel uncomfortable to read, I've sandbagged them. The point is to see where I'd defend rather than agree.
