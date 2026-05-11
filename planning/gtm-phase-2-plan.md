# GTM Phase 2 — SDK Wedge & Paid Conversion Plan

**Duration:** 8 weeks
**Driver:** Six-month GTM plan in [`../awp-market-research.md`](../awp-market-research.md), Months 3–5 (design partner hunt → ecosystem wedge → sales repeatability).
**Goal:** Convert the OSS credibility built in GTM Phase 1 into a paying design partner by shipping the LangGraph SDK (the adoption wedge) and `awp-cloud` MVP (the paid surface), with public pricing and a free-to-paid conversion path that survives a procurement conversation at a mid-market regulated buyer.

## Context

GTM Phase 1 ([`gtm-phase-1-plan.md`](gtm-phase-1-plan.md)) makes the prototype demo-able to Sarah (compliance lead) and unblocks Marcus (platform operator). What it does **not** do is give either persona a way to *try AWP on their own workload*, which is the gating moment between "watched the demo" and "signed a paid pilot."

The driving insight for this phase: **the OSS SDK is the marketing budget.** Every team that installs `awp-langgraph` is top-of-funnel; conversion to paid happens at specific lifecycle moments (first auditor conversation, first multi-team deployment, first compliance deadline). Without the SDK there is no funnel — the §5 outbound motion in the market research has nothing to sequence against. With it, the §3.3 SOM math becomes operational: outbound to accounts who already produced >100k attestations against their own LangGraph agents last month.

The monetization architecture is **open-source-core, paid-hosted**, modelled on LangSmith / Helicone / Braintrust / Sentry / Posthog. The OSS SDK must be fully functional offline so that procurement-conservative buyers can adopt without a SaaS dependency on day one. The hosted service (`awp-cloud`) earns its keep on retention, sharing, and compliance — the things a team will not build themselves.

## Scope

In scope for this phase:

1. **LangGraph SDK v0.1** — `pip install awp-langgraph`. Wraps a `StateGraph`; every node execution emits a signed attestation to a configurable sink. PyO3 bindings to the existing Rust core (no Python rewrite of signing).
2. **`awp-cloud` MVP** — hosted ingest endpoint, hosted viewer, share-links, search by agent/customer/time, one-year retention. Single region, single AZ. Stripe billing for the Team tier.
3. **Public pricing page** — three tiers (OSS, Team, Enterprise). Self-serve sign-up for Team tier; "talk to us" for Enterprise.
4. **Design partner #1 contract** — first paid pilot ($25–50k for 6 months, discounted in exchange for co-authored case study and reference rights). Founder-led close.
5. **LangSmith metadata integration** — optional one-line config that emits AWP attestation IDs into LangSmith traces, so existing LangSmith users see AWP as additive rather than competitive.
6. **Conversion telemetry** — anonymous SDK usage metrics (attestation volume, sink type) so we can identify which OSS users are conversion-ready before reaching out.

Explicitly **out of scope** for this phase:

- **SOC 2 audit.** Pursued in GTM Phase 3 once a paying pilot exists to justify the spend. Type I report is a Month 4–6 deliverable, not Month 3.
- **Merkle anchoring / on-chain.** Per GTM §4.1 lock #1, still a footnote. Becomes an *enterprise feature* in Phase 3, not a Phase 2 deliverable.
- **CrewAI / AutoGen / Mastra SDKs.** Second-framework expansion is Phase 3. LangGraph wins on regulated-buyer concentration; ship one well before splitting effort.
- **Multi-region `awp-cloud`.** Single region (US-East) only. EU residency is a Phase 3 enterprise concern.
- **`TaskExecution` coordination-type generalisation** and **storage consolidation** — both still deferred from Phase 1. Revisit in Phase 3 if Phase 2 surfaces real pressure on either.
- **TypeScript SDK.** Same pattern as Python (thin TS package + WASM bindings to Rust core) is preserved for Phase 3+; not built here.

## Sequencing

```
Week 1-2 (parallel):  Step 1 — PyO3 Bindings        Step 2 — awp-cloud MVP scaffold
                              │                            │
                              └────────────┬───────────────┘
                                           │
Week 3-4 (parallel):  Step 3 — LangGraph SDK v0.1   Step 4 — Pricing & billing
                              │                            │
                              └────────────┬───────────────┘
                                      merge to main
                                           │
Week 5-6:             Step 5 — Quickstart, docs, conversion telemetry
                                           │
Week 7-8:             Step 6 — Design Partner #1 close + LangSmith integration
                                           │
                                      phase exit
```

