# Migration — `FileSink` → `CloudSink` without losing attestations

The common path: a team starts with `FileSink` for local development,
builds confidence, then attaches the hosted service for retention,
search, and share-links. Every attestation produced offline imports
cleanly into the cloud — the receipts you've already signed are
unchanged.

## The two-line switch

```python title="Before — local-first"
from attestly.langgraph import attest, FileSink

graph = attest(graph, agent_id="my-agent", sink=FileSink("./data/attestations.jsonl"))
```

```python title="After — hosted"
import os
from attestly.langgraph import attest, CloudSink

graph = attest(
    graph,
    agent_id="my-agent",
    sink=CloudSink(api_key=os.environ["ATTESTLY_API_KEY"]),
)
```

That's the on-going write path. New attestations land in the cloud.
But you also want the receipts you produced offline yesterday to be
searchable today — keep reading.

## Backfilling old attestations

Every attestation already in your local JSONL is a complete, signed
record. To import them into the cloud, POST each line to the ingest
endpoint:

```bash title="One-line backfill"
while read -r line; do
  curl -sS -X POST https://api.attestly.xyz/v1/attestations \
       -H "x-api-key: $ATTESTLY_API_KEY" \
       -H 'content-type: application/json' \
       -d "$line"
done < ./data/attestations.jsonl
```

The ingest endpoint is idempotent on attestation `id` — re-running the
backfill is safe.

For larger archives, a Python loop with a bounded thread pool is
straightforward:

```python title="backfill.py"
import json
import os
import requests

ENDPOINT = "https://api.attestly.xyz/v1/attestations"
HEADERS = {
    "x-api-key": os.environ["ATTESTLY_API_KEY"],
    "content-type": "application/json",
}

with open("./data/attestations.jsonl", "r", encoding="utf-8") as f:
    for line in f:
        att = json.loads(line)
        r = requests.post(ENDPOINT, json=att, headers=HEADERS, timeout=10)
        r.raise_for_status()
        print(att["id"], r.status_code)
```

Each POST returns `201 Created` on first ingest, `200 OK` on a
re-deliver. A `422 signature_invalid` means the record was tampered
with at rest — the cloud refuses to store it. That's the load-bearing
property at the boundary: a tampered receipt does not enter the
hosted database.

## Going the other direction — exporting back to JSONL

The `/v1/export` endpoint streams every attestation for your account
as JSONL — the same format `FileSink` produces. You can pipe it into
the static viewer or back into your own filesystem.

```bash
curl -H "x-api-key: $ATTESTLY_API_KEY" \
     https://attestly.xyz/v1/export > receipts.jsonl
open tools/audit-viewer/index.html
# → drag in receipts.jsonl
```

This is the **free-to-leave guarantee**: cancelling the hosted tier
does not invalidate your receipts. Their proof lives in the bytes, not
in our database.

The export endpoint stays available for 30 days after cancellation.
After that, hosted-side retention is purged per the
[pricing tier's retention promise](https://attestly.xyz/#pricing) —
but every receipt you exported stays valid forever, because the math
doesn't depend on us.

## Switching identities

Identities live in `./data/identities/<agent_id>.json` on the file
system. The cloud never needs the private key — it verifies against
the public key embedded in each attestation. So the migration story is
just "keep the same identity file, change the sink." If you regenerate
identities, the new attestations sign under the new key; both keys
appear in the cloud's per-account search filter and the viewer renders
each receipt under whichever public key signed it.

## Dual-sink during migration

If you want belt-and-braces during a cut-over, run two sinks in
parallel:

```python
from attestly.langgraph import attest, FileSink, CloudSink, CallableSink

file_sink = FileSink("./data/attestations.jsonl")
cloud_sink = CloudSink(api_key=os.environ["ATTESTLY_API_KEY"])

def dual_emit(att):
    file_sink.emit(att)
    cloud_sink.emit(att)

graph = attest(graph, agent_id="prod-01", sink=CallableSink(dual_emit))
```

Same receipts in both places, identical bytes, dual-verifiable.

## Status

The `attest()`, sink, and dual-sink wiring described on this page is
the deliverable of Step 3 in `planning/gtm-phase-2-plan.md`. The
backfill flow is unblocked by Step 2 (the hosted ingest endpoint)
and ships today.
