# Attestly — Market Research & Go-to-Market Report

**Prepared:** 2026-05-06
**Subject:** Attestly — signed attestations for AI agent work, with optional on-chain anchoring
**Author perspective:** Market researcher covering web3, Ethereum, LLM/agent infra, and infrastructure for financial systems

---

## Executive Summary

| Question | Answer |
|---|---|
| Does Attestly solve a real problem now or in the next 6 months? | **Yes — but the buyer is split between two camps with different urgency profiles.** |
| Is there a TAM that supports a $1M ARR business in 24 months? | **Yes, comfortably — $1M ARR is roughly ~30–60 mid-market customers or ~3–6 platform deals.** |
| Overall market score (1–10) | **6.5 / 10** |
| Recommendation | **Go to market, but reposition.** Lead with **"agent audit & receipts"** for compliance/regulated buyers. Treat on-chain anchoring as an *optional* settlement primitive, not the headline. |

The protocol is technically sound and lands on a real, growing pain point (verifiable agent work). The risk is not the technology — it's that the strongest buyers (enterprise, regulated industries) don't care about the chain, and the buyers who care about the chain (crypto-native agent networks) are still small in dollar terms. Resolving that positioning tension is the central GTM job for the next six months.

---

## 1. Market Context (Mid-2026 Snapshot)

### 1.1 The agent infrastructure stack is consolidating

By mid-2026, the agent stack has roughly settled into recognizable layers:

- **Models:** Anthropic, OpenAI, Google, open-weights
- **Orchestration / runtime:** LangGraph, CrewAI, AutoGen, Mastra, AutoAgents (Rust), and bespoke harnesses
- **Tooling protocol:** **MCP** has won as the de-facto standard for agent ↔ tool wiring
- **Identity / access:** Auth0/Okta agent identity products, plus emerging "agent passports"
- **Eval / observability:** LangSmith, Braintrust, Arize, Helicone
- **Payments / commerce:** Stripe Agent Toolkit, Skyfire, Coinbase x402, Visa/Mastercard agent rails
- **Audit / verification:** **Sparsely populated.** This is where Attestly plays.

The "what did the agent actually do, and can I prove it" layer is real but underdeveloped. Eval tools answer *how well* an agent did. Observability answers *what happened on my server*. Neither produces a portable, third-party-verifiable receipt. That gap is Attestly's wedge.

### 1.2 Three macro forces creating demand in the next 6 months

1. **Regulation is arriving.** EU AI Act high-risk obligations are now being enforced; US sectoral rules (healthcare, financial services) increasingly require auditable AI decision logs. "Show me what the agent did" is moving from nice-to-have to mandatory in regulated verticals.
2. **Agent-to-agent commerce is starting.** Stripe, Visa, and crypto-native rails (x402, Skyfire) all shipped agent-payment products in 2025–2026. Settlement requires proof-of-work-completion. Without it, disputes are unwinnable.
3. **Trust crises.** Public incidents of agents fabricating completed work, hallucinating tool calls, or being silently swapped have created enterprise procurement requirements around "non-repudiation of agent output."

### 1.3 Competitive landscape

| Category | Players | How they relate to Attestly |
|---|---|---|
| **Agent observability** | LangSmith, Braintrust, Arize, Helicone, Langfuse | Adjacent; they log, Attestly attests. Could become Attestly's distribution channel or its competitor if they add signing. |
| **ZK ML / verifiable inference** | EZKL, Giza, Ora, Modulus Labs | Solve a *harder* problem (prove model inference). Slow, expensive, narrow. Attestly is the pragmatic 80% solution. |
| **Crypto agent frameworks** | Virtuals, Olas (formerly Autonolas), Fetch.ai, ai16z/ELIZA, Wayfinder, ChaosChain | Most have ad-hoc attestation primitives. None have shipped a clean cross-framework spec. Attestly could become the standard here. |
| **Agent identity / DIDs** | cheqd, Disco, Privado, World ID for agents | Complementary; Attestly needs identity, but doesn't need to own it. |
| **Enterprise AI audit** | Credo AI, Holistic AI, Fairly, Robust Intelligence | Policy & risk layer. Could be a *channel* for Attestly — they need underlying receipts. |
| **Verifiable compute (zkVM)** | RISC Zero, Succinct (SP1), Aleo | Attestly's optional upgrade path (per Appendix A of the prototype plan). |
| **Closest direct analog** | EigenLayer AVS attestations, Ritual, Atoma | Crypto-native, validator-set heavy, opinionated about chain. Attestly is lighter and chain-agnostic. |