**Sequencing rules:**

- Step 1 (PyO3 bindings) and Step 2 (`awp-cloud` scaffold) are independent — one is Rust + Python, the other is a hosted service. Run in parallel worktrees.
- Step 3 (LangGraph SDK) depends on Step 1 — the SDK uses the bindings for signing. Cannot start until Step 1 lands on `main`.
- Step 4 (pricing + Stripe billing) depends on Step 2 — needs the hosted service to attach billing to.
- Step 5 is integration polish: quickstart that produces a paid-tier signup link, anonymous telemetry, documentation site.
- Step 6 is founder-led sales. The first paid pilot has to close before phase exit or the phase has failed.

## Step 1 — PyO3 Bindings

**Owner:** dispatched agent on `gtm-phase-2/pyo3-bindings`

### Deliverables

1. New crate `crates/awp-python` — PyO3 bindings exposing the AWP signing path to Python. ~200 lines of Rust.
2. Python-side API surface:
   - `awp.sign_attestation(payload: dict, identity: AgentIdentity) -> Attestation`
   - `awp.verify_attestation(attestation: Attestation, pubkey: bytes) -> bool`
   - `awp.AgentIdentity.load_or_create(path: str, agent_id: str) -> AgentIdentity`
   - `awp.Attestation` — dataclass-shaped Python object backed by Rust struct
3. **Byte-identical signatures across Rust and Python.** The canonical-JSON serialization in Python must produce the same bytes as the Rust `serde_json` canonical encoding. Ship a cross-language self-test that takes a fixed payload, signs in Rust, verifies in Python (and vice versa), and asserts bit-equality of the signing-payload bytes.
4. `maturin`-based build with a CI matrix producing wheels for: macOS (x86, arm64), Linux (x86, arm64 manylinux), Windows x86_64; Python 3.9 through 3.13. Use `maturin-action` for GitHub Actions.
5. Published to TestPyPI under `awp-core-py` (real PyPI publish gated behind the design-partner close in Step 6).
6. README at `crates/awp-python/README.md` documenting install, usage, and the byte-identical-signature guarantee with example.

### Exit criteria

- [ ] `pip install --index-url https://test.pypi.org/simple/ awp-core-py` succeeds on macOS, Linux, Windows
- [ ] Cross-language signing self-test: sign in Rust, verify in Python — passes
- [ ] Cross-language signing self-test: sign in Python, verify in Rust — passes
- [ ] CI matrix produces all wheel artefacts on tag push
- [ ] No regression in `make check` for the Rust workspace
- [ ] README documents the FFI boundary and which Python types map to which Rust types

## Step 2 — `awp-cloud` MVP Scaffold

**Owner:** dispatched agent on `gtm-phase-2/awp-cloud-scaffold`

### Deliverables

1. New directory `services/awp-cloud/` containing the hosted-service codebase. Stack: Rust (Axum) for the API, Postgres for storage, S3 (or compatible) for attestation blob storage, deployed as a single container.
2. HTTP API surface:
   - `POST /v1/attestations` — accept signed attestations, validate signature server-side, store. Authenticated via API key.
   - `GET /v1/attestations?agent_id=&customer_id=&from=&to=` — paginated search.
   - `GET /v1/attestations/{id}` — fetch a single attestation, public if marked shareable.
   - `POST /v1/share-links` — generate a tokenised public share URL for a single attestation or filtered set.
3. **Server never sees private keys.** The API validates the signature but stores only the signed payload + signature. Re-verification happens against the agent's public key, which is part of the attestation.
4. **Tamper-evident retention.** Stored attestations are content-addressed by SHA-256. Any retrieved attestation is re-verified server-side before being returned; a verification failure surfaces as an HTTP 422 with the failure reason.
5. Web viewer at `https://app.awp-cloud.xyz` (placeholder domain) — same UX as the static viewer (Phase 1 Step 1) but server-backed: search, share-link generation, account dashboard. Reuse the existing static viewer's JS verification logic.
6. Local dev: `docker compose up` brings the whole stack up against a local Postgres + Minio (S3-compatible).
7. Deployment target: a single managed Postgres + a single container on Fly.io or Railway. Not designed for HA at this stage.

### Exit criteria

- [ ] `docker compose up` produces a working local instance
- [ ] An attestation produced by the LangGraph SDK can be POSTed and retrieved
- [ ] Search returns expected paginated results across 10k seeded attestations
- [ ] Share-links work in incognito (token-gated, no auth required)
- [ ] Tampered attestation (bytes edited at rest in S3) surfaces as HTTP 422 on fetch
- [ ] Staging deployment live at a real (non-prod) URL
- [ ] README documents the deployment model, the key-handling story, and the limitations (single region, no SLA)

