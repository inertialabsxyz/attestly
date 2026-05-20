# LangGraph integration

`awp-langgraph` is a one-line wrapper around any LangGraph `StateGraph`
that emits a signed attestation per node execution. The wrap is
behaviour-preserving — same node outputs, same routing, same error
propagation.

## Install

```bash
pip install awp-langgraph
```

Pulls in `awp-core-py` (the PyO3 bindings to the Rust signing core)
transitively. `awp-langgraph` is pure-Python on top.

## The five lines

```python title="One-line integration"
from awp.langgraph import attest
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="my-agent-01")
graph.invoke({"hello": "world"})
```

Defaults:

- **Sink** — `FileSink("./data/attestations.jsonl")`. Switch via
  `sink=...`.
- **Identity** — loaded from
  `./data/identities/<agent_id>.json` if present, otherwise generated
  and saved on first use.
- **Telemetry** — anonymous daily aggregate on by default, opt-out via
  `AWP_TELEMETRY=0`. See the [telemetry section](#telemetry) below.

## Sinks

### `FileSink(path)`

```python
from awp.langgraph import attest, FileSink

graph = attest(graph, agent_id="local-dev", sink=FileSink("./receipts.jsonl"))
```

Appends one JSONL row per attestation. Survives crashes; tolerates
concurrent writers on the same path via `O_APPEND`. The
[static audit viewer](self-hosted.md) renders the resulting file
without a server.

### `CloudSink(api_key, endpoint=...)`

```python
import os
from awp.langgraph import attest, CloudSink

graph = attest(
    graph,
    agent_id="prod-kyc-01",
    sink=CloudSink(api_key=os.environ["AWP_API_KEY"]),
)
```

POSTs each attestation to `https://api.awp-cloud.xyz/v1/attestations`
per the [hosted ingest API contract](https://github.com/inertialabsxyz/awp/blob/main/services/awp-cloud/API.md).
Retries on `5xx` with exponential backoff (three attempts max).
A `422 signature_invalid` response raises a loud Python exception —
that indicates the SDK is broken, not the network.

### `CallableSink(fn)`

```python
from awp.langgraph import attest, CallableSink

def my_sink(attestation):
    queue.publish("audit", attestation.to_json())

graph = attest(graph, agent_id="custom", sink=CallableSink(my_sink))
```

Escape hatch for custom destinations. The SDK does not retry —
ownership of delivery is yours.

## Identity handling

The SDK reads `AgentIdentity` from
`./data/identities/<agent_id>.json` by default — the persistent store
landed in GTM Phase 1 Step 3. Override with an explicit object:

```python
from awp import AgentIdentity
from awp.langgraph import attest

identity = AgentIdentity.load_or_create("./data/identities", "my-agent-01")
graph = attest(graph, agent_id="my-agent-01", identity=identity)
```

The SDK never holds a private key in memory beyond the lifetime of the
wrapped graph object. If you `del` the graph, the key material drops
out of the process heap.

## Dual-agent mode

```python
graph = attest(
    graph,
    agent_id="worker-01",
    verifier_agent_id="verifier-01",
)
```

Runs each node twice with two identities, emits a Worker attestation
and a Verifier attestation, and surfaces disagreement as a structured
log line rather than an exception — your graph runs to completion even
when the Verifier disagrees. The disagreement signal is what your
auditor looks at after the fact.

Sequential in v0.1; parallel verifier execution is a v0.2 nice-to-have.

## Telemetry

The SDK reports a small daily aggregate to
`https://telemetry.awp-cloud.xyz/v1/usage` containing:

```json
{"install_id": "<uuid generated on first import>",
 "sdk_version": "0.1.0",
 "attestations_emitted_today": 4231,
 "sink_type": "FileSink",
 "python_version": "3.11.5",
 "os": "darwin"}
```

That is the entire payload. No payload data, no agent ids, no customer
ids, no user identifiers beyond the install UUID. The UUID is generated
locally on first import and lives at `~/.config/awp/install_id`.

Opt out with either:

```bash
export AWP_TELEMETRY=0
```

or, programmatically:

```python
from awp.langgraph import telemetry_disable
telemetry_disable()
```

With opt-out set, the SDK makes zero network calls to the telemetry
endpoint — there's a test asserting this in the package's own
`tests/test_telemetry.py`.

## Runnable examples

The package ships two runnable examples:

- `examples/quickstart_snippet.py` — the same five lines the
  [Quickstart](quickstart.md) walks through, wrapping a minimal one-node
  `StateGraph`. Runs against `CloudSink` when `AWP_API_KEY` is set, and
  falls back to `FileSink` otherwise so it works offline.
- `examples/kyc_graph.py` — a full KYC graph with three scenarios
  (Approve, Flag, and a dual-agent disagreement), the LangGraph port of
  the GTM Phase 1 `kyc_receipts` demo. The receipts it produces verify
  against the in-browser audit viewer byte-for-byte.

## Status

GTM Phase 2 Step 5 ships the telemetry module and the package layout.
The full SDK surface (`attest`, the three sinks, dual-agent mode) is
the Step 3 deliverable — see
`planning/gtm-phase-2-plan.md` for sequencing.
