# Attestly Landing Page

A static, single-page landing site for Attestly. Visual language modelled on heymya.ai: dark background, generous whitespace, bold sans-serif headlines, persistent "Get a demo" CTA, problem → solution → buyers → final-CTA flow.

This is currently a thinking-tool for the founder (see `docs/LANDING.md` for the written-out version of the pitch). When GTM Month 1 starts (per `attestly-market-research.md`), this becomes the draft for the public marketing site.

## Positioning

Three stacked frames, in order of abstraction:

1. **Category claim (the layer):** *Attestly — the trust layer for what AI agents do.* The extra phrase **what agents do** distinguishes Attestly from identity-shaped "trust layer" products (Nuggets, Auth0 agent identity, agent passports), which answer *who is acting and is it allowed*. Used in `<title>`, hero eyebrow, "Where it fits" section, final CTA opener, footer tagline.
2. **Mechanism (the proof):** *Cryptographic receipts for AI agents.* Stays as the H1 — the concrete, screenshot-friendly headline that grounds the abstract layer claim.
3. **Jobs to be done:** *A signed, verifiable record of every task your agents complete — for audit, billing, and dispute resolution.* The subhead. Tells the buyer what problems get solved.

The frames don't compete. "Trust layer for what agents do" sets the category the buyer is looking for and disambiguates it from identity products; "receipts" makes it tangible enough to demo; the subhead binds it to procurement pains. This is the same shape Stripe and Auth0 use (layer / product / job).

### Competitive note

Nuggets (nuggets.life) uses *"The Trust Layer for AI Agents & Humans"* and operates in the identity slot of the stack (KYC/KYB/KYM/KYA, IAM extension, verifiable credentials). Attestly goes head-to-head on the category phrase but disambiguates on substance: identity attests to **who** acted; Attestly attests to **what they did, signed at the moment they did it, re-verified by an independent agent.** The Worker–Verifier protocol — re-execution by a second agent — is the load-bearing differentiator and is not present in identity-shaped products. The Verifier section's headline (*"The verifier doesn't read the log. It re-runs the task."*) is the category claim that nuggets cannot mirror without rebuilding their product.

## How to view it

```bash
open tools/landing-page/index.html
```

No build step. No `npm install`. Static HTML + CSS only.

## Sections

1. **Sticky nav** with brand, in-page anchors, "Get a demo" pill button
2. **Hero** — eyebrow names the trust layer; H1 is *"Cryptographic receipts for AI agents."*; bifurcated CTA ("Try it on your LangGraph agent" / "Talk to us about a pilot")
3. **Problem strip** — Sarah-shaped question + three stat callouts with cited sources
4. **How it works** — three alternating feature blocks (Worker / Verifier / Audit) with monospace code-style visuals
5. **See it work** — embedded mock audit-viewer card showing the KYC three-row demo with the tampered red row
6. **Where it fits (the stack)** — a 7-row stack diagram placing Attestly (highlighted) between Observability and Payments; concretizes the trust-layer category claim
7. **Install** — `pip install attestly-langgraph` code snippet alongside four "what you get" bullets; the developer's adoption surface
8. **Who it's for** — three buyer cards (Compliance lead, Platform operator, Cross-party counterparty) each with a "Pay-for" section
9. **Pricing** — three tiers (OSS, Team $499/mo, Enterprise "talk to us") with a "free to start, free to leave" footnote
10. **What Attestly is not** — five crossed-out scope-honest cards (observability, correctness, on-chain, framework coverage, "general AI trust product")
11. **Final CTA** — eyebrow restates the trust layer; oversized headline + dual CTA mirroring the hero
12. **Footer** — brand ("the trust layer for AI agents"), product links, resources, company, legal

## Editing notes

- **Stats in the problem section are placeholders.** The numbers (73%, $2.4M, 0) are illustrative and not from real surveys. Replace with citable data before showing this to a buyer.
- **The "Get a demo" CTA points to `mailto:hello@inertialabs.xyz`** — change to a Calendly or real form before launch.
- **The "Try it on your LangGraph agent" CTA anchors to `#install`** — until the SDK actually ships (GTM Phase 2), the install snippet is aspirational. Don't show this page externally until `pip install attestly-langgraph` actually works.
- **Pricing section is forward-looking.** The Team tier ($499/mo) and Attestly Cloud delivery surface are GTM Phase 2 deliverables. The "Start a trial" CTA must point at a real signup before launch.
- **Embedded viewer mock is hand-coded HTML**, not a live render. If the real viewer's UX changes, this snapshot needs updating to match.
- **Single file** — all CSS and HTML live in `index.html`. Vendor folders / external assets deliberately avoided so the page can be hosted anywhere (GitHub Pages, Netlify, S3) by uploading one file.

## Design intent

Per `docs/LANDING.md`, the pitch the page commits to is:

> **Cryptographic receipts for AI agents.**
> A signed, verifiable record of every task your agents complete — for audit, billing, and dispute resolution.

The blockchain / Merkle / on-chain content is deliberately not on this page. Per `attestly-market-research.md` §4.1 lock #1, on-chain anchoring is a footnote feature, not the headline. A future `/for-crypto` page would carry that content (where the existing `attestly-landing-page-v2.md` shape lives).
