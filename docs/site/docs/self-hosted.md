# Self-hosted

AWP is open source, MIT/Apache-2.0 dual-licensed, and the SDK is fully
functional without any cloud dependency. This page is the OSS-only
path: signed receipts written to a local file, re-verified by a static
HTML viewer in your browser. No account, no network call, no us.

## When to choose self-hosted

- **Procurement won't accept a SaaS dependency.** Some regulated
  buyers require that the audit trail live on infrastructure they
  control. Self-hosted satisfies that gate without compromising the
  guarantees — every receipt is signed at the source and re-verifiable
  by anyone with the public key.
- **Local-first development.** Start with `FileSink`; switch to
  `CloudSink` later. [Migration](migration.md) is one command.
- **Cost.** Open source is $0 forever.

## Five lines

```python title="self_hosted_snippet.py"
from awp.langgraph import attest, FileSink
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="local-agent", sink=FileSink("./data/attestations.jsonl"))
graph.invoke({"hello": "world"})
```

Every node emits a signed attestation to `./data/attestations.jsonl`.
Identity files live under `./data/identities/<agent_id>.json`; both
paths stay out of git (`data/` is gitignored).

## The static audit viewer

Open
[`tools/audit-viewer/index.html`](https://github.com/inertialabsxyz/awp/tree/main/tools/audit-viewer)
in a browser. Drag your `attestations.jsonl` onto the drop zone. The
viewer:

1. Parses each line as an attestation.
2. Re-verifies the Ed25519 signature against the embedded public key.
3. Renders one row per receipt — green check on a valid signature, red
   cross on any tamper.

The viewer ships a vendored Ed25519 implementation and uses no network
fetches. You can save the entire `tools/audit-viewer/` directory to a
USB stick and hand it to your auditor with a working laptop; that's the
intended deployment for the regulated-buyer story.

## Runnable end-to-end

The Rust workspace ships a complete demo:

```bash
git clone https://github.com/inertialabsxyz/awp
cd awp

# Worker → Verifier round-trip with persistent identities.
cargo run --example kyc_receipts

# Open the viewer and drag the produced file in.
open tools/audit-viewer/index.html
# → drag data/attestations.jsonl
```

You'll see three rows. The third — the tampered scenario — flips red
on load. The Worker's signature does not match the post-tamper bytes,
and the in-browser verifier catches it.

## What you give up vs. AWP Cloud

| Feature | Self-hosted | AWP Cloud |
|---|---|---|
| Signed receipts | ✓ | ✓ |
| In-browser verification | ✓ | ✓ |
| Static audit viewer | ✓ | ✓ |
| Per-account hosted retention | — | 1y (Team) / 7y (Enterprise) |
| Search across agents / customers / time | — | ✓ |
| Share-links (auditor-grade, no auth) | — | ✓ |
| One-command export to JSONL | — | ✓ |
| Compliance templates, SR 11-7 mapping | doc only | doc + tooling |

The Cloud guarantees are operational, not cryptographic. The
**math doesn't change** between the two — a self-hosted receipt is as
verifiable as a hosted one.

## Tamper detection

Same demo, alternative ending: edit the `output` field on row 3 of
`attestations.jsonl` to flip the decision. Reload the viewer. Row 3
goes red. The signature no longer matches the edited canonical bytes.

This is the load-bearing property: **the receipt is the evidence**,
not the database it was retrieved from.

## Going hosted later

When you're ready to attach retention, search, and share links,
[Migration](migration.md) walks the offline-to-hosted transition. No
attestation produced on `FileSink` is lost; the import is a single
POST per row.
