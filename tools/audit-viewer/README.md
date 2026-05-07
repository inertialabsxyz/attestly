# AWP Audit Viewer

A static, single-page viewer for AWP attestation receipts. Open
[`index.html`](index.html) in a browser, drop in `attestations.json` and
`executions.json`, and read the receipts as a non-developer.

This is the buyer-facing artefact for compliance reviewers (Persona A in
[`docs/USER_JOURNEYS.md`](../../docs/USER_JOURNEYS.md)). It exists so the
people who need to act on agent receipts can do so without setting up a
Rust toolchain.

## How to use it

1. Generate sample data:

   ```bash
   cargo run --example dispatcher_flow
   ```

   This populates `data/attestations.json` and `data/executions.json`
   relative to the repo root. The two files are runtime outputs and are
   not committed.

2. Open the viewer:

   ```bash
   open tools/audit-viewer/index.html
   ```

   Or double-click the file in Finder. There is no build step,
   `npm install`, or framework — only HTML, CSS, and vanilla JavaScript.

3. Drop both `attestations.json` and `executions.json` onto the page (or
   pick them via the file picker). The timeline renders one row per
   `TaskExecutionRecord`. Click a row to see the full Worker and Verifier
   receipts side-by-side.

The viewer makes no network calls and ships no analytics. Every byte
loaded stays in the local browser tab.

## What "verified in browser" means

Each receipt the viewer renders is re-checked locally:

1. The 32-byte ed25519 public key embedded in the receipt is decoded.
2. The exact bytes the agent originally signed — the canonical encoding
   produced by `Attestation::signing_payload` in
   [`crates/awp-core/src/attestation.rs`](../../crates/awp-core/src/attestation.rs)
   — are reconstructed from the on-disk fields.
3. The signature is verified using a vendored, verify-only ed25519
   implementation in [`vendor/ed25519.js`](vendor/ed25519.js).

If the signature checks out, the row shows
**✓ signature verified in browser**. If even one byte of the receipt has
been altered after signing — output, timestamp, references, anything that
goes into the signing payload — verification fails and the row shows
**✗ signature invalid**.

This re-verification gives an auditor an independent path to confidence.
They are not trusting that the JSON file says "valid"; they are
recomputing the cryptographic check themselves, in their own browser, on
their own machine.

### What it does *not* do

- **Identity registration is out of scope.** The viewer trusts that the
  public key in each receipt belongs to the named agent. It cannot detect
  a forged identity — only an altered receipt or an unmatched signature.
  GTM Phase 1 Step 3 (persistent identity) is the prerequisite for
  meaningful identity assertions; production-grade identity binding is
  deferred beyond Phase 1.
- **Output semantics are not validated.** "Signature verified" means the
  agent attested to this exact decision; it does not mean the decision is
  *correct*. That is what the Verifier's separate attestation is for, and
  why the viewer surfaces verifier disagreements as amber rather than
  green.

### The signing-payload contract

Rust is the source of truth. `signingPayloadBytes` in
[`signing-payload.js`](signing-payload.js) reconstructs the JSON encoding
that Rust signs over: an object with these keys, in this order:

```
id, agent_id, agent_pubkey, task_hash, output_hash,
output, status, references, timestamp
```

`agent_pubkey`, `task_hash`, and `output_hash` are 32-byte arrays
serialized as hex strings (already-hex on the wire).

If you ever change `Attestation::signing_payload` on the Rust side, the
JS side must change to match. Two safety nets catch drift:

1. The page itself runs a **self-test on load** that asserts the JS
   canonical encoding of a fixed fixture matches the expected JSON byte
   string. The dropzone shows
   "✓ self-test: signing-payload bytes match Rust canonical encoding"
   when this passes; a red error if not.
2. The headless test [`tests/signing-payload.test.js`](tests/signing-payload.test.js)
   verifies every signature in `data/attestations.json` using
   `signingPayloadBytes` + the vendored ed25519 verifier. Any one-byte
   divergence from Rust's canonical encoding would cause every real
   receipt to fail verification.

## Running the headless tests

```bash
node tools/audit-viewer/tests/signing-payload.test.js
```

The end-to-end test reads `data/attestations.json`; if that file does not
exist, the e2e portion is skipped. The static parity test runs
unconditionally. Exit status is 0 on success, 1 on any failure.

## Aesthetic

Monochrome, system-font, audit-document feel. No flashy animations. The
view is meant to look like something an auditor could screenshot for a
report.

## File layout

```
tools/audit-viewer/
├── index.html                  single-page viewer
├── signing-payload.js          canonical-payload reconstruction
├── vendor/
│   └── ed25519.js              verify-only ed25519, pure JS
└── tests/
    └── signing-payload.test.js byte-parity + signature checks
```

## Threat model boundaries

The viewer is a defensive, read-only tool. Things it can detect:

- a signature whose key does not match the receipt
- a receipt mutated after signing (any field in the signing payload)
- a missing or malformed signature

Things it cannot detect:

- a receipt signed by a legitimately-issued but compromised key
- a forged attestation produced by an attacker who controls the agent's
  private key
- replay attacks (a signed receipt re-played at a later time)

Identity-binding hardening is the responsibility of the upstream key-
management story (GTM Phase 1 Step 3 onwards), not the viewer.