## Step 3 — LangGraph SDK v0.1

**Owner:** dispatched agent on `gtm-phase-2/langgraph-sdk`

### Deliverables

1. New Python package `python/awp-langgraph/` published as `awp-langgraph` on TestPyPI.
2. Primary API:
   ```python
   from awp.langgraph import attest

   graph = build_my_graph()                       # user's existing StateGraph
   graph = attest(graph, agent_id="my-agent-01")  # wraps with attestation hooks
   ```
3. **One-line integration.** Wrapping a graph attaches a LangGraph callback that, on every node completion, signs the `{input_hash, output_hash, agent_id, timestamp}` payload and writes to the configured sink.
4. Sinks (pluggable, `Sink` protocol):
   - `FileSink(path: str)` — append JSONL to a local file. Default if no sink configured.
   - `CloudSink(api_key: str, endpoint: str = "...")` — POST to `awp-cloud`. Stripe billing hooks on the cloud side count attestations.
   - `CallableSink(fn: Callable)` — escape hatch for custom destinations.
5. **Dual-agent mode (optional):** `attest(graph, agent_id=..., verifier_agent_id=...)` runs the graph twice with different identities, emits both Worker and Verifier attestations, and reports the verdict. Off by default (single-agent attestation is the common path); enabled by users who want the full Worker/Verifier protocol.
6. **Identity handling:** Reads `AgentIdentity` from the persistent store landed in GTM Phase 1 Step 3 by default (`./data/identities/<agent_id>.json`). Override via explicit `identity=` kwarg.
7. **No LLM dependency.** The SDK does not call an LLM itself; it observes whatever the LangGraph node already produces. This preserves the GTM Phase 1 guardrail.
8. Example: `python/awp-langgraph/examples/kyc_graph.py` — a LangGraph version of the KYC demo from GTM Phase 1 Step 2, demonstrating the one-line wrap and end-to-end attestation flow.
9. Documentation at `python/awp-langgraph/README.md`: install, quickstart, sink configuration, identity management, dual-agent mode, troubleshooting.

### Exit criteria

- [ ] `pip install awp-langgraph` installs cleanly on macOS, Linux, Windows
- [ ] Example `kyc_graph.py` runs end-to-end and writes valid attestations to `./data/attestations.jsonl`
- [ ] Attestations from the Python SDK verify against the existing static audit viewer (Phase 1 Step 1) — byte-identical to Rust-produced attestations
- [ ] `CloudSink` successfully POSTs to a local `awp-cloud` instance and the attestation appears in search results
- [ ] Dual-agent mode produces two attestations (Worker + Verifier) with the Verifier's verdict embedded
- [ ] Wrapping does not change the LangGraph node's behaviour — same outputs, same routing, same errors propagate normally
- [ ] Performance overhead documented: signing adds <10ms per node on a modern laptop (Ed25519 is cheap, but measure)

## Step 4 — Pricing, Billing, and Public Pricing Page

**Owner:** dispatched agent on `gtm-phase-2/pricing-billing`

### Deliverables

1. **Public pricing page** at `tools/landing-page/pricing.html` (or new section on the main page — see landing-page update below). Three tiers:
   - **OSS / Free** — $0. Local-first, file-based, you own everything.
   - **Team** — $499 / month. 1M attestations / month, 1-year retention, hosted viewer, email support.
   - **Enterprise** — *Talk to us.* Unlimited attestations, 7-year retention, SSO, SOC 2 report (Phase 3), compliance templates, dedicated SE, MSA + DPA. Indicative range $50k–$150k ACV — not on the public page; reserved for sales conversations.
2. **Stripe integration** in `awp-cloud`:
   - Stripe Checkout for Team tier self-serve sign-up.
   - Metered billing: attestation count beyond the 1M floor surcharged at $0.10 per additional 1k attestations.
   - Customer portal for plan changes and invoices.
3. **Account model**: an `awp-cloud` account holds one or more API keys. Each API key is scoped to a project. Attestation usage is aggregated per account for billing.
4. **Account-level retention policy**: Team accounts get 1-year retention enforced by a background sweeper; Enterprise accounts get 7-year. OSS users (no account) never hit the cloud at all.
5. **"Free to leave" guarantee on the pricing page** — explicit statement that cancelling the paid tier does not invalidate existing receipts; users can export everything as JSONL.
6. **Export endpoint**: `GET /v1/export` streams all attestations for an account as JSONL. Used by the free-to-leave guarantee and as the answer to "what happens to our data if we cancel."

