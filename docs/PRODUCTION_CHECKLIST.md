# Attestly Production Checklist

**Scope:** what must be true before `attestly-cloud` can host a **paid Persona B
pilot**. Deliberately *not* the enterprise (Persona A) bar — no SOC2, no
multi-region, no compliance mapping. Those are Persona A / Phase 3 concerns.

---

## Who our Persona is

**Marcus — Agent Platform Operator.** He runs an agent marketplace / commerce
platform (probably Node-based) where buyers pay agents to complete tasks. His
pain is **settlement**: when a buyer disputes a $500 agent task, he has no way
to prove the work was completed, so he can't release escrow either way — he
absorbs the dispute or loses the customer.

- **What he buys:** verifiable proof-of-completion he can attach to invoices
  and settle against. Attestly receipts become the evidence that closes
  disputes.
- **Decisive moment:** *"one dispute resolved using the receipt."* His journey
  foregrounds the output (receipt → invoice → resolution), not the
  cryptography.
- **Willingness to pay:** Medium-High — $25k–$150k ACV plus potential rev share.
  Smaller buyer universe (~20–50 platforms globally) but fast to adopt.
- **What he needs that the demo doesn't cover:** stable agent identity that
  survives restarts (a settlement system can't trust keys that rotate on every
  process bounce), and hosted retention/search/sharing to justify paying.

Source: [`docs/USER_JOURNEYS.md`](../docs/USER_JOURNEYS.md) §Persona B,
[`attestly-market-research.md`](../attestly-market-research.md) §2.1.

---

## Blockers — must complete before a paid pilot

### 1. Deploy the hosted service
- [ ] `flyctl deploy` `attestly-cloud` (staging app `attestly-cloud-staging`
      already defined in [`fly.toml`](../services/attestly-cloud/fly.toml)).
- [ ] Provision managed Postgres and set `DATABASE_URL` as a Fly secret
      (`flyctl secrets set DATABASE_URL=...`) — Neon/Supabase per the README.
- [ ] Confirm the persistent blob volume mounts at `BLOB_ROOT`
      (`/var/lib/attestly-cloud/blobs`) and survives a machine restart.
- [ ] Set `ATTESTLY_CLOUD_BASE_URL` to the real host so share-link `url`s
      resolve (today it returns the placeholder `app.attestly.xyz`).
- [ ] Point real DNS + TLS at the app (Fly gives `force_https`; wire the
      custom domain).
- [ ] Set `ATTESTLY_ADMIN_KEY` to a strong secret (used for admin/seed paths).
- [ ] Smoke test against the live URL: `GET /healthz` → 200, then the full
      ingest → search → share-link flow (the local run we just did, but over
      HTTPS).

### 2. Billing (Stripe) — verified end to end
The code is wired ([`src/stripe.rs`](../services/attestly-cloud/src/stripe.rs),
account handlers, webhooks) but unverified. Configure and exercise it:
- [ ] Set `STRIPE_API_KEY`, `STRIPE_API_BASE`, `STRIPE_TEAM_PRICE_ID`,
      `STRIPE_OVERAGE_ITEM_ID`, `STRIPE_WEBHOOK_SECRET` (Fly secrets).
- [ ] Register the webhook endpoint in Stripe and confirm signature
      verification passes against `STRIPE_WEBHOOK_SECRET`.
- [ ] Drive one real Checkout → subscription-active → usage-recorded →
      overage-billed cycle in Stripe **test mode**.
- [ ] Confirm the dashboard reflects plan + usage after the webhook lands.

### 3. Persistent agent identity (Persona-B load-bearing)
A settlement system cannot use receipts whose signing keys vanish on restart.
Disk-backed identity exists in the core
([`crates/attestly-core/src/identity.rs`](../crates/attestly-core/src/identity.rs)).
- [x] **Verified (manual):** the SDK's default path
      ([`wrapper.py:120`](../python/attestly-langgraph/attestly/langgraph/wrapper.py) —
      `AgentIdentity.load_or_create('data/identities', agent_id)`) yields a
      **stable pubkey across separate processes**, and that exact pubkey is
      what lands in the cloud-stored receipt. Confirmed end to end:
      `agent-kyc-01` → `6c654550…` in two fresh interpreters and in its
      `GET /v1/attestations/{id}` payload; `agent-kyc-02` (verifier) correctly
      carries a distinct stable key.