**Whitespace:** A clean, framework-agnostic, MCP-complementary attestation spec with a permissive license and a reference implementation. No one credibly owns this slot today. Window is open but not infinite — expect Anthropic, LangChain, or one of the crypto agent platforms to ship something in this space within 12 months.

---

## 2. Does Attestly Solve a Real Problem?

### 2.1 The four buyer personas

| Persona | Pain | Willingness to pay | Notes |
|---|---|---|---|
| **A. Regulated enterprise (financial services, healthcare, legal)** | "We have to prove what the agent did to auditors/regulators." | **High** ($50k–$500k ACV) | Strong fit. Don't care about chain. Want receipts, retention, signing keys controlled by them. |
| **B. Agent platform / marketplace operator** | "Buyers won't pay if they can't verify completion." | **Medium-High** ($25k–$150k ACV, plus rev share) | Strong fit if they're building agent commerce. Smaller universe. |
| **C. Crypto-native agent networks** | "Need on-chain settlement primitive for agent-to-agent payments." | **Low-Medium in $, high in mindshare** | Will adopt fast and free. Useful for distribution and credibility, not direct revenue. |
| **D. Internal dev team using agents** | "Devs want to debug what their agent did." | **Low** | Already covered by LangSmith/Braintrust. Don't chase. |

### 2.2 Is the pain *acute* in the next 6 months?

- **Persona A:** Yes. EU AI Act enforcement and SR 11-7-style model-risk extensions to agents are creating procurement line items *now*.
- **Persona B:** Yes, but the buyers are few. ~20–50 platforms globally that matter.
- **Persona C:** Already pulling. They will adopt anything credible that ships.
- **Persona D:** No urgency.

**Verdict:** Real problem, multiple buyers, regulatory tailwind. The question is not "is there a problem" but "can Attestly own the category before a bigger player does."

### 2.3 Why Attestly's positioning is at risk

Reading the landing page cold, a CISO at a regulated buyer sees "blockchain," "Merkle," "EVM L2," "chain-agnostic" — and bounces. The current page reads as a crypto infrastructure project that happens to do attestations. The buyer with the budget reads it as a crypto infrastructure project they don't need.

The core asset is the **attestation spec + signing + multi-agent verification**. On-chain anchoring is a feature, not the pitch. This is the single most important repositioning lever.

---

## 3. TAM / SAM / SOM

Numbers below are directional, built bottom-up. Sources: public earnings calls, Gartner agent forecasts (2025–2026), AI infra funding databases, comparable pricing in observability space.

### 3.1 TAM (5-year horizon)

- Total enterprise AI/agent spend by 2030: $200B–$400B range (Gartner mid-case)
- Audit / governance / observability slice: historically 3–5% of platform spend → **~$8B–$20B TAM** for the audit-and-trust layer
- Verifiable agent attestations as a sub-slice: ~10–20% of that → **$1B–$4B addressable**

### 3.2 SAM (24-month addressable)

Buyers ready to spend on agent attestations specifically by 2028:
- ~3,000 mid-to-large regulated enterprises actively deploying agents in production
- ~50 agent platforms / marketplaces with revenue
- ~200 crypto-native agent projects with budget

Average ACV $30k blended → **SAM ≈ $100M–$200M** in 2028.

### 3.3 SOM (24 months from launch)

Realistic obtainable share for a focused team executing well:
- 30–60 mid-market deals at $20k–$50k ACV → **$1M–$2M ARR**
- OR 3–6 platform/anchor-customer deals at $150k–$300k → **$1M–$2M ARR**
- OR a hybrid

**$1M ARR in 24 months is achievable but not assured.** It requires (a) one anchor design partner in the first 90 days, (b) clear repositioning away from "blockchain protocol," (c) at least one regulatory tailwind event referenceable in sales conversations.

