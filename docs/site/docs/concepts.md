# Concepts

This page is the mental model. Everything else in the docs assumes
these four ideas.

## Attestation

An **attestation** is a single signed receipt that an agent produced a
specific output for a specific input at a specific moment, signed with
the agent's private key. Tamper with any field — the signature breaks.

```json title="An attestation, on the wire"
{
  "id":           "550e8400-e29b-41d4-a716-446655440000",
  "agent_id":     "kyc-worker-01",
  "agent_pubkey": "<64 hex chars — the public key the signature was made against>",
  "task_hash":    "<sha256 of the task input>",
  "output_hash":  "<sha256 of the output>",
  "output":       "<the canonical bytes the agent claims it produced>",
  "status":       "Completed",
  "timestamp":    1715456789,
  "signature":    "<ed25519 signature over the canonical signing payload>"
}
```

**The signing unit is the output, not the session.** Every decision
produces its own receipt — not a per-login credential, not a session
token. That's what makes the record evidentiary rather than
circumstantial: the auditor's question "did the agent do this on this
day for this customer" maps onto exactly one attestation.

The canonical signing-payload bytes are byte-identical across Rust,
Python, and the in-browser JS verifier — see the cross-language test
vector in `crates/attestly-core/tests/`. A receipt produced by Python
verifies against the static audit viewer the same way a Rust-produced
one does.

## Sink

A **sink** is where an attestation goes after it's signed. The SDK
supports three out of the box:

| Sink | When to use |
|---|---|
| `FileSink(path)` | Local-first development; OSS-only deployments where you don't want a hosted dependency. JSONL on disk. |
| `CloudSink(api_key, endpoint)` | Shipping to Attestly Cloud for retention, search, share links. POSTs to the [hosted ingest API](https://github.com/inertialabsxyz/attestly/blob/main/services/attestly-cloud/API.md). |
| `CallableSink(fn)` | Escape hatch — pipe attestations into your own queue, your own S3 prefix, your own auditor's mailbox. |

You can switch sinks any time without losing the receipts you already
produced — that's the [Migration](migration.md) story.

## Verifier

A **verifier** is a second, independently-identified agent that re-runs
the original task and signs its own attestation recording the verdict.
Two independent verdicts come out of a verifier run:

- `attestation_valid` — does the Worker's signature still verify?
- `answer_correct` — does the Verifier reach the same answer as the
  Worker did?

```json title="A verifier's attestation"
{
  "agent_id": "kyc-verifier-01",
  "references": "<worker_attestation_id>",
  "status": {
    "Verified": {
      "attestation_valid": true,
      "answer_correct": true
    }
  }
}
```

Disagreement is preserved as data, not hidden. The viewer renders a
disagreement row in amber rather than dropping it.

This is the load-bearing **category claim**: identity products tell
you who was allowed to act; Attestly tells you what they did and proves it
by doing it again.

## Why a tamper-evident log matters

Application logs are circumstantial. A request shows up in your access
log; a database row gets written; a metric ticks. None of those are
evidence that any specific agent produced any specific output at any
specific moment.

A signed receipt is evidence:

1. The signature is over the canonical bytes of the receipt. Edit any
   field — the signature breaks visibly.
2. The signature was made with the agent's private key. The cloud
   never sees that key; the receipt re-verifies against the public key
   embedded in the receipt itself.
3. The same verification path runs in three independent places — the
   Rust core, the Python SDK, and the in-browser JS — so an auditor can
   verify with any of them without trusting our server.

That's the property that survives a procurement review at a regulated
buyer.

## Runnable example

The OSS repo ships a runnable KYC demo at
[`crates/attestly-examples/kyc_receipts.rs`](https://github.com/inertialabsxyz/attestly/blob/main/examples/kyc_receipts.rs)
that produces three attestations — an approval, a flag, and a tampered
record — and a static viewer that renders them green / green / red.

```bash
git clone https://github.com/inertialabsxyz/attestly
cd attestly
cargo run --example kyc_receipts
open tools/audit-viewer/index.html
# Drag in data/attestations.jsonl
```

The tampered row goes red the moment the viewer re-verifies it. Same
mechanism that catches a database edit in production.