- [ ] Add a regression test asserting identity survives a restart (guards the
      `load_or_create` default against a future swap to `generate()`). This is
      the only remaining work on this item — the substance is already proven.

### 4. Minimum production hardening
- [ ] Auth: confirm every `/v1/*` route requires a valid API key; no
      unauthenticated data leakage.
- [ ] Rate limiting on ingest + share-link creation.
- [ ] Structured logging + a basic alert on `5xx` / healthcheck failure.
- [ ] Automated Postgres backups (managed provider setting) and a documented
      restore.
- [ ] Confirm the tamper-evidence guarantee end to end on the live host: a
      mutated at-rest blob surfaces `HTTP 422 signature_invalid` and the share
      page renders the red banner.

---

## Explicitly out of scope for the Persona B pilot

Deferred by design — do **not** pull forward without an explicit decision.
Each item below records *why* it's deferred and the **trigger** that would
flip it into scope, so the deferral is a documented call rather than an
oversight.

- **SOC2 + compliance mapping docs** — Persona A gate, not Persona B.
  *Trigger:* pursuing a regulated-enterprise (Persona A) deal.
- **Multi-region / SLA** — README scopes the service as single-region
  (`iad`), no SLA, stated on the pricing page. Fine for a pilot.
  *Trigger:* a customer with an availability SLA in the contract.
- **Durable object storage (S3/R2/B2)** — only `filesystem` + `memory` blob
  backends exist today; the S3 backend the README calls a "one-line swap" is
  not yet implemented. FsBlobStore on a Fly volume is acceptable for a
  single-region pilot; *trigger:* scaling past one machine, or a durability
  requirement the single Fly volume can't meet.

### On-chain anchoring — deferred (positioning, not laziness)

The market research is emphatic: **lead with receipts, not the chain**
([`attestly-market-research.md`](../attestly-market-research.md) §4.1). On-chain
anchoring is an *optional settlement primitive*, kept below the fold. For
Persona B, Marcus's decisive moment is *"one dispute resolved using the
receipt"* — a signed receipt attached to an invoice and settled **inside his
own platform**. The Ed25519 signature *is* the settlement evidence; anchoring
adds trustless cross-org settlement, which is an expansion, not the pilot
mechanism. Pulling it forward also re-introduces the crypto-framing risk the
research warns kills the enterprise path (§2.3, Appendix A).

- **Why deferred:** the pilot's job is to prove the off-chain loop (sign →
  ship → retain → share → resolve dispute) and that someone pays for it.
  Front-loading a chain de-risks demand that isn't validated yet — the
  "crypto framing creep" failure mode (§5).
- **Trigger — becomes a blocker only if:** the specific pilot customer's
  settlement is genuinely **trustless / on-chain** (crypto-native agent
  commerce), rather than receipts resolved within their own system. If
  settlement is internal to their platform, this stays deferred with high
  confidence.

### LLM framework in `attestly-agents` — deferred (not on the product path)

`crates/attestly-agents` (Worker/Verifier/Dispatcher/Batcher) currently ships
as **plain async trait objects with no LLM wired**
([`docs/DECISIONS.md`](../docs/DECISIONS.md) D1.1, D2.2). The recommended path
is Option C — a thin custom layer on Rig — but DECISIONS.md is explicit that
the framework has *zero lines in the implementation today*, so "keep deferring"
and "adopt Rig" are indistinguishable until the day LLM-driven agents are
needed.

- **Why it doesn't gate the pilot:** `attestly-agents` is the **reference**
  orchestration, not what the Persona B product ships. Marcus wraps **his own**
  agents (his LLM, his framework) with the one-line `attest()` SDK and ships
  receipts to the cloud. He never touches the Worker/Verifier crate.
- **Trigger — becomes a blocker only if:** the product ships
  **Attestly-operated** Worker/Verifier agents as a hosted service (i.e. you
  run the verification agents for the customer) rather than the
  customer-brings-their-own-agents SDK model. Estimated cost when triggered:
  ~1–2 weeks — one `rig_client` module the existing trait impls call into
  (DECISIONS.md "What 'thin custom layer on Rig' looks like").

## Adoption-stage (after the pilot proves out)
- **TypeScript / Node SDK** — Marcus's platform is likely Node; today only the
  Python `attestly-langgraph` SDK exists. Medium-severity for
  pilot→adoption, but he can integrate against the locked HTTP contract
  ([`services/attestly-cloud/API.md`](../services/attestly-cloud/API.md))
  directly for the pilot.
