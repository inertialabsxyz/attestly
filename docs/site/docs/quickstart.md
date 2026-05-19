# Quickstart — your first signed receipt in 60 seconds

The fastest path from "I read about AWP" to a green check next to a
signed receipt in your dashboard. Four steps, copy-paste-able, designed
to complete inside five minutes for a fresh user.

The hosted version of this page lives at
[`https://awp-cloud.xyz/quickstart`](https://awp-cloud.xyz/quickstart)
and includes a live signup form and a polling indicator. This page is
the same flow, written for terminals and air-gapped reading.

## 1. Sign up

```bash
curl -X POST https://awp-cloud.xyz/v1/account/signup \
     -H 'content-type: application/json' \
     -d '{"email":"you@company.com","password":"a-long-password"}'
```

The response includes your **API key** — show it once, then store it in
your secrets manager. We cannot recover it later.

```json
{
  "account_id": "5cc3...",
  "email": "you@company.com",
  "plan": "team",
  "api_key": "9f7e...-...",
  "dashboard_url": "https://app.awp-cloud.xyz/dashboard?welcome=1"
}
```

Free Team-tier trial — 1M attestations per month, no credit card. See
the [pricing page](https://awp-cloud.xyz/#pricing) for what kicks in
above the floor.

## 2. Install the SDK

```bash
pip install awp-langgraph
```

The package is pure-Python on top of `awp-core-py` (the PyO3 bindings
to the Rust signing core). The byte-identical-signatures guarantee
means receipts produced by Python verify against the static viewer the
same way Rust-produced receipts do.

## 3. Wrap your graph (five lines)

```python title="quickstart_snippet.py"
import os
from awp.langgraph import attest, CloudSink
from langgraph.graph import StateGraph

graph = build_my_graph()                     # your existing StateGraph
graph = attest(
    graph,
    agent_id="quickstart-agent",
    sink=CloudSink(api_key=os.environ["AWP_API_KEY"]),
)
graph.invoke({"hello": "world"})
```

Set `AWP_API_KEY` to the key from step 1, then run your agent. Every
node execution emits a signed attestation to the cloud.

!!! note "No LangGraph yet?"
    The OSS [`tools/audit-viewer/`](https://github.com/inertialabsxyz/awp/tree/main/tools/audit-viewer)
    ships a runnable KYC example you can drag receipts into. See
    [Self-hosted](self-hosted.md) for the offline path.

## 4. Watch the first receipt land

Open the dashboard at `https://app.awp-cloud.xyz/dashboard`. The usage
chart ticks once for each attestation. The hosted quickstart page
auto-polls `/v1/account/usage` and shows a green check the moment your
first receipt arrives — usually within seconds of running the snippet.

## What happened under the hood

1. The SDK created an Ed25519 keypair for `agent_id="quickstart-agent"`
   (or loaded it from `./data/identities/quickstart-agent.json` if one
   was already there).
2. On each node completion, it hashed the input state and the output
   delta, built a canonical-JSON attestation payload, signed it locally,
   and POSTed the result to `https://awp-cloud.xyz/v1/attestations`.
3. The cloud server **re-verified** the signature against the embedded
   public key before storing the attestation. The server never sees
   your private key.

Now go [read the Concepts](concepts.md) to understand why each of those
steps matters — or jump to the
[LangGraph integration reference](langgraph.md) for the full SDK
surface.
