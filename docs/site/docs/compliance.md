# Compliance

AWP is purpose-built for the procurement conversation at a regulated
buyer. The receipts AWP produces are the evidence a model-governance
examiner, a SOC 2 auditor, or a counterparty due-diligence team asks
for when they say *"show me what the agent did, and prove it wasn't
edited."*

This page points at the regulatory mappings the project maintains and
the deployment patterns that survive review.

## SR 11-7 — model risk governance

The Federal Reserve's [SR 11-7 guidance on Model Risk
Management](https://www.federalreserve.gov/supervisionreg/srletters/sr1107a1.pdf)
is the de-facto model-governance baseline at U.S. banks and many
mid-market fintechs. The AWP repo ships a pre-mapping that walks each
relevant SR 11-7 requirement onto the corresponding attestation
schema, identity model, and verification path.

**Authoritative source:**
[`docs/compliance/SR_11_7.md`](https://github.com/inertialabsxyz/awp/blob/main/docs/compliance/SR_11_7.md)
in the repo. The pre-mapping is generic; a deployment-specific mapping
(your agents, your decision points, your audit artefact format) is the
deliverable of the design-partner programme described in
`planning/gtm-phase-2-plan.md` Step 6.

## What an examiner actually wants

Three questions land in every model-governance review we've seen:

1. **Who acted?** AWP attaches an Ed25519 public key to every
   attestation; the `data/identities/<agent_id>.json` store persists
   per-agent keys across restarts so the chain of evidence survives a
   redeploy.
2. **What did they do?** Each attestation embeds the canonical bytes of
   the agent's output. Tamper with any byte — the signature breaks.
   The receipt **is** the evidence.
3. **Can I verify without trusting your platform?** Yes. The static
   audit viewer re-verifies signatures in the browser using a vendored
   Ed25519 implementation. Save the viewer to a USB stick and hand it
   to the examiner — no network call to us required.

## Worked example — KYC

The repo's `kyc_receipts` demo
([`examples/kyc_receipts.rs`](https://github.com/inertialabsxyz/awp/blob/main/examples/kyc_receipts.rs))
walks through three KYC decisions: an APPROVE, a FLAG, and a tampered
record. The viewer renders the first two green and catches the third
in red. That's the pattern that maps onto SR 11-7's "model validation
must include independent challenge" requirement: the Verifier agent is
the challenger.

Run it:

```bash
git clone https://github.com/inertialabsxyz/awp
cd awp
cargo run --example kyc_receipts
open tools/audit-viewer/index.html
# Drag in data/attestations.jsonl — green / green / red.
```

## Related frameworks

The pre-mapping in
[`docs/compliance/`](https://github.com/inertialabsxyz/awp/tree/main/docs/compliance)
covers SR 11-7 today. The architecture is portable to:

- **EU AI Act** — high-risk system audit-trail requirements
- **SOC 2 Type II** — change-management and access-control evidence
- **HIPAA** — minimum-necessary disclosure logs for healthcare agents
- **NIST AI RMF** — model risk management broadly

Specific deployment mappings ship as part of design-partner
engagements; the generic SR 11-7 mapping is the only one in the public
repo today.

## What AWP is not (compliance edition)

- **Not a SOC 2 report.** The Phase 3 deliverable is a SOC 2 Type I
  report attached to the hosted service. The OSS protocol itself does
  not require a SOC 2.
- **Not an opinion on correctness.** AWP attests to what the agent
  *claimed*. Whether that claim is right under your business rules is
  a model-validation question, not an audit-trail question. Both
  matter; AWP is purpose-built for the second.
- **Not legal advice.** The mappings on this page are best-effort
  technical alignment; consult your own counsel for jurisdiction-
  specific compliance interpretation.