### Exit criteria

- [ ] Pricing page renders cleanly and matches the landing page's visual language
- [ ] Stripe Checkout flow signs up a new Team account, issues an API key, and accepts a test card
- [ ] Metered billing emits a usage record to Stripe at end-of-month for a synthetic account exceeding the floor
- [ ] Customer portal accessible from the account dashboard
- [ ] `GET /v1/export` returns valid JSONL that the static audit viewer can re-render and re-verify
- [ ] Sweeper job correctly retains 1-year-old attestations on a Team account and deletes anything older

## Step 5 — Quickstart, Docs, and Conversion Telemetry

**Owner:** dispatched agent on `gtm-phase-2/quickstart-docs`

### Deliverables

1. **Quickstart at `https://awp-cloud.xyz/quickstart`** — 60-second flow:
   - Sign up (email + password, optional OAuth)
   - `pip install awp-langgraph`
   - Copy-paste a 5-line snippet wrapping a tiny LangGraph example
   - First attestation appears in the dashboard within seconds
2. **Documentation site** at `docs.awp-cloud.xyz`. Sections:
   - **Quickstart** — the 60-second path
   - **Concepts** — what's an attestation, what's a verifier, what's a sink
   - **LangGraph integration** — full SDK reference
   - **Self-hosted** — OSS-only path (file sinks, static viewer, no cloud)
   - **Compliance** — pointers to the SR 11-7 mapping from GTM Phase 1 Step 4
   - **Migration** — moving from `FileSink` to `CloudSink` without losing attestations
3. **Anonymous SDK telemetry** (opt-in by default, off-by-config):
   - Emits a daily aggregate count: `{install_id, sdk_version, attestations_emitted_today, sink_type}`
   - No payload data, no agent IDs, no user identifiers beyond a randomly-generated install ID
   - Documented prominently in the SDK README with an opt-out instruction (`AWP_TELEMETRY=0`)
   - Used to identify conversion-ready OSS users for outbound (>10k attestations/day on `FileSink` is a strong signal)
4. **Landing page integration** — the main landing page's "Try it on your LangGraph agent" CTA links to the quickstart.
5. **README at root** updated to lead with the Python SDK install, demote the Rust prototype examples to "Reference implementation."

### Exit criteria

- [ ] Quickstart can be completed end-to-end by a new user in under 5 minutes
- [ ] Docs site is live with all six sections populated and at least one example per section
- [ ] Telemetry produces aggregate stats visible in a dashboard for the AWP team
- [ ] Opt-out works: `AWP_TELEMETRY=0` produces no network calls (verified by mitmproxy or equivalent)
- [ ] Quickstart's first-attestation moment ("here's your first signed receipt") works reliably

## Step 6 — Design Partner #1 + LangSmith Integration

**Owner:** Founder-led, not a dispatched agent. Engineering support for the LangSmith integration only.

### Deliverables

1. **Design Partner #1 signed.** Target profile:
   - Mid-market regulated buyer (fintech, insurtech, healthtech, legaltech)
   - Already running LangGraph in production or staging
   - Has an active compliance or audit pain point (regulator inquiry, SR 11-7 examination, internal model risk review)
   - Engineering lead reachable; compliance lead reachable
2. **Contract terms:**
   - 6-month pilot, $25–50k total (discounted from indicative Enterprise range)
   - In exchange: weekly engineering call, co-authored case study, named reference on the website, intro to at least two peers
   - Cap at 3 design partners total; tier 1 (first to close) gets the deepest discount and the case-study lead position
3. **Co-authored SR 11-7 deployment-specific mapping** — extend the generic SR 11-7 doc from Phase 1 Step 4 with the partner's specific decision points and attestation schema.
4. **LangSmith metadata integration** (engineering deliverable, ~2 days):
   - SDK option: `attest(graph, langsmith_callback=True)` injects attestation IDs into LangSmith trace metadata
   - Each LangGraph node's LangSmith trace gets an `awp_attestation_id` field pointing to the attestation
   - The `awp-cloud` viewer can render a "View in LangSmith" link if the trace ID is present
   - Joint blog post pitch to LangChain ("AWP + LangSmith: provenance and observability in one trace") — sent in Phase 3 Month 5 but the integration ships here so the post can reference live functionality
5. **First case study draft** — bullet outline of the design partner's pain, their integration path, and the measured outcome. Final write-up gates on 60+ days of pilot use; the outline gates on contract signing.

