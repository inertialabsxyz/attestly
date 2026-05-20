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
graph.compile().invoke({"customer_id": "4711"})
```

A signed JSONL receipt is written to `./data/attestations.jsonl` per
node by default. Swap `sink=CloudSink(api_key=...)` to ship attestations
to the hosted retention service.

There is a full runnable example at
[`examples/kyc_graph.py`](examples/kyc_graph.py) — a LangGraph port of the
GTM Phase 1 KYC demo with three scenarios (Approve, Flag, Tampered). Run it
with `python python/awp-langgraph/examples/kyc_graph.py`.

## What `attest()` does

`attest(graph, agent_id, ...)`:

1. Loads (or creates and persists) a stable Ed25519 identity under
   `./data/identities/<agent_id>.json` via `awp-core-py`'s
   `AgentIdentity.load_or_create` — the same persistent-identity store
   GTM Phase 1 landed.
2. Wraps every node callable so that, after the node returns, the SDK
   hashes the node's input state and output, builds the payload
   `{node_name, input_hash, output_hash, agent_id, timestamp}`, signs it
   via `awp.sign_attestation`, and emits the resulting attestation to the
   configured sink.
3. Returns the same graph. Node behaviour is unchanged — same outputs,
   same routing, same exception propagation.

The wrap is **idempotent** (calling `attest()` twice is a no-op) and
**behaviour-preserving** (wrapped and un-wrapped runs produce identical
state). If a node raises, no attestation is emitted — the audit log only
contains receipts for work that completed.

All cryptography is delegated to `awp-core-py`; the SDK never
re-implements signing in Python, so a Python-signed attestation verifies
byte-identically in Rust and in the static audit viewer.

## Sinks

```python
from awp.langgraph import FileSink, CloudSink, CallableSink

# 1. JSONL on disk (default if you don't pass `sink=`)
attest(g, agent_id="a", sink=FileSink("data/attestations.jsonl"))

# 2. POST to awp-cloud (services/awp-cloud/API.md)
attest(g, agent_id="a", sink=CloudSink(api_key="...", endpoint="https://app.awp-cloud.xyz"))

# 3. Escape hatch
attest(g, agent_id="a", sink=CallableSink(lambda att: my_sink(att)))
```

* **`FileSink(path)`** — append JSONL to a local file. Default when no
  sink is passed; lets a team adopt AWP with zero hosted dependency.
* **`CloudSink(api_key, endpoint)`** — POSTs to AWP Cloud per
  [`services/awp-cloud/API.md`](../../services/awp-cloud/API.md). Retries
  5xx and connection errors with exponential backoff (3 attempts). HTTP
  422 (signature failed server-side verification) raises `CloudSinkError`
  immediately — that points at an SDK bug, not the network, so silently
  retrying would mask it.
* **`CallableSink(fn)`** — escape hatch for custom destinations (your own
  queue, your own S3 prefix, your own auditor's mailbox).

## Identity management

By default, identities live in `./data/identities/<agent_id>.json` — the
persistent store landed in GTM Phase 1, wire-compatible with the Rust
`FileIdentityStore`. Override with an explicit identity:

```python
import awp
ident = awp.AgentIdentity.load_or_create("./secrets", "my-agent-01")
attest(g, agent_id="my-agent-01", identity=ident)
```

The SDK does not hold private keys beyond the lifetime of the wrapped
graph object — the identity is loaded inside `attest()`.

## Dual-agent mode (optional)

```python
attest(g, agent_id="worker-01", verifier_agent_id="verifier-01")
```

Every node runs twice — once under the Worker identity, once under the
Verifier identity — and emits two attestations per node. The Verifier
attestation's signed payload carries `references` (the Worker
attestation's id) plus the verdict (`attestation_valid`, `answer_correct`).
A non-deterministic node that produces different outputs across the two
runs makes the Verifier record `answer_correct: false` and the SDK emits a
`warning` log line. **The graph still runs to completion** — disagreement
is data, not an exception. v0.1 runs the two passes sequentially; parallel
execution is a v0.2 nice-to-have.

## Performance

Signing is delegated to `awp-core-py` (Ed25519 in Rust). The Phase 2
budget is <10 ms of attestation overhead per node. Measured on a modern
laptop (Apple Silicon, Python 3.9), building the payload and calling
`sign_attestation` costs **~0.02 ms per node** — three orders of
magnitude under budget. Re-verification (`verify_attestation`) costs
**~0.03 ms**.

Run `pytest python/awp-langgraph/tests/bench_signing.py -v -s` to
remeasure on your hardware; paste the printed number back here whenever
the signing path changes.

## Troubleshooting

**"My attestations aren't appearing in awp-cloud."** Most likely a
`CloudSink` 422 (the cloud's server-side signature re-verification failed —
almost always an SDK bug; capture the `CloudSinkError` message and file it)
or a wrong API key (surfaces as `CloudSinkError` with HTTP 401). A genuinely
unreachable cloud raises `CloudSinkUnavailable` after 3 retries.

**"Some of my nodes don't emit attestations."** The wrapper reaches into a
few different node-spec attributes (`runnable.func`, `runnable.afunc`,
`func`, `action`, `node`) to cover LangGraph 0.2.x–0.6.x. If your LangGraph
version exposes a shape we haven't accounted for, the SDK logs a `warning`
naming the node and the wrap silently does nothing for it — file a bug with
your `langgraph` version.

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

GTM Phase 2 of the AWP project. This release ships the **Step 3** SDK —
`attest()`, the three sinks, dual-agent mode — on top of the Step 5
telemetry scaffold. Cryptography is delegated to the Step 1 `awp-core-py`
bindings; the hosted `CloudSink` target is the Step 2 `awp-cloud` service.

## Documentation

Full docs (concepts, integration reference, self-hosted path,
compliance pointers, migration from `FileSink` to `CloudSink`) at
[`docs.awp-cloud.xyz`](https://docs.awp-cloud.xyz). Source under
[`docs/site/`](../../docs/site/) in this repo.

## License

Dual-licensed under Apache-2.0 OR MIT, at your option.
