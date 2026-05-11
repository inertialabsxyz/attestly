# AWP Landing Page

A static, single-page landing site for AWP. Visual language modelled on heymya.ai: dark background, generous whitespace, bold sans-serif headlines, persistent "Get a demo" CTA, problem → solution → buyers → final-CTA flow.

This is currently a thinking-tool for the founder (see `docs/LANDING.md` for the written-out version of the pitch). When GTM Month 1 starts (per `awp-market-research.md`), this becomes the draft for the public marketing site.

## How to view it

```bash
open tools/landing-page/index.html
```

No build step. No `npm install`. Static HTML + CSS only.

## Sections

1. **Sticky nav** with brand, in-page anchors, "Get a demo" pill button
2. **Hero** — *"Cryptographic receipts for AI agents."* with bifurcated CTA ("Try it on your LangGraph agent" / "Talk to us about a pilot")
3. **Problem strip** — Sarah-shaped question + three stat callouts with cited sources
4. **How it works** — three alternating feature blocks (Worker / Verifier / Audit) with monospace code-style visuals
5. **See it work** — embedded mock audit-viewer card showing the KYC three-row demo with the tampered red row
6. **Install** — `pip install awp-langgraph` code snippet alongside four "what you get" bullets; the developer's adoption surface
7. **Who it's for** — three buyer cards (Compliance lead, Platform operator, Cross-party counterparty) each with a "Pay-for" section
8. **Pricing** — three tiers (OSS, Team $499/mo, Enterprise "talk to us") with a "free to start, free to leave" footnote
9. **What AWP is not** — four crossed-out scope-honest cards
10. **Final CTA** — oversized headline + dual CTA mirroring the hero
11. **Footer** — brand, product links, resources, company, legal

## Editing notes

- **Stats in the problem section are placeholders.** The numbers (73%, $2.4M, 0) are illustrative and not from real surveys. Replace with citable data before showing this to a buyer.
- **The "Get a demo" CTA points to `mailto:hello@inertialabs.xyz`** — change to a Calendly or real form before launch.
- **The "Try it on your LangGraph agent" CTA anchors to `#install`** — until the SDK actually ships (GTM Phase 2), the install snippet is aspirational. Don't show this page externally until `pip install awp-langgraph` actually works.
- **Pricing section is forward-looking.** The Team tier ($499/mo) and AWP Cloud delivery surface are GTM Phase 2 deliverables. The "Start a trial" CTA must point at a real signup before launch.
- **Embedded viewer mock is hand-coded HTML**, not a live render. If the real viewer's UX changes, this snapshot needs updating to match.
- **Single file** — all CSS and HTML live in `index.html`. Vendor folders / external assets deliberately avoided so the page can be hosted anywhere (GitHub Pages, Netlify, S3) by uploading one file.

## Design intent

Per `docs/LANDING.md`, the pitch the page commits to is:

> **Cryptographic receipts for AI agents.**
> A signed, verifiable record of every task your agents complete — for audit, billing, and dispute resolution.

The blockchain / Merkle / on-chain content is deliberately not on this page. Per `awp-market-research.md` §4.1 lock #1, on-chain anchoring is a footnote feature, not the headline. A future `/for-crypto` page would carry that content (where the existing `awp-landing-page-v2.md` shape lives).