### Exit criteria

- [ ] Signed contract with Design Partner #1 in place
- [ ] First paid invoice issued and paid
- [ ] LangSmith integration tested end-to-end against a partner's actual LangSmith workspace (with their consent)
- [ ] Case study outline drafted and approved by the partner
- [ ] Weekly engineering call cadence established with the partner's lead engineer

## Phase exit checklist

End of Week 8:

- [ ] All six steps merged to `main` (or shipped, in the case of the hosted service)
- [ ] `pip install awp-langgraph` works against the real PyPI (not TestPyPI)
- [ ] `awp-cloud` is live at its production URL with at least 3 internal-test accounts and the design-partner account
- [ ] Public pricing page live on the landing page
- [ ] Stripe billing processing real charges (Team tier and the discounted design-partner Enterprise charge)
- [ ] First paid revenue collected
- [ ] LangSmith integration shippable
- [ ] Conversion telemetry surfacing at least 5 candidate OSS users with >10k attestations/day
- [ ] Documentation site live and complete
- [ ] Updated GTM dashboard tracking: SDK installs, attestations / day across all sinks, paid signups, ARR

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| PyO3 wheel-build matrix breaks on a Python or OS version | High | Medium | Use the well-trodden `maturin-action` GitHub Actions setup; budget 2 days for matrix debugging |
| Canonical-JSON encoding drift between Rust and Python | Medium | High | Cross-language byte-equality self-test is non-negotiable in Step 1's exit criteria; without it, signatures will silently fail across languages in production |
| `awp-cloud` scope expands into a full multi-region service | High | Medium | Hard rule: single region, single AZ, no SLA promise on the public pricing page. Multi-region is Phase 3+. |
| No design partner closes in 8 weeks | Medium | High | Founder spends ≥50% of phase time on outbound; if no signed LOI by week 6, narrow ICP and reduce to discounted pilot at $15k for 3 months. Acknowledged compression on case-study leverage. |
| LangGraph upstream changes break the wrapper | Low | Medium | Pin to a specific LangGraph minor version in the SDK; document upgrade path; ship a CI workflow that tests against the latest LangGraph release weekly |
| SDK telemetry feels invasive and damages OSS trust | Medium | Medium | Opt-out documented prominently; aggregate-only; no payload data; consider opt-IN if community signal turns negative within 30 days of launch |
| Stripe billing edge cases (failed payments, mid-cycle plan changes) eat engineering time | Medium | Low | Use Stripe Billing's hosted customer portal; don't build a custom billing UI in v1 |

## What success looks like

A 90-minute conversation with the Head of Engineering at a Series B insurtech:

1. They opened the docs site three days ago after a Hacker News post about the LangGraph SDK
2. Their engineering team installed `awp-langgraph` on a staging branch the next day; 4,200 attestations produced overnight on `FileSink`
3. The Head of Engineering shares the static viewer output with their compliance lead, who asks "can our auditor see this without a VPN?"
4. Today's call covers: pricing (Team tier, likely Enterprise within 90 days), SR 11-7 mapping (they're prepping for an examination in Q4), and the design-partner case study slot (interested if discount is meaningful)
5. The conversation ends with a signed pilot order form and a kickoff scheduled for next week

That conversation produces the first $25–50k of ARR and is the proof that GTM Phase 2 succeeded. Everything in this plan is in service of that conversation happening before the end of Week 8.

## Revenue trajectory implied by this plan

If Phase 2 closes Design Partner #1 on time and Phase 3 hits SOC 2 Type I by Month 6:

| Month | Milestone | ARR |
|---|---|---|
| 3–4 | Design Partner #1 signed ($25–50k for 6 months, prorated) | ~$50k |
| 6 | 2–3 paid pilots in flight, no logo customers yet | ~$100–150k bookings |
| 9 | First enterprise close at $100k+ ACV; SOC 2 Type I in hand | ~$250k |
| 12 | 4–6 customers, mixed Team and Enterprise | $300–500k ARR |
| 18–24 | Per §3.3 SOM math | $1–2M ARR |

The thing the previous plan iterations did not name explicitly: **the OSS SDK is the marketing budget.** Every team that installs `awp-langgraph` is top-of-funnel; conversion happens at the lifecycle moments above (first auditor conversation, first multi-team deployment, first compliance deadline, first cross-org dispute, first scale wall). Without the SDK there is no funnel; with it, the §5 outbound motion in the market research becomes "outbound to accounts already producing >10k attestations / day against their LangGraph agents."
