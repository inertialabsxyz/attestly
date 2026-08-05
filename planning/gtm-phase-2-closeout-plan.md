# GTM Phase 2 — Closeout Plan

**Driver:** GTM Phase 2 ([`gtm-phase-2-plan.md`](gtm-phase-2-plan.md)) — the four items below are the remaining gap between "Phase 2 built" and "Phase 2 succeeded" per that plan's own exit criteria.
**Goal:** Take the engineering work that is already merged to `main` and turn it into Phase 2's defined success condition — first paid revenue from a signed design partner, against production-grade surfaces.

## Context

The six steps of [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) split cleanly into work that is done and work that is not:

**Engineering steps 1–5 are merged to `main`** (PRs #10–#14):

| Step | Deliverable | State |
|---|---|---|
| 1. PyO3 bindings | `crates/awp-python` | merged — `awp-verify` binary, cross-language signing vector |
| 2. `awp-cloud` MVP | `services/awp-cloud` | merged — Axum + Postgres, `docker-compose`, `fly.toml` |
| 3. LangGraph SDK | `python/awp-langgraph` | merged — `attest()`, pluggable sinks, KYC graph example |
| 4. Pricing & Stripe billing | billing, sweeper, dashboard, pricing CTAs | merged |
| 5. Quickstart, docs, telemetry | mkdocs-material site, telemetry module, signup | merged |

**Step 6 and the production cutover are not done.** This document pulls the four items that block Phase 2's exit checklist into one plan so they can be tracked and sequenced explicitly, rather than living as scattered "not for the dispatched agent" footnotes in [`gtm-phase-2-agent-prompts.md`](gtm-phase-2-agent-prompts.md) §"Scope notes for the human".

These four items were always reserved for the human / founder — they are founder-led sales, production operations, and a small engineering task gated on a real partner. The agent-dispatched build is finished; what remains is the go-to-market work the build existed to enable.

## The four closeout items

1. **Design Partner #1 — founder-led close.** Sign the first paid pilot. Phase 2 fails its own definition of success without this.
2. **LangSmith metadata integration.** ~2 days of engineering, gated to ship alongside the partner so the joint-blog framing references live functionality.
3. **Production cutover.** Promote `awp-langgraph` and `awp-core-py` to real PyPI, point `awp-cloud` at its production URL with production secrets, and move Stripe to live mode.
4. **GTM dashboard.** Track SDK installs, attestations/day across all sinks, paid signups, and ARR.

## Sequencing

```
Parallel from day 1:
  Item 1 — Design Partner #1 close ........... founder-led, ~8 weeks outbound
  Item 4 — GTM dashboard ..................... agent-dispatched, ~1 week

Gated on Item 1 reaching a signed LOI / verbal commit:
  Item 3 — Production cutover ................ operator task, ~3-5 days
  Item 2 — LangSmith integration ............. agent-dispatched, ~2 days

Phase 2 exit: Item 1 closed + first invoice paid + Items 2-4 shipped.
```

**Sequencing rationale:**

- **Item 1 is the long pole** and starts immediately — founder outbound runs the full window. Everything else is sized in days.
- **Item 4 is independent** and can start now: a GTM dashboard does not need a signed partner, and having it live makes Item 1's outbound sharper (the telemetry from Step 5 already produces the raw signal — see [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) §"Step 5" conversion telemetry).
- **Items 2 and 3 gate on Item 1's pipeline maturing.** Per [`gtm-phase-2-agent-prompts.md`](gtm-phase-2-agent-prompts.md) §"Scope notes for the human", the real-PyPI promote and the production cutover are "a human decision … tied to the design-partner close" — don't burn the production-publish and live-Stripe one-way doors before there is a partner to serve. LangSmith integration ships here too, so it is testable against the partner's actual LangSmith workspace per Step 6's exit criteria.
- If no signed LOI exists by the planned week-6 checkpoint, apply [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) §"Risks" — narrow the ICP and drop to a discounted $15k / 3-month pilot.

## Item 1 — Design Partner #1 Close

**Owner:** Founder. Not agent-dispatched.

Carries forward the founder-led portion of [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) "Step 6". Nothing in the spec changes — restated here so it is tracked on the closeout checklist rather than buried in a step that is half-engineering.

### Deliverables

1. **Signed Design Partner #1** matching the Step 6 target profile: mid-market regulated buyer (fintech / insurtech / healthtech / legaltech), already running LangGraph in production or staging, with an active compliance or audit pain point, and both an engineering lead and a compliance lead reachable.
2. **Contract:** 6-month pilot, $25–50k total, in exchange for a weekly engineering call, a co-authored case study, a named website reference, and intros to at least two peers. Cap at 3 design partners total; tier 1 gets the deepest discount and the case-study lead slot.
3. **Co-authored deployment-specific SR 11-7 mapping** — extend `docs/compliance/SR_11_7.md` (GTM Phase 1 Step 4) with the partner's specific decision points and attestation schema.
4. **First case-study outline** — bullet outline of the partner's pain, integration path, and target outcome. Gates on contract signing; the full write-up gates on 60+ days of pilot use.

### Exit criteria

- [ ] Signed contract with Design Partner #1
- [ ] First paid invoice issued and paid
- [ ] Case-study outline drafted and approved by the partner
- [ ] Weekly engineering call cadence established with the partner's lead engineer

## Item 2 — LangSmith Metadata Integration

**Owner:** Agent-dispatched on `gtm-phase-2/langsmith-integration`. Gated on Item 1 pipeline maturity so it can be tested against the partner's LangSmith workspace.

The full prompt already exists at [`gtm-phase-2-agent-prompts.md`](gtm-phase-2-agent-prompts.md) §"Step 6 — GTM Phase 2: LangSmith Metadata Integration" — dispatch that verbatim. Summary of scope:

### Deliverables

1. **`langsmith_callback` option** on `attest()` in `python/awp-langgraph/` — detects an attached LangSmith tracer and, per node execution, writes `awp_attestation_id` and `awp_attestation_url` into the LangSmith run metadata. Degrades to a warning log if LangSmith is unavailable; never raises into the user's graph.
2. **Reverse "View in LangSmith" link** in the `awp-cloud` viewer when an attestation carries a LangSmith trace ID.
3. **Tested against a real LangSmith workspace** — `kyc_graph.py` run with `langsmith_callback=True`; credentials from env vars, never committed.
4. **Documentation** — a "LangSmith integration" section in the docs site and `python/awp-langgraph/README.md`, framed "provenance and observability in one trace."

### Exit criteria

- [ ] LangSmith integration tested end-to-end against a partner's actual LangSmith workspace (with their consent) — per Step 6's exit criteria
- [ ] Attestation IDs appear in LangSmith trace metadata; "View in LangSmith" link opens the correct trace
- [ ] Disabling the option produces no LangSmith calls; LangSmith being unavailable does not fail the graph
- [ ] `make check` passes

## Item 3 — Production Cutover

**Owner:** Operator (founder or designated). Not agent-dispatched — these are one-way doors that should be walked deliberately, not by an autonomous agent. Gated on Item 1 reaching a signed LOI or firm verbal commit.

Today's surfaces are all staging-grade by design: wheels publish to TestPyPI only, `fly.toml` targets `awp-cloud-staging` in `iad`, and Stripe runs in test mode. The cutover promotes each to production.

### Deliverables

1. **PyPI promote.**
   - Publish `awp-core-py` (the PyO3 bindings) to real PyPI. The wheel matrix in `.github/workflows/python-wheels.yml` already builds every platform; the publish step is gated behind `workflow_dispatch` — flip it to production PyPI and run it on a release tag.
   - Publish `awp-langgraph` to real PyPI so `pip install awp-langgraph` works without `--index-url`.
   - Verify the SDK's dependency on `awp-core-py` resolves from real PyPI (not TestPyPI) on macOS, Linux, and Windows.
2. **`awp-cloud` production deploy.**
   - Provision production Postgres (managed Neon / Supabase) and production blob storage; set `DATABASE_URL` and blob secrets as Fly secrets.
   - Add a production Fly app (or promote `awp-cloud-staging`) and point the real `app.awp-cloud.xyz` / `api.awp-cloud.xyz` / `docs.awp-cloud.xyz` / `telemetry.awp-cloud.xyz` DNS at it.
   - Confirm the docs site and quickstart resolve at their production hostnames.
3. **Stripe live mode.**
   - Rotate the Stripe key from test to live in the production `awp-cloud` config.
   - Re-create the Team-tier product, price, and metered-usage SKU in Stripe live mode; verify the webhook signing secret is the live one.
   - Run one real low-value charge end-to-end (the discounted design-partner invoice is a good candidate) to confirm Checkout, the customer portal, and metered billing all work against live Stripe.
4. **Cutover runbook** — record the DNS, secrets, and Stripe-mode steps in `services/awp-cloud/README.md` (or a `docs/DEPLOY.md`) so the production environment is reproducible.

### Exit criteria

- [ ] `pip install awp-langgraph` works from real PyPI on macOS, Linux, Windows — no `--index-url` flag
- [ ] `awp-cloud` is live at its production URL with at least 3 internal-test accounts and the design-partner account
- [ ] Production health check green; docs site and quickstart resolve at production hostnames
- [ ] Stripe processes a real charge (Team tier and the discounted design-partner Enterprise charge)
- [ ] First paid revenue collected
- [ ] Cutover runbook committed

## Item 4 — GTM Dashboard

**Owner:** Agent-dispatched on `gtm-phase-2/gtm-dashboard`. Independent of the other items — can start immediately.

The conversion telemetry from [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) Step 5 already lands daily aggregate events in the `telemetry_events` table, and the billing tables track paid accounts. What is missing is a single view that turns those rows into the GTM-team metrics named in the Phase 2 exit checklist.

### Deliverables

1. **A GTM dashboard** — an admin-gated route in the `awp-cloud` web app (reuse the existing admin-key gate from the telemetry admin route). Surfaces:
   - **SDK installs** — distinct `install_id`s seen, with new-installs-per-week trend.
   - **Attestations / day across all sinks** — `FileSink` vs `CloudSink` split (the `FileSink` count is the conversion-funnel top; `CloudSink` is paid-adjacent).
   - **Paid signups** — Team-tier accounts created, derived from the billing tables.
   - **ARR** — sum of active Team-tier subscriptions plus any Enterprise / design-partner contract value entered manually (no Enterprise self-serve, so a small manual-entry field is acceptable).
2. **Conversion-ready signal** — surface the >10k attestations/day on `FileSink` threshold from the Step 5 plan as a flagged list of `install_id`s for outbound, feeding Item 1.
3. **Documentation** — a short section in `services/awp-cloud/README.md` explaining the dashboard route, the admin gate, and the manual ARR-entry field.

### Exit criteria

- [ ] Dashboard route renders the four metrics from real telemetry and billing data
- [ ] `FileSink` / `CloudSink` attestation split is visible
- [ ] Conversion-ready `install_id` list surfaces accounts above the >10k/day threshold
- [ ] Admin-key gate prevents unauthenticated access
- [ ] `make check` passes

## Phase exit checklist

This closes out [`gtm-phase-2-plan.md`](gtm-phase-2-plan.md) §"Phase exit checklist". The engineering-step rows there are already satisfied; the rows below are what this document tracks:

- [ ] **Item 1** — Design Partner #1 signed; first paid invoice issued and paid
- [ ] **Item 2** — LangSmith integration shippable and tested against a real workspace
- [ ] **Item 3** — `pip install awp-langgraph` works from real PyPI; `awp-cloud` live at its production URL; Stripe processing real charges; first paid revenue collected
- [ ] **Item 4** — GTM dashboard live, tracking SDK installs, attestations/day, paid signups, ARR; conversion telemetry surfacing ≥5 candidate OSS users above the threshold
- [ ] Public pricing page live (already shipped in Step 4 — verify still live post-cutover)
- [ ] Documentation site live and complete (already shipped in Step 5 — verify resolves at the production hostname)

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| No design partner closes in the window | Medium | High | Founder spends ≥50% of time on outbound; if no signed LOI by week 6, narrow ICP and drop to a discounted $15k / 3-month pilot per `gtm-phase-2-plan.md` §Risks |
| Production cutover walked before a partner exists | Medium | Medium | Item 3 is explicitly gated on Item 1's signed LOI; real-PyPI publish and live-Stripe are one-way doors — do not pre-empt them |
| Real-PyPI name `awp-langgraph` / `awp-core-py` already taken or squatted | Low | Medium | Check name availability on PyPI early (before the cutover, during Item 4's window) so a rename is cheap if needed |
| Stripe live-mode config drifts from test-mode (different product/price IDs) | Medium | Medium | The cutover runbook records every live-mode ID; run one real low-value charge end-to-end before relying on it |
| GTM dashboard ARR figure is misleading (Enterprise entered manually) | Low | Low | Label the manual-entry field clearly; the dashboard is an internal tool, not investor reporting |
| LangSmith integration cannot be tested against the partner's workspace in time | Medium | Low | Step 6's prompt already supports a free-tier test workspace as a fallback; partner-workspace testing is preferred but not blocking for the integration itself |

## What success looks like

The Phase 2 "What success looks like" conversation — the Series B insurtech Head of Engineering signing a pilot order form — has happened. The closeout state:

1. Design Partner #1's contract is signed and the first invoice is paid — real ARR on the books.
2. `pip install awp-langgraph` works for anyone, from real PyPI, and `awp-cloud` serves the partner from a production URL.
3. The LangSmith integration is live, so the partner sees AWP as additive to their existing observability stack.
4. The GTM dashboard shows the funnel — installs in, attestations flowing, the first paid logo — and surfaces the next outbound targets.

That state is the precondition for GTM Phase 3 (SOC 2 Type I, second-framework SDK, the §3.3 SOM outbound motion). Phase 2 is not "done" until this document's checklist is.