### 3.4 Market score: **6.5 / 10**

Breakdown:

| Dimension | Score | Rationale |
|---|---|---|
| Problem reality | 8 | Real pain, multiple personas, regulatory pull |
| Timing | 7 | 6–18 month window before bigger players ship something adjacent |
| TAM | 7 | Small now, large in 3–5 years |
| Defensibility | 4 → 6 with §4.2 moves | Spec-level work is copyable; moat must come from network effects, integrations, or being the *standard*. Concrete short-term levers in §4.2 |
| Distribution path | 5 → 7 with §4.3 moves | Open source / dev-tools motion is slow; enterprise sales is heavy lift. Channel partnerships in §4.3 materially shift this |
| Founder/market fit signal | 7 | Technical execution looks credible; market positioning needs work |
| Capital efficiency to $1M ARR | 7 | Achievable with a small team if focused |
| **Overall (baseline)** | **6.5** | Strong candidate to bring to market with focused repositioning |
| **Overall (with §4.2/§4.3 moves executed)** | **~7.5** | Achievable in 6 months without raising more or pivoting |

A 10 would require either (a) a clear regulatory mandate naming the spec, or (b) a major platform (Anthropic, OpenAI, Stripe) committing to integrate. Neither is in hand, but both are plausible pursuits.

---

## 4. Strategic Recommendations Before GTM

### 4.1 Positioning decisions to lock

Before spending dollars on launch, three positioning decisions need to be locked:

1. **Lead message:** "Cryptographic receipts for AI agent work" — *not* "blockchain protocol for agents." Chain anchoring is a footnote feature for buyers who ask.
2. **Primary ICP:** Regulated mid-market (fintech, insurance, healthtech) deploying agents in production. Secondary: crypto-native agent networks (for credibility and ecosystem mindshare).
3. **Product surface:** Ship a hosted reference service (`attestly-cloud`) alongside the open-source spec. Open spec wins mindshare; hosted service generates revenue. The crypto-only path will not get to $1M ARR in 24 months.

### 4.2 Short-term defensibility levers (3–6 months)

Defensibility is structurally hard to score high on for any pre-revenue infra play. Five concrete moves materially raise the floor without requiring a token or a moonshot:

1. **Be the *default* in one popular framework, not a plugin.** A plugin is copyable; being the built-in attestation layer in (say) Mastra or AutoAgents is sticky. Negotiate "Attestly-native" status in exchange for engineering work and joint marketing — not just a PR.
2. **Capture a vertical-specific schema.** Generic attestations are commoditizable. An "Attestly for healthcare claims processing" or "Attestly for KYC agents" schema, co-developed with a regulator-facing partner, is much harder to displace because it encodes domain rules, not just signatures.
3. **Own the verifier marketplace, not just the spec.** Once 3rd-party verifier agents exist on Attestly, switching costs accrue to the network of verifiers, not the format. Ship a verifier registry early — small product, large moat.
4. **Reference data set / benchmark.** Publish "AgentAudit-1" — a public benchmark of attestation/verification quality across agent frameworks. Whoever owns the benchmark in this category owns the conversation. Cost ~$10–20k. Asymmetric upside.
5. **Compliance pre-mapping as IP.** The "Attestly → SR 11-7 / EU AI Act / HIPAA clause" mapping documents are *legal interpretation work* that compounds. A buyer choosing a competitor has to redo it. Make these public; it's marketing *and* a moat.

Realistic uplift: defensibility 4 → 6.

### 4.3 Short-term distribution levers (3–6 months)

Four channel partnerships that compress the GTM timeline:

1. **Co-sell with one observability vendor.** LangSmith / Braintrust / Helicone all have sales motions into your ICP and don't have an attestation story. A referral or OEM relationship gives you their pipeline. Realistic in 6 months if approached early.
2. **MCP-server bundling.** Every MCP server is a distribution surface. Ship a one-line wrapper that adds Attestly attestations to any MCP server, then push it into the top 20 community servers. The agent-era equivalent of `npm install` distribution.
3. **Audit firm channel.** Big-4 advisory teams (Deloitte, PwC, EY) are scrambling for AI audit methodology. One signed methodology partnership = inbound enterprise pipeline you couldn't buy.
4. **Insurance partnership.** AI liability insurance is an emerging market (Munich Re, Lloyd's syndicates, Coalition). Carriers want underwriting signal — attestations are exactly that. A pilot with one carrier creates a "Attestly-attested agents qualify for lower premiums" wedge that drives adoption from the demand side.

Realistic uplift: distribution 5 → 7.

These two sections together move the overall market score from 6.5 → ~7.5 within the same six-month window and budget.

---

## 5. Six-Month Marketing & GTM Plan

**Goal by end of month 6:**
- 1 paying anchor design partner (Persona A or B), $50k+ ACV
- 5 LOIs / pilots in flight
- 2–3 OSS adopters (frameworks or agent platforms) integrating the attestation spec
- 1,500+ developers on mailing list / Discord
- Spec at v1.0 with at least one external contributor

### Month 1 — Foundation & Repositioning

**Goals:** Lock positioning, prep assets, open initial conversations.

- **Repositioning:**
  - Rewrite landing page lead. Headline: *"Cryptographic receipts for AI agents."* Subhead: *"A signed, verifiable record of every task your agents complete — for audit, billing, and dispute resolution."*
  - Move blockchain content below the fold and into a separate "On-chain anchoring" page.
  - Add three buyer-specific landing pages: `/for-compliance`, `/for-platforms`, `/for-crypto`.
- **Content seed:**
  - Long-form essay: *"What 'proof of work' means for AI agents."* Aimed at HN front page.
  - Technical deep-dive on the spec for crypto/dev audience.
- **Assets:**
  - One-pager PDF for enterprise conversations (compliance-led framing).
  - Pricing thinking exercise (don't publish yet): per-attestation, per-seat, or platform license.
- **Outreach (low-volume, high-signal):**
  - 20 hand-picked conversations: 10 regulated-enterprise AI leads (via warm intros), 5 agent platform CTOs, 5 crypto-native agent founders.
  - Goal: discovery, not selling. Validate pain language.

**Spend:** Mostly time. ~$2k for design polish and PDF.

### Month 2 — Public Launch (technical audience)

**Goals:** Establish technical credibility, recruit first OSS contributors and ecosystem partners.

- **Launch sequence (single week):**
  - Tuesday: HN Show post tied to spec v0.9 + working prototype.
  - Wednesday: Long-form essay on agent attestations published on the company blog and cross-posted to lobste.rs, X, LinkedIn.
  - Thursday: Demo video (3 min, no narration, just the protocol working end-to-end).
  - Friday: Twitter Space / X audio with 2–3 known names from agent infra space.
- **Ecosystem moves:**
  - PR to MCP spec repo proposing complementarity language ("MCP defines tool access, Attestly defines tool attestation").
  - Open issues in 3 popular agent frameworks (LangGraph, CrewAI, Mastra) proposing Attestly integration; offer to do the work.
- **Community:**
  - Discord open. Spec RFC process documented.
- **Press:** Light. The Information, Latent Space podcast pitch, Decrypt for the crypto-side angle.

**Spend:** ~$5k — video production, light PR, Twitter Space promotion.

### Month 3 — Design Partner Hunt

**Goals:** Convert one of the early conversations into a paid design partnership.

- **Target list:** 30 named accounts. Mix of:
  - Mid-market regulated firms with public agent initiatives (look for press releases mentioning agents in fintech, insurtech, healthtech).
  - Agent platforms with announced commerce features.
- **Motion:**
  - Founder-led. Custom outreach citing the prospect's specific public statements about agent governance.
  - Offer: 50% off year-one ACV, weekly engineering call, co-authored case study, name on the website. Cap at 3 design partners.
- **Product work funded by partner conversations:**
  - SOC2-readiness checklist for the hosted service (most regulated buyers will require this; start the audit prep now).
  - SDK packages for Python and TypeScript (Rust core stays the reference, but most buyers consume from Python).
- **Content:**
  - Industry-specific posts: *"Agent attestations under EU AI Act,"* *"Receipts for agent payments — what dispute resolution actually requires."*

**Spend:** ~$10k — SOC2 prep work begins, sales tooling, light paid distribution on LinkedIn for industry posts.

### Month 4 — Ecosystem Wedge

**Goals:** Get Attestly attestations *generated by default* in at least one popular agent framework.

- **Integrations shipped:**
  - LangGraph callback that emits Attestly attestations.
  - CrewAI middleware.
  - Mastra plugin.
  - At least one MCP server reference implementation that produces Attestly attestations for tool calls.
- **Crypto ecosystem:**
  - Partnership announcement with one crypto agent network (Olas, Virtuals, or similar). Co-authored post. They get a credible attestation primitive; you get distribution and mindshare in that segment.
- **First conference appearance:** Submit talk to one technical conference (LlamaCon, AI Engineer Summit, or Devcon, depending on lead). Talk title: *"Receipts for agents: what every multi-agent system gets wrong about provenance."*
- **First case study:** From the design partner signed in month 3.

**Spend:** ~$8k — conference travel, partnership announcement design, integration engineering time.

### Month 5 — Sales Repeatability

**Goals:** Move from founder-led 1:1 selling to a repeatable motion.

- **Productize the pitch:**
  - Self-serve tier of the hosted service launches. Free up to N attestations/month, paid above.
  - Public pricing page. Even imperfect pricing beats no pricing — it filters serious buyers and gives sales a shape.
- **Sales hire:** First technical AE (or fractional sales engineer). Target someone from the observability space (LangSmith, Braintrust alumni, Datadog AI side).
- **Outbound campaign:**
  - 200 named accounts, sequenced outbound, segmented by persona.
  - Two ICP-specific sales decks (regulated enterprise vs. agent platform).
- **Webinar program:**
  - One per month, paired with a partner. Format: "[Partner X] + Attestly — building auditable agents."
- **Analyst briefings:** Forrester, Gartner. Position as a category-creator in "AI agent governance / attestation."

**Spend:** ~$15k — sales hire ramp, webinar production, paid LinkedIn campaigns.

### Month 6 — Standardization Push

**Goals:** Move from product to *standard*. This is the moat.

- **Standards play:**
  - Submit Attestly to a relevant standards body or open governance process. Candidates: Linux Foundation AI & Data, OpenWallet Foundation, IETF (long shot but high signal), or a new lightweight foundation seeded with 3–5 partners.
  - Multi-company spec working group: one regulated enterprise, one agent platform, one framework, one crypto network, one identity provider.
- **Spec v1.0:** Published with semver guarantees. Backwards-compatibility commitments.
- **Reference compliance kit:**
  - "Attestly for [SR 11-7 / EU AI Act / HIPAA]" — three documents matching attestation features to specific regulatory clauses.
- **Pipeline review:**
  - At least 5 active sales pursuits at $50k+ ACV.
  - At least 2 closed-won at any size.
  - Pipeline coverage of $2M+ for the next 12 months.
- **Funding decision point:** Either you have credible $1M+ ARR trajectory and raise a focused Series A, or you've learned the buyer doesn't exist at the price you can charge — pivot or wind down honestly.

**Spend:** ~$10k — standards body fees, spec v1.0 launch event, compliance kit production.

### Six-Month Budget Total

| Category | Amount |
|---|---|
| Content / design / PR | ~$15k |
| SOC2 prep & compliance work | ~$15k |
| Conferences & events | ~$10k |
| Paid distribution (LinkedIn, X) | ~$8k |
| First sales hire (3-month ramp) | ~$60k |
| Tooling (CRM, sales engagement, webinar) | ~$8k |
| Buffer | ~$14k |
| **Total** | **~$130k** |

### Key Metrics to Watch Monthly

- Discord/mailing list growth (leading indicator of dev mindshare)
- GitHub stars + external PRs (leading indicator of standardization momentum)
- Discovery → pilot conversion rate (leading indicator of pain validation)
- Design partner check-ins per week (leading indicator of stickiness)
- Pipeline coverage (lagging indicator of GTM health)

### Failure Modes to Watch

1. **Crypto framing creep.** If the team can't resist leading with chain content, the enterprise pipeline will not materialize. Discipline this in every public artifact.
2. **A bigger player ships.** If Anthropic or LangChain releases an attestation primitive in this window, Attestly must already have a meaningful design partner and an ecosystem integration story to remain differentiated. Move fast.
3. **Spec without product.** Open spec without a hosted offering won't get to $1M ARR. The hosted service is non-negotiable for revenue.
4. **Founder bandwidth.** This plan assumes founder-led selling for months 1–4. If that's not realistic, hire the AE in month 3 instead of month 5 and slow the integration shipping pace.

---

## 6. Bottom Line

Attestly solves a real problem at a real time. The core technical thesis is right and the prototype direction is sound. The market is small in revenue dollars today but growing on a steep curve, with regulatory and commercial tailwinds both pushing in your favor over the next 6–18 months.

The win condition is becoming the **default attestation format** developers reach for when an agent does something that has to be provable later. That's a category-creation play, not a feature play, and the next six months are the right time to make it.

**Score: 6.5/10 baseline, ~7.5/10 with the §4.2/§4.3 short-term moat and distribution moves executed. Bring it to market — but lead with receipts, not the chain. See Appendix A on the token question.**

---

## Appendix A — On a Token: When, Not If

The question deserves direct treatment because it's the natural one to ask in this space, and the answer is more nuanced than the landing page's "Not a token. No incentive theater" suggests.

### A.1 Where a token genuinely helps Attestly

- **Bootstrapping a verifier network.** This is the strongest case. If you want hundreds of independent verifier agents (Persona C use case), staking + slashing economics solve the cold-start problem better than salary or grants. EigenLayer-style restaking for agent verification is a legitimate primitive.
- **Aligning agent-network operators.** Crypto-native agent platforms (Olas, Virtuals, Fetch) only really integrate things that have token economics they can plug into.
- **Capital formation.** A token round in 2026 can raise $5–20M faster than equity at this stage. Real money, real runway.
- **Liquidity for early contributors / framework integrators.** Hard to pay LangGraph maintainers in equity; easy to allocate them tokens.

### A.2 Where a token actively hurts Attestly specifically

- **The highest-value buyer (Persona A — regulated enterprise) cannot touch it.** Bank/insurer/healthco procurement will not adopt protocols with native tokens. Compliance, treasury, and legal block it. This is the killer objection — and the $1M ARR path runs through these buyers.
- **It reframes the project as crypto infra.** Re-creates the exact positioning problem flagged in §2.3, but worse and harder to reverse.
- **Token design eats founder time.** Tokenomics, legal structure, market-making, exchange listings — easily 6 months of distraction at the worst possible moment.
- **It's a credibility hit with the technical audience.** The landing page already says "Not a token. No incentive theater." Reversing that is read as a values capitulation by the developers most likely to evangelize the spec.

### A.3 The compromise that actually works in the next 12 months

Separate the protocol from any token mechanics. Run a **points / credits program** for the next 12 months — non-transferable, off-chain, used for verifier reputation and contributor recognition.

This:
- Captures ~80% of the alignment benefit
- Doesn't poison enterprise sales
- Preserves optionality to launch a token later (year 2–3) once there's a real network and a real reason
- Lets you observe whether the verifier network actually *needs* tokenized incentives, or whether reputation alone suffices

### A.4 Conditions for revisiting in 12–18 months

A token launch becomes a sensible *expansion* — not the core bet — when all four conditions are met:

1. A clearly tokenizable user base exists (active verifiers, agent operators, stakers measured in hundreds, not dozens).
2. Regulated-enterprise revenue is on a separate non-crypto SKU, in a legal entity insulated from any token entity.
3. There's a specific economic mechanism the token solves that points/credits demonstrably could not (most likely: slashing-backed verifier guarantees that buyers will pay a premium for).
4. A jurisdiction-clean legal structure is achievable (offshore foundation, US-compliant launch path, or equivalent) without compromising the operating company.

### A.5 Bottom line on token

It's a real revenue and alignment mechanism in this space, but launching one in the next 6 months trades Attestly's strongest GTM path (regulated enterprise) for its weakest in dollar terms (crypto-native, low-$ TAM today). Wrong order.

**Earn the right to a token by first building the verifier network and revenue base; *then* tokenize the network you already have.** The protocol's chain-agnostic, optional-anchoring design means this path is preserved without commitment cost — which is itself an underrated strategic asset.
