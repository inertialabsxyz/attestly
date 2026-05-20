# AWP Demo Runbook

An internal presenter guide for demoing AWP. Two parts:

- **Demo A** — the LangGraph wedge and in-browser receipt verification. No
  Docker. ~5 minutes. This is the headline; it tells the whole story on its own.
- **Demo B** — the hosted `awp-cloud` surface (retention, search, share-links).
  Needs Docker. ~5 minutes more.

Every command below was run and verified. Paths assume the repo root
`/Users/andybell/devel/awp` — adjust if your checkout differs.

## Prerequisites

- A Python virtualenv at `.venv/` with the SDK installed. The examples insert
  the in-tree SDK onto `sys.path`, so `awp-core-py` (`maturin develop` in
  `crates/awp-python/`) and `langgraph` are the only hard installs.
- For Demo B only: Docker running.

---

## Demo A — The LangGraph wedge + verifiable receipts

### Setup (before the audience is watching)

```bash
cd /Users/andybell/devel/awp
make check                       # confirm the build is green
rm -f data/attestations.jsonl    # clean slate so the viewer starts empty
```

### Step 1 — Run the agent

```bash
.venv/bin/python python/awp-langgraph/examples/kyc_graph.py
```

Three scenarios print:

- **Approve** — `Customer #4711: approved.`
- **Flag** — `Customer #8842: flagged — amount exceeds threshold.`
- **Tampered (dual-agent)** — a `Worker/Verifier disagreement` log line, then
  `Customer #9999: flagged ...`. The graph still completes.

It writes 8 signed attestations to `data/attestations.jsonl`.

> **Say:** "The agent's own code didn't change. `attest(graph, agent_id=...)`
> is one line — now every node execution emits a cryptographically signed
> receipt."

### Step 2 — Show the receipts

```bash
open tools/audit-viewer/index.html
```

Drag `data/attestations.jsonl` onto the drop zone. The timeline renders
**6 node rows** (a `screen` and a `render` row per scenario):

- Scenarios 1 & 2 — **⚪ Attested** (single-agent, valid).
- Scenario 3 — the `screen` row is **🟡 Verified (disputed)** and `render` is
  **🟢 Verified**. The dual-agent disagreement shows directly in the timeline.

Click any row to expand it — each receipt shows
**"✓ signature verified in browser."**

> **Say:** "Verification runs in the auditor's browser — they don't trust our
> servers, they re-check the Ed25519 signature themselves."

### Step 3 — The tamper test (the moment that lands)

- Open `data/attestations.jsonl` in an editor.
- Change one character inside any `signature` field. Save.
- Back in the viewer, drag the file in again.
- That row flips to **🔴 Signature invalid**.

> **Say:** "A receipt altered after signing cannot pass verification. That's
> the audit guarantee — tamper-evidence, not just logging."

---

## Demo B — The hosted `awp-cloud` surface

### Step 4 — Bring up the stack

```bash
make -C services/awp-cloud up      # first build ~5 min; subsequent runs fast
```

Confirm it is healthy:

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/healthz
# expect: 200
```

### Step 5 — Seed an account and capture the API key

```bash
make -C services/awp-cloud seed
```

The seed prints `export TEST_KEY=...` and inserts 10k synthetic attestations.
Copy that key — each seed run creates a fresh account, so the key changes.

### Step 6 — Ship attestations from the SDK to the cloud

```bash
AWP_API_KEY=<TEST_KEY> \
AWP_CLOUD_ENDPOINT=http://localhost:8080 \
  .venv/bin/python python/awp-langgraph/examples/kyc_graph.py --sink cloud
```

> `AWP_CLOUD_ENDPOINT` is **required**. Without it the SDK targets the
> placeholder production domain, which does not resolve. With a bad key the
> example crashes loudly with a traceback — that is by design.

### Step 7 — Show they landed, searchably

```bash
curl -s -H "x-api-key: <TEST_KEY>" \
  'http://localhost:8080/v1/attestations?limit=10' | python3 -m json.tool
```

### Step 8 — The hosted viewer and a share-link

```bash
open http://localhost:8080
```

Search by agent. Generate a share-link and open it in an **incognito window**
— it renders publicly, no login required.

> **Say:** "The auditor gets a URL, not a VPN account. And the hosted viewer
> still re-verifies every signature client-side."

### Reset / teardown

```bash
# Full clean restart (wipes data, re-seeds):
make -C services/awp-cloud down
docker volume rm awp-cloud_pg-data awp-cloud_blob-data
make -C services/awp-cloud up
make -C services/awp-cloud seed        # prints a new TEST_KEY

# Just stop when done:
make -C services/awp-cloud down
```

---

## The narrative arc

1. **Wedge** — one line wraps any LangGraph agent, producing signed receipts
   per node.
2. **Trust** — anyone re-verifies independently; tampering is detectable.
3. **Money** — hosted retention, search, and shareable audit links are the
   paid tier.

## Honesty caveats for live delivery

- **Scenario 3** prints "flagged" as the graph output — the disagreement is
  the *verdict on the receipt*, not the graph result. Point at the
  🟡 row (or the `Worker/Verifier disagreement` log line) explicitly, or the
  audience may miss the dual-agent story.
- **Demo B's Stripe Checkout / pricing page** exists but is not part of this
  verified runbook. If billing is part of your pitch, smoke-test that flow
  privately first.
