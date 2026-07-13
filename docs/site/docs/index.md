# Attestly

**Signed, verifiable attestations of what your AI agents actually did.**

When agents work on your behalf, you need receipts — not just results.
Attestly is a minimal protocol that signs every agent decision at the moment
of execution and stores it in a tamper-evident log that auditors,
counterparties, and your compliance lead can verify without trusting
you.

## Three ways in

<div class="grid cards" markdown>

- :material-rocket-launch: **[Quickstart](quickstart.md)** — a 60-second
  flow from `pip install` to a first signed receipt visible in your
  hosted dashboard.

- :material-book-open-page-variant: **[Concepts](concepts.md)** — what
  an attestation is, why a second agent re-runs the task, where the
  sink fits, and which guarantees are load-bearing.

- :material-language-python: **[LangGraph integration](langgraph.md)**
  — full SDK reference. One line wraps any `StateGraph` and emits a
  signed receipt per node execution.

</div>

## Where Attestly sits

```
┌──────────────────────────────────────────────────────────────┐
│ Models                Anthropic · OpenAI · Google · open    │
├──────────────────────────────────────────────────────────────┤
│ Orchestration         LangGraph · CrewAI · AutoGen · custom │
├──────────────────────────────────────────────────────────────┤
│ Tooling               MCP                                    │
├──────────────────────────────────────────────────────────────┤
│ Identity              Auth0 / Okta agent identity · DIDs    │
├──────────────────────────────────────────────────────────────┤
│ Observability         LangSmith · Helicone · Arize · Lang…  │
├──────────────────────────────────────────────────────────────┤
│ Trust (Attestly)           Signed receipts · Worker/Verifier ·   │
│                       tamper-evident retention              │
├──────────────────────────────────────────────────────────────┤
│ Payments              Stripe Agent Toolkit · Skyfire · x402 │
└──────────────────────────────────────────────────────────────┘
```

Identity tells you *who acted*. Observability tells you *what happened
on your infrastructure*. Attestly tells you *what the agent claimed, signed
at the source, and proves the claim wasn't edited after the fact.*

## Two ways to run it

- **Self-hosted (OSS, $0)** — `attestly-langgraph` writes JSONL to a local
  `FileSink`. The [static audit viewer](self-hosted.md) re-verifies
  every signature in the browser. Works offline forever.
- **Attestly Cloud (Team / Enterprise)** — the same signed receipts,
  shipped via `CloudSink` to a hosted retention service with search,
  share links, one-year (Team) or seven-year (Enterprise) retention,
  and one-command export. See the
  [pricing page](https://attestly.xyz/#pricing) for the tiers.

You can [migrate from `FileSink` to `CloudSink`](migration.md) without
losing a single attestation — the receipts you produced offline import
cleanly into the cloud.

## For regulated buyers

The [Compliance](compliance.md) section maps Attestly onto SR 11-7 model
governance requirements. If your auditor needs a deployment-specific
mapping, that conversation lives in the design-partner programme
described in `planning/gtm-phase-2-plan.md` Step 6.
