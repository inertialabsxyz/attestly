# `awp-langgraph`

One-line LangGraph integration for the **Agent Work Protocol (AWP)** —
every node execution emits a cryptographically signed attestation to a
configurable sink (local file, AWP Cloud, or your own callable).

Install:

```bash
pip install awp-langgraph
```

Quickstart:

```python
from awp.langgraph import attest
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="kyc-worker-01")
graph.invoke({"customer_id": "4711"})
```

A signed JSONL receipt is written to `./data/attestations.jsonl` per
node by default. Swap `sink=CloudSink(api_key=...)` to ship attestations
to the hosted retention service.

## Anonymous SDK telemetry — and how to turn it off

**This SDK reports a small daily aggregate to
`https://telemetry.awp-cloud.xyz/v1/usage` on each run.** The payload is
exactly:

```json
{"install_id": "<uuid generated on first import>",
 "sdk_version": "0.1.0",
 "attestations_emitted_today": 4231,
 "sink_type": "FileSink",
 "python_version": "3.11.5",
 "os": "darwin"}
```

That is the entire payload. **No payload data. No agent ids. No customer
ids. No user identifiers beyond the install UUID.** The install UUID is
generated locally on first import and stored at
`~/.config/awp/install_id` — it is not derived from any account or user
information.

**To opt out**, set one environment variable before running your agent:

```bash
export AWP_TELEMETRY=0
```

Or, programmatically:

```python
from awp.langgraph import telemetry_disable
telemetry_disable()
```

With opt-out set, the SDK makes **zero network calls** to the telemetry
endpoint. We treat that as a load-bearing contract — there is a test
case in `tests/test_telemetry.py` that asserts it.

Why on-by-default? Conversion intelligence is what funds OSS
development — knowing which OSS users have grown into real workloads
lets us reach out and offer them the hosted retention service. The
trade-off is one env var to disable, zero PII collected, and a single
aggregate row per day per install.

If the community signal turns negative inside the first 30 days of
launch, the maintainers may flip the default to opt-IN. See
`planning/gtm-phase-2-plan.md` for the broader context.

## Status

GTM Phase 2 of the AWP project. **Step 5 scaffold** — this release
ships the telemetry module, the package layout, and the namespace
import path so the quickstart in
[`docs/site/`](../../docs/site/) and at
`https://awp-cloud.xyz/quickstart` can copy-paste working snippets.

The full SDK API surface (`attest()`, `FileSink`, `CloudSink`,
`CallableSink`, dual-agent mode) is the deliverable of **Step 3** —
see `planning/gtm-phase-2-agent-prompts.md` for the per-step sequencing
and `planning/gtm-phase-2-plan.md` for the full plan.

## Sinks (Step 3)

The Step 3 deliverable will add three sinks:

* **`FileSink(path)`** — append JSONL to a local file. Default when no
  sink is passed; lets a team adopt AWP with zero hosted dependency.
* **`CloudSink(api_key, endpoint)`** — POSTs to AWP Cloud per
  [`services/awp-cloud/API.md`](../../services/awp-cloud/API.md).
* **`CallableSink(fn)`** — escape hatch for custom destinations
  (your own queue, your own S3 prefix, your own auditor's mailbox).

## Documentation

Full docs (concepts, integration reference, self-hosted path,
compliance pointers, migration from `FileSink` to `CloudSink`) at
[`docs.awp-cloud.xyz`](https://docs.awp-cloud.xyz). Source under
[`docs/site/`](../../docs/site/) in this repo.

## License

Dual-licensed under Apache-2.0 OR MIT, at your option.
